//! SQL injection regression. Hoy todos los handlers de query pasan el input
//! del usuario a ClickHouse como parámetros server-bound (`{name:String}`),
//! lo cual es seguro por construcción. Estos tests son la red de seguridad
//! para detectar regresiones si alguien reintroduce `format!("... {} ...")`
//! con input no sanitizado, o si se vuelve a apoyar en un `escape_sql()`
//! casero como menciona el riesgo en 3.1.
//!
//! Para cada endpoint × param, mandamos 3 payloads clásicos y verificamos:
//!   - status ∈ {200, 4xx controlado}, NUNCA 5xx (panic / SQL crudo).
//!   - el body NO contiene marcas de leak (password_hash, argon2).
//!   - las tablas críticas siguen presentes al final.

mod common;

use common::TestApp;
use reqwest::StatusCode;

const PAYLOADS: &[&str] = &[
    "x' OR '1'='1",
    "x'; DROP TABLE faro.logs; --",
    "x' UNION SELECT password_hash FROM users--",
    "' OR 1=1 --",
    "x\"; SELECT 1; --",
];

/// Marca un status como "aceptable" para inputs maliciosos: cualquier 2xx o
/// 4xx es un fallo controlado. Un 5xx significa que la query rompió el server
/// (panic, SQL no parametrizado que CH rechaza con error 500, etc.).
fn assert_controlled(label: &str, payload: &str, status: StatusCode, body: &str) {
    assert!(
        status.is_success() || status.is_client_error(),
        "[{label}] payload {payload:?} causó status no controlado: {status} body={body:.200}"
    );
    // Si una regresión hace que el SQL ejecute y traiga password_hash, queremos
    // verlo aquí — esos campos no aparecen en NINGUNA respuesta legítima de
    // estos endpoints.
    let lowered = body.to_lowercase();
    assert!(
        !lowered.contains("password_hash"),
        "[{label}] payload {payload:?} filtró 'password_hash' en el body"
    );
    assert!(
        !lowered.contains("argon2"),
        "[{label}] payload {payload:?} filtró un hash argon2 en el body"
    );
}

async fn get_q(
    app: &TestApp,
    session: &str,
    path: &str,
    query: &[(&str, &str)],
) -> (StatusCode, String) {
    let resp = app
        .http
        .get(format!("{}{}", app.api_url, path))
        .query(query)
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send");
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    (status, body)
}

async fn get_path(app: &TestApp, session: &str, url: String) -> (StatusCode, String) {
    let resp = app
        .http
        .get(url)
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send");
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    (status, body)
}

#[tokio::test]
async fn logs_query_params_resist_sql_injection() {
    let app = TestApp::spawn().await;
    let email = app.create_user("inj-pw").await;
    let session = app.login_session(&email, "inj-pw").await;

    let project = app.project_slug.clone();
    for p in PAYLOADS {
        for (param, label) in [
            ("service", "logs.service"),
            ("query", "logs.query"),
            ("trace_id", "logs.trace_id"),
            ("project", "logs.project"),
        ] {
            let q: Vec<(&str, &str)> = if param == "project" {
                vec![(param, p)]
            } else {
                vec![("project", project.as_str()), (param, p)]
            };
            let (status, body) = get_q(&app, &session, "/api/v1/logs", &q).await;
            assert_controlled(label, p, status, &body);
        }
    }
}

#[tokio::test]
async fn traces_query_params_resist_sql_injection() {
    let app = TestApp::spawn().await;
    let email = app.create_user("inj-pw").await;
    let session = app.login_session(&email, "inj-pw").await;

    let project = app.project_slug.clone();
    for p in PAYLOADS {
        for (param, label) in [
            ("service", "traces.service"),
            ("status", "traces.status"),
            ("project", "traces.project"),
        ] {
            let q: Vec<(&str, &str)> = if param == "project" {
                vec![(param, p)]
            } else {
                vec![("project", project.as_str()), (param, p)]
            };
            let (status, body) = get_q(&app, &session, "/api/v1/traces", &q).await;
            assert_controlled(label, p, status, &body);
        }

        // Path param `:trace_id` — el handler lo bind-ea como `{trace_id:String}`.
        // Usamos un cliente HTTP de bajo nivel: para meter caracteres especiales
        // en el path usamos `Url::parse_with_params` no aplica, así que
        // construimos la URL ya percent-encoded con `url::form_urlencoded`.
        let enc = percent_encode_path(p);
        let url = format!("{}/api/v1/traces/{}", app.api_url, enc);
        let (status, body) = get_path(&app, &session, url).await;
        assert_controlled("traces/:id", p, status, &body);
    }
}

#[tokio::test]
async fn metrics_query_params_resist_sql_injection() {
    let app = TestApp::spawn().await;
    let email = app.create_user("inj-pw").await;
    let session = app.login_session(&email, "inj-pw").await;

    let project = app.project_slug.clone();
    for p in PAYLOADS {
        // /metrics/series exige `name` — probamos el name malicioso y luego
        // service/project con un name benigno.
        let (status, body) = get_q(&app, &session, "/api/v1/metrics/series", &[("name", p)]).await;
        assert_controlled("metrics.name", p, status, &body);

        for (param, label) in [
            ("service", "metrics.service"),
            ("project", "metrics.project"),
        ] {
            let q: Vec<(&str, &str)> = if param == "project" {
                vec![("name", "cpu"), (param, p)]
            } else {
                vec![("name", "cpu"), ("project", project.as_str()), (param, p)]
            };
            let (status, body) = get_q(&app, &session, "/api/v1/metrics/series", &q).await;
            assert_controlled(label, p, status, &body);
        }

        // /metrics/names solo toma `project` (entre los relevantes).
        let (status, body) =
            get_q(&app, &session, "/api/v1/metrics/names", &[("project", p)]).await;
        assert_controlled("metrics.names.project", p, status, &body);
    }
}

#[tokio::test]
async fn errors_query_params_resist_sql_injection() {
    let app = TestApp::spawn().await;
    let email = app.create_user("inj-pw").await;
    let session = app.login_session(&email, "inj-pw").await;

    let project = app.project_slug.clone();
    for p in PAYLOADS {
        for (param, label) in [
            ("service", "errors.service"),
            ("status", "errors.status"),
            ("project", "errors.project"),
        ] {
            let q: Vec<(&str, &str)> = if param == "project" {
                vec![(param, p)]
            } else {
                vec![("project", project.as_str()), (param, p)]
            };
            let (status, body) = get_q(&app, &session, "/api/v1/errors", &q).await;
            assert_controlled(label, p, status, &body);
        }

        // Path param `:fingerprint` en errors/:fp.
        let enc = percent_encode_path(p);
        let url = format!("{}/api/v1/errors/{}", app.api_url, enc);
        let (status, body) = get_path(&app, &session, url).await;
        assert_controlled("errors/:fp", p, status, &body);
    }
}

/// Defensa final: si alguno de los payloads de DROP hubiera ejecutado, una de
/// estas tablas dejaría de existir. Este test corre al final y falla con un
/// mensaje claro si la SQLi pegó.
#[tokio::test]
async fn critical_tables_still_present_after_attack_payloads() {
    let app = TestApp::spawn().await;
    let email = app.create_user("inj-pw").await;
    let session = app.login_session(&email, "inj-pw").await;
    let project = app.project_slug.clone();

    let drop_payload = "x'; DROP TABLE faro.logs; --";
    let triples: &[(&str, &[(&str, &str)])] = &[
        (
            "/api/v1/logs",
            &[("project", "_"), ("service", drop_payload)],
        ),
        (
            "/api/v1/traces",
            &[("project", "_"), ("service", drop_payload)],
        ),
        (
            "/api/v1/metrics/series",
            &[("name", "cpu"), ("service", drop_payload)],
        ),
        (
            "/api/v1/errors",
            &[("project", "_"), ("service", drop_payload)],
        ),
    ];
    for (path, q) in triples {
        // Reemplazamos el placeholder "_" por el proyecto real.
        let qmap: Vec<(&str, &str)> = q
            .iter()
            .map(|(k, v)| (*k, if *v == "_" { project.as_str() } else { *v }))
            .collect();
        let _ = get_q(&app, &session, path, &qmap).await;
    }

    // Verificación directa contra la DB: las tablas críticas siguen ahí.
    for table in [
        "faro.logs",
        "faro.users",
        "faro.user_sessions",
        "faro.projects",
    ] {
        app.ch
            .query_raw(&format!("SELECT 1 FROM {table} LIMIT 0"))
            .await
            .unwrap_or_else(|e| {
                panic!("tabla {table} dejó de ser consultable tras los payloads: {e}")
            });
    }
}

/// Percent-encode los caracteres reservados de path. Usamos `url` (que ya
/// está en deps via reqwest) en lugar de meter un crate nuevo.
fn percent_encode_path(s: &str) -> String {
    // El set RFC 3986 "path" reserva: / ? # [ ] @ ! $ & ' ( ) * + , ; =
    // y excluye los unreserved. Para SQLi payloads queremos escapar todo lo
    // ASCII no-alfanumérico para no toparnos con interpretación raras.
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}
