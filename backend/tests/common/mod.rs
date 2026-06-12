//! Test fixture: levanta el router de Faro en puertos efímeros contra el
//! ClickHouse real (`CLICKHOUSE_URL` del entorno; default = `localhost:8123`),
//! con un proyecto único por test para que las filas de uno no contaminen al
//! siguiente. Equivalente a lo que el job `backend` de `.github/workflows/ci.yml`
//! ya provisiona, así que en CI no hace falta nada extra.

#![allow(dead_code)] // no todos los tests usan todos los helpers

use std::sync::{Arc, Once};
use std::time::Duration;

use chrono::Utc;
use faro::api;
use faro::auth;
use faro::config::Config;
use faro::ingest;
use faro::state::{AppState, SharedState};
use faro::storage::{Client, ProjectRow};
use faro::workers;
use reqwest::Client as HttpClient;
use tokio::net::TcpListener;
use uuid::Uuid;

static INIT: Once = Once::new();

fn init_tracing() {
    INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
            )
            .with_test_writer()
            .try_init();
    });
}

fn env_or(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}

/// Construye un `Config` con defaults razonables para tests: CH leído de env
/// vars (alineados con el job de CI), batch flush a 50 ms (vs 750 ms de prod)
/// para que `wait_for` no se quede largo, y workers que en tests no aportan
/// (anomaly, fingerprint compactor, stale) desactivados.
pub fn test_config() -> Config {
    Config {
        api_addr: "127.0.0.1:0".into(),
        otlp_addr: "127.0.0.1:0".into(),
        otlp_grpc_addr: "127.0.0.1:0".into(),
        clickhouse_url: env_or("CLICKHOUSE_URL", "http://localhost:8123"),
        clickhouse_database: env_or("CLICKHOUSE_DATABASE", "faro"),
        clickhouse_user: env_or("CLICKHOUSE_USER", "faro"),
        clickhouse_password: env_or("CLICKHOUSE_PASSWORD", "faro"),
        clickhouse_pool_max_idle: 8,
        sse_max_per_project: 10,
        sse_max_global: 100,
        redis_url: None,
        batch_max_rows: 100,
        batch_flush_ms: 50,
        ingest_rate_per_second: 100_000,
        metrics_token: None,
        public_base_url: "http://test".into(),
        telegram_bot_token: None,
        telegram_api_base: "https://api.telegram.org".into(),
        dashboard_origins: vec![],
        enable_hsts: false,
        anomaly_enabled: false,
        anomaly_interval_secs: 300,
        anomaly_window_minutes: 5,
        anomaly_z_fire: 3.0,
        anomaly_z_resolve: 1.5,
        anomaly_min_baseline_errors: 2.0,
        anomaly_min_baseline_p95_ms: 20.0,
        anomaly_min_baseline_logs: 50.0,
        feature_rollback_enabled: false,
        feature_rollback_interval_secs: 300,
        feature_rollback_window_minutes: 15,
        feature_rollback_ratio: 5.0,
        feature_rollback_resolve_ratio: 2.0,
        feature_rollback_min_sample: 20,
        feature_rollback_min_treatment_errors: 5,
        fingerprint_compactor_enabled: false,
        fingerprint_compactor_interval_secs: 1800,
        fingerprint_compactor_jaccard: 0.85,
        stale_detector_enabled: false,
        stale_detector_interval_secs: 3600,
        stale_threshold_hours: 24,
        user_unifier_enabled: false,
        user_unifier_interval_secs: 60,
        session_aggregator_enabled: false,
        session_aggregator_interval_secs: 300,
        session_aggregator_gap_minutes: 30,
        session_aggregator_lookback_minutes: 360,
    }
}

pub struct TestApp {
    pub api_url: String,
    pub otlp_url: String,
    pub state: SharedState,
    pub ch: Client,
    pub project_slug: String,
    pub project_token: String,
    pub http: HttpClient,
}

impl TestApp {
    pub async fn spawn() -> Self {
        init_tracing();
        let cfg = test_config();
        let ch = Client::new(&cfg).await.expect("CH client");
        ch.wait_until_ready().await.expect("CH ready");

        let state = Arc::new(AppState::new(cfg, ch.clone()));

        // Proyecto único antes de arrancar workers/listeners — así el primer
        // POST de ingesta ya encuentra el token en caché.
        let project_slug = format!("test-{}", Uuid::new_v4().simple());
        let project_token = format!("tok-{}", Uuid::new_v4().simple());
        insert_project(&ch, &project_slug, &project_token).await;
        state.projects.reload(&ch).await.expect("reload projects");

        workers::start_ingest_writers(state.clone());

        let api_router = api::router(state.clone());
        let otlp_router = ingest::otlp_router(state.clone());

        let api_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind api");
        let api_addr = api_listener.local_addr().expect("api addr");
        let otlp_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind otlp");
        let otlp_addr = otlp_listener.local_addr().expect("otlp addr");

        tokio::spawn(async move {
            let _ = axum::serve(api_listener, api_router).await;
        });
        tokio::spawn(async move {
            let _ = axum::serve(otlp_listener, otlp_router).await;
        });

        // Sin cookie_store: la cookie `faro_session` se emite con `Secure=true`
        // y reqwest no la reusaría sobre HTTP de todas formas. Los tests
        // reenvían el valor a mano vía header `Cookie:`.
        let http = HttpClient::builder().build().expect("http client");

        TestApp {
            api_url: format!("http://{}", api_addr),
            otlp_url: format!("http://{}", otlp_addr),
            state,
            ch,
            project_slug,
            project_token,
            http,
        }
    }

    /// Espera hasta `attempts * 50 ms` a que el predicate devuelva true. Útil
    /// para los tests de ingesta: la inserta es async-flush, no instantánea.
    pub async fn wait_for<F, Fut>(&self, attempts: u32, mut f: F) -> bool
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        for _ in 0..attempts {
            if f().await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    pub async fn count_in(&self, table: &str) -> u64 {
        #[derive(serde::Deserialize)]
        struct Cnt {
            count: u64,
        }
        let sql = format!(
            "SELECT toUInt64(count()) AS count FROM faro.{table} \
             WHERE project_id = {{p:String}}"
        );
        let row: Option<Cnt> = self
            .ch
            .select_one_with_params(&sql, &[("p", &self.project_slug)])
            .await
            .expect("select count");
        row.map(|c| c.count).unwrap_or(0)
    }

    /// Crea un user admin con password conocido en `faro.users`. Devuelve el
    /// email; el caller usa `password` para hacer login. Email y password se
    /// generan aleatorios para no chocar entre tests paralelos.
    pub async fn create_user(&self, password: &str) -> String {
        let email = format!("u-{}@test.local", Uuid::new_v4().simple());
        let now = Utc::now();
        let row = auth::UserRow {
            id: Uuid::new_v4(),
            email: email.clone(),
            password_hash: auth::hash_password(password).expect("hash pwd"),
            name: "Test User".into(),
            role: "admin".into(),
            created_at: now,
            updated_at: now,
            deleted: 0,
            version: now.timestamp_millis() as u64,
            totp_secret: String::new(),
            totp_enabled: 0,
        };
        self.ch
            .insert("faro.users", &[row])
            .await
            .expect("insert user");
        email
    }

    /// Login HTTP y devuelve el valor crudo del cookie `faro_session`. La
    /// cookie se emite con `Secure=true`, así que `reqwest::cookie_store` no
    /// la reusa sobre HTTP — extraemos el valor del header `Set-Cookie` y lo
    /// reenviamos a mano en los siguientes requests.
    pub async fn login_session(&self, email: &str, password: &str) -> String {
        let resp = self
            .http
            .post(format!("{}/api/v1/auth/login", self.api_url))
            .json(&serde_json::json!({ "email": email, "password": password }))
            .send()
            .await
            .expect("login send");
        assert!(
            resp.status().is_success(),
            "login status: {}",
            resp.status()
        );
        extract_session_cookie(resp.headers()).expect("login response sin cookie faro_session")
    }
}

async fn insert_project(ch: &Client, slug: &str, token: &str) {
    let now = Utc::now();
    let row = ProjectRow {
        id: Uuid::new_v4(),
        slug: slug.into(),
        name: slug.into(),
        description: "integration test".into(),
        ingest_token: token.into(),
        redaction_rules: String::new(),
        allowed_origins: String::new(),
        created_at: now,
        updated_at: now,
        deleted: 0,
        version: now.timestamp_millis() as u64,
    };
    ch.insert("faro.projects", &[row])
        .await
        .expect("insert project");
}

/// Devuelve el valor del cookie `faro_session` si el header `Set-Cookie` lo
/// contiene. Necesario porque la cookie se emite con `Secure` y el cliente
/// HTTP no la reusa sobre HTTP plano.
pub fn extract_session_cookie(headers: &reqwest::header::HeaderMap) -> Option<String> {
    for v in headers.get_all(reqwest::header::SET_COOKIE).iter() {
        let s = v.to_str().ok()?;
        let head = s.split(';').next()?.trim();
        if let Some(val) = head.strip_prefix("faro_session=") {
            return Some(val.to_string());
        }
    }
    None
}
