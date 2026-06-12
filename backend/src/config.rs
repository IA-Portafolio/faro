//! Configuración del backend, leída del entorno (variables `FARO_*`, `CLICKHOUSE_*`).
//!
//! `Config` reúne las direcciones de escucha (API + OTLP HTTP/gRPC), la conexión a
//! ClickHouse, los topes de pooling y de subscriptores SSE, los tokens y los
//! parámetros de arranque (bootstrap). Cada campo documenta el porqué de su default.

use anyhow::Result;

#[derive(Clone, Debug)]
pub struct Config {
    pub api_addr: String,
    pub otlp_addr: String,
    /// Listener OTLP/gRPC en `:4317` por defecto, separado del de HTTP/JSON
    /// (`:4318`) porque los SDKs oficiales de OpenTelemetry usan gRPC.
    pub otlp_grpc_addr: String,
    pub clickhouse_url: String,
    pub clickhouse_database: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
    /// Máximo de conexiones HTTP idle que el cliente mantiene cacheadas hacia
    /// ClickHouse. El default de reqwest es `usize::MAX` (ilimitado), lo que
    /// bajo bursts hace crecer el pool sin freno y nunca lo recorta. 64 cubre
    /// el paralelismo realista (ingest writers + workers + dashboard queries)
    /// dejando margen, y acota RAM/FDs. Configurable vía
    /// `FARO_CLICKHOUSE_POOL_MAX_IDLE`.
    pub clickhouse_pool_max_idle: usize,
    /// Tope de subscriptores SSE simultáneos por proyecto (clave "*" para los
    /// que no piden filtro de proyecto). Sin tope, un cliente abriendo tabs
    /// en loop o un atacante pueden inflar el broadcast channel y el número
    /// de conexiones HTTP. Default 10 — suficiente para un equipo abriendo
    /// varios tabs/dashboards del mismo proyecto. Configurable vía
    /// `FARO_SSE_MAX_PER_PROJECT`.
    pub sse_max_per_project: usize,
    /// Tope global de subscriptores SSE en el proceso. Acota el daño aun cuando
    /// el atacante itere por proyectos para evitar el cap por-proyecto. Default
    /// 100. Configurable vía `FARO_SSE_MAX_GLOBAL`.
    pub sse_max_global: usize,
    pub redis_url: Option<String>,
    pub batch_max_rows: usize,
    pub batch_flush_ms: u64,
    /// Records/segundo aceptados por proyecto en cualquier endpoint de ingesta
    /// (OTLP/HTTP, OTLP/gRPC, `/logs`). Burst = 2× este valor. Por defecto 5000
    /// — suficiente para flujo legítimo y atrapa loops accidentales que mandan
    /// miles/seg por accidente. Configurable vía `FARO_INGEST_RATE_PER_SECOND`.
    pub ingest_rate_per_second: u32,
    /// `/metrics` exige `Authorization: Bearer <token>` y rechaza cualquier
    /// otra cosa con 401. Si no está definido, devuelve 401 (fail-closed) —
    /// en ese caso no hay forma de acceder a las métricas internas.
    /// Configurable vía `FARO_METRICS_TOKEN`.
    pub metrics_token: Option<String>,
    pub public_base_url: String,
    /// Token global del bot de Telegram. Si se define, los targets `tg://<chat_id>`
    /// usan este bot. Los targets `tg://<chat_id>@<token>` siempre pueden traer
    /// su propio token y no necesitan que esté configurado a nivel global.
    pub telegram_bot_token: Option<String>,
    /// Base de la API de Telegram. Configurable solo para pruebas — en producción
    /// se usa el valor por defecto.
    pub telegram_api_base: String,
    /// Orígenes permitidos para CORS en el API del dashboard (`:8080`).
    /// Lista separada por comas: `https://faro.example.com,https://app.example.com`.
    /// Si está vacío (dev), permite cualquier origen sin credenciales.
    /// En producción deberías definir `FARO_DASHBOARD_ORIGINS` para que el browser
    /// sólo envíe la cookie de sesión hacia los orígenes conocidos y no hacia
    /// cualquier sitio que lo solicite. Configurable vía `FARO_DASHBOARD_ORIGINS`.
    pub dashboard_origins: Vec<String>,
    /// Si `true`, el backend agrega `Strict-Transport-Security: max-age=31536000;
    /// includeSubDomains` a las respuestas del dashboard. Default: `false` porque
    /// el browser cachea HSTS por un año por origen, lo que rompe testing en HTTP
    /// (incluyendo `localhost:8080` si ya se hizo HTTPS contra el mismo host).
    /// Encender sólo en producción con TLS estable, o dejarlo apagado y que el
    /// reverse proxy (Caddy/nginx) inyecte el header. Configurable vía
    /// `FARO_ENABLE_HSTS=true`.
    pub enable_hsts: bool,
    /// Detector de anomalías por z-score. Cuando está activo, un worker corre
    /// cada `anomaly_interval_secs` y compara la tasa actual de cada
    /// (proyecto, servicio, señal) contra la misma franja horaria de los
    /// últimos 7 días. Dispara incidentes en `faro.alert_incidents` cuando
    /// el z-score supera `anomaly_z_fire` y los resuelve cuando baja de
    /// `anomaly_z_resolve` (hysteresis para no aletear en el borde).
    pub anomaly_enabled: bool,
    pub anomaly_interval_secs: u64,
    /// Ventana de la observación actual y de cada muestra histórica, en minutos.
    /// 5 es un buen balance: lo bastante pequeño para detectar spikes cortos,
    /// lo bastante grande para que los conteos no sean ruido de Poisson.
    pub anomaly_window_minutes: u32,
    pub anomaly_z_fire: f64,
    pub anomaly_z_resolve: f64,
    /// Baseline mínimo (en la unidad de la señal) para considerar una serie
    /// "interesante". Si la media histórica es menor, no se evalúa — evita
    /// dispararse cuando una sola observación contra una media casi-cero
    /// produce un z-score astronómico.
    pub anomaly_min_baseline_errors: f64,
    pub anomaly_min_baseline_p95_ms: f64,
    pub anomaly_min_baseline_logs: f64,
    /// Detector de rollback recomendado para feature flags. Une exposures de
    /// `product_events` con `error_events` vía `trace_id` y dispara incidentes
    /// cuando la variante B tiene una tasa de error mucho mayor que A.
    pub feature_rollback_enabled: bool,
    pub feature_rollback_interval_secs: u64,
    pub feature_rollback_window_minutes: u32,
    pub feature_rollback_ratio: f64,
    pub feature_rollback_resolve_ratio: f64,
    pub feature_rollback_min_sample: u64,
    pub feature_rollback_min_treatment_errors: u64,
    /// Compactador MinHash de fingerprints. Cada `fingerprint_compactor_interval_secs`
    /// agrupa errores semánticamente equivalentes (Jaccard ≥ `fingerprint_compactor_jaccard`)
    /// que el hash exacto de `fingerprint.rs` deja como issues separados.
    pub fingerprint_compactor_enabled: bool,
    pub fingerprint_compactor_interval_secs: u64,
    pub fingerprint_compactor_jaccard: f64,
    /// Detector de servicios stale. Cada `stale_detector_interval_secs` revisa
    /// `services_seen` y emite eventos cuando un servicio cruza
    /// `stale_threshold_hours` sin tráfico (o cuando vuelve).
    pub stale_detector_enabled: bool,
    pub stale_detector_interval_secs: u64,
    pub stale_threshold_hours: u32,
    /// Unificador de usuarios multi-device (goal 10.E.1). Cada
    /// `user_unifier_interval_secs` agrega `product_events` recientes y mantiene
    /// `product_users` + `product_user_aliases`. Sin este worker, las dos tablas
    /// quedan vacías y "ver todos los eventos del user X en cualquier device"
    /// requiere escanear `product_events` entero.
    pub user_unifier_enabled: bool,
    pub user_unifier_interval_secs: u64,
    /// Session aggregator (goal 10.F.1). Cada `session_aggregator_interval_secs`
    /// sesionaliza `product_events` recientes y mantiene `product_sessions`. Si el
    /// SDK manda `session_id` en el evento, se respeta; si no, se cortan sesiones
    /// por gap > `session_aggregator_gap_minutes` (default 30 — convención
    /// GA/Mixpanel). El worker mira siempre `session_aggregator_lookback_minutes`
    /// hacia atrás; mantenerlo ≥ a la duración máxima esperada de una sesión activa
    /// evita drift de `started_at` cuando el primer evento se cae de la ventana.
    pub session_aggregator_enabled: bool,
    pub session_aggregator_interval_secs: u64,
    pub session_aggregator_gap_minutes: u32,
    pub session_aggregator_lookback_minutes: u32,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            api_addr: env_or("FARO_API_ADDR", "0.0.0.0:8080"),
            otlp_addr: env_or("FARO_BIND_ADDR", "0.0.0.0:4318"),
            otlp_grpc_addr: env_or("FARO_OTLP_GRPC_ADDR", "0.0.0.0:4317"),
            clickhouse_url: env_or("CLICKHOUSE_URL", "http://localhost:8123"),
            clickhouse_database: env_or("CLICKHOUSE_DATABASE", "faro"),
            clickhouse_user: env_or("CLICKHOUSE_USER", "faro"),
            clickhouse_password: env_or("CLICKHOUSE_PASSWORD", "faro"),
            // `pool_max_idle_per_host(0)` en reqwest DESACTIVA el pool (abre TCP nuevo
            // por cada request). Para evitar que un typo en el env tire el throughput
            // del ingest, clampeamos a >= 1. Si alguien realmente quiere desactivar el
            // pool, hay que hacerlo por código, no por config accidental.
            clickhouse_pool_max_idle: env_or("FARO_CLICKHOUSE_POOL_MAX_IDLE", "64")
                .parse::<usize>()
                .unwrap_or(64)
                .max(1),
            sse_max_per_project: env_or("FARO_SSE_MAX_PER_PROJECT", "10")
                .parse()
                .unwrap_or(10),
            sse_max_global: env_or("FARO_SSE_MAX_GLOBAL", "100").parse().unwrap_or(100),
            redis_url: std::env::var("REDIS_URL").ok(),
            batch_max_rows: env_or("FARO_BATCH_MAX_ROWS", "5000")
                .parse()
                .unwrap_or(5000),
            batch_flush_ms: env_or("FARO_BATCH_FLUSH_MS", "750").parse().unwrap_or(750),
            ingest_rate_per_second: env_or("FARO_INGEST_RATE_PER_SECOND", "5000")
                .parse()
                .unwrap_or(5000),
            metrics_token: std::env::var("FARO_METRICS_TOKEN")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            public_base_url: env_or("FARO_PUBLIC_BASE_URL", "http://localhost:8080"),
            telegram_bot_token: std::env::var("TELEGRAM_BOT_TOKEN")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            telegram_api_base: env_or("TELEGRAM_API_BASE", "https://api.telegram.org"),
            dashboard_origins: std::env::var("FARO_DASHBOARD_ORIGINS")
                .ok()
                .map(|s| {
                    s.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            enable_hsts: matches!(
                env_or("FARO_ENABLE_HSTS", "false").to_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            ),
            anomaly_enabled: matches!(
                env_or("FARO_ANOMALY_ENABLED", "true")
                    .to_lowercase()
                    .as_str(),
                "1" | "true" | "yes" | "on"
            ),
            anomaly_interval_secs: env_or("FARO_ANOMALY_INTERVAL_SECS", "300")
                .parse()
                .unwrap_or(300),
            anomaly_window_minutes: env_or("FARO_ANOMALY_WINDOW_MINUTES", "5")
                .parse()
                .unwrap_or(5),
            anomaly_z_fire: env_or("FARO_ANOMALY_Z_FIRE", "3.0").parse().unwrap_or(3.0),
            anomaly_z_resolve: env_or("FARO_ANOMALY_Z_RESOLVE", "1.5")
                .parse()
                .unwrap_or(1.5),
            anomaly_min_baseline_errors: env_or("FARO_ANOMALY_MIN_BASELINE_ERRORS", "2.0")
                .parse()
                .unwrap_or(2.0),
            anomaly_min_baseline_p95_ms: env_or("FARO_ANOMALY_MIN_BASELINE_P95_MS", "20.0")
                .parse()
                .unwrap_or(20.0),
            anomaly_min_baseline_logs: env_or("FARO_ANOMALY_MIN_BASELINE_LOGS", "50.0")
                .parse()
                .unwrap_or(50.0),
            feature_rollback_enabled: matches!(
                env_or("FARO_FEATURE_ROLLBACK_ENABLED", "true")
                    .to_lowercase()
                    .as_str(),
                "1" | "true" | "yes" | "on"
            ),
            feature_rollback_interval_secs: env_or("FARO_FEATURE_ROLLBACK_INTERVAL_SECS", "300")
                .parse()
                .unwrap_or(300),
            feature_rollback_window_minutes: env_or("FARO_FEATURE_ROLLBACK_WINDOW_MINUTES", "15")
                .parse()
                .unwrap_or(15),
            feature_rollback_ratio: env_or("FARO_FEATURE_ROLLBACK_RATIO", "5.0")
                .parse()
                .unwrap_or(5.0),
            feature_rollback_resolve_ratio: env_or("FARO_FEATURE_ROLLBACK_RESOLVE_RATIO", "2.0")
                .parse()
                .unwrap_or(2.0),
            feature_rollback_min_sample: env_or("FARO_FEATURE_ROLLBACK_MIN_SAMPLE", "20")
                .parse()
                .unwrap_or(20),
            feature_rollback_min_treatment_errors: env_or(
                "FARO_FEATURE_ROLLBACK_MIN_TREATMENT_ERRORS",
                "5",
            )
            .parse()
            .unwrap_or(5),
            fingerprint_compactor_enabled: matches!(
                env_or("FARO_FP_COMPACTOR_ENABLED", "true")
                    .to_lowercase()
                    .as_str(),
                "1" | "true" | "yes" | "on"
            ),
            fingerprint_compactor_interval_secs: env_or("FARO_FP_COMPACTOR_INTERVAL_SECS", "1800")
                .parse()
                .unwrap_or(1800),
            fingerprint_compactor_jaccard: env_or("FARO_FP_COMPACTOR_JACCARD", "0.85")
                .parse()
                .unwrap_or(0.85),
            stale_detector_enabled: matches!(
                env_or("FARO_STALE_DETECTOR_ENABLED", "true")
                    .to_lowercase()
                    .as_str(),
                "1" | "true" | "yes" | "on"
            ),
            stale_detector_interval_secs: env_or("FARO_STALE_DETECTOR_INTERVAL_SECS", "3600")
                .parse()
                .unwrap_or(3600),
            stale_threshold_hours: env_or("FARO_STALE_THRESHOLD_HOURS", "24")
                .parse()
                .unwrap_or(24),
            user_unifier_enabled: matches!(
                env_or("FARO_USER_UNIFIER_ENABLED", "true")
                    .to_lowercase()
                    .as_str(),
                "1" | "true" | "yes" | "on"
            ),
            user_unifier_interval_secs: env_or("FARO_USER_UNIFIER_INTERVAL_SECS", "60")
                .parse()
                .unwrap_or(60),
            session_aggregator_enabled: matches!(
                env_or("FARO_SESSION_AGGREGATOR_ENABLED", "true")
                    .to_lowercase()
                    .as_str(),
                "1" | "true" | "yes" | "on"
            ),
            session_aggregator_interval_secs: env_or(
                "FARO_SESSION_AGGREGATOR_INTERVAL_SECS",
                "300",
            )
            .parse()
            .unwrap_or(300),
            session_aggregator_gap_minutes: env_or("FARO_SESSION_GAP_MINUTES", "30")
                .parse()
                .unwrap_or(30),
            session_aggregator_lookback_minutes: env_or("FARO_SESSION_LOOKBACK_MINUTES", "360")
                .parse()
                .unwrap_or(360),
        })
    }
}

fn env_or(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}
