//! Integration tests del worker `monitor_runner`. Levanta un servidor HTTP
//! efímero en `127.0.0.1:0` que sirve rutas controladas, invoca `run_check`
//! contra él, y verifica que la fila resultado encolada en
//! `state.ingest.monitor_results_tx` refleje el comportamiento esperado.
//!
//! Cubre:
//!   * 2xx dentro del rango esperado → success=1.
//!   * status fuera del rango → success=0 + mensaje describiendo el código.
//!   * regex de body que matchea → success=1.
//!   * regex de body que NO matchea → success=0 + mensaje "body did not match".
//!   * dispatch por método HTTP (GET, POST, PUT) — el verbo viaja al servidor.
//!   * headers custom — viajan al servidor y son visibles en el handler.
//!   * red caída / DNS roto — success=0, status_code=0, error_message poblado.
//!   * rango custom (e.g. expected_status_min=500) — un 500 se considera éxito.
//!
//! Aislamiento: cada test arranca su propio servidor en un puerto efímero, así
//! corren en paralelo sin chocarse y sin necesidad de ClickHouse.

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use axum::extract::State as AxumState;
use axum::http::{HeaderMap as AxumHeaders, StatusCode};
use axum::routing::{any, get};
use axum::Router;
use chrono::Utc;
use common::test_config;
use faro::state::AppState;
use faro::storage::{AttrMap, Client, MonitorResultRow, MonitorRow};
use faro::workers::monitor_runner::run_check;
use tokio::net::TcpListener;
use uuid::Uuid;

/// State mínimo: client CH real (run_check no lo toca pero AppState lo exige),
/// y el resto de inicialización por defecto. Lo importante es que
/// `state.ingest.monitor_results_tx` esté wired al receiver dentro del Mutex.
async fn minimal_state() -> Arc<AppState> {
    let cfg = test_config();
    let ch = Client::new(&cfg).await.expect("CH client");
    ch.wait_until_ready().await.expect("CH ready");
    Arc::new(AppState::new(cfg, ch))
}

/// Recibe la próxima fila empujada por `run_check` o falla con timeout. Toma el
/// rx del Mutex inline — solo un test consume cada AppState.
async fn next_result(state: &Arc<AppState>) -> MonitorResultRow {
    let mut rx = state
        .ingest
        .monitor_results_rx
        .lock()
        .take()
        .expect("rx no disponible (¿otra cosa lo tomó?)");
    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout esperando fila de monitor_result")
        .expect("canal cerrado sin entregar fila")
}

fn monitor(id: Uuid, url: &str) -> MonitorRow {
    let now = Utc::now();
    MonitorRow {
        id,
        project_id: "test".into(),
        name: "probe".into(),
        method: "GET".into(),
        url: url.into(),
        headers: AttrMap::new(),
        body: String::new(),
        interval_seconds: 60,
        timeout_seconds: 5,
        expected_status_min: 200,
        expected_status_max: 299,
        expected_body_regex: String::new(),
        enabled: 1,
        created_at: now,
        updated_at: now,
        deleted: 0,
        version: 1,
    }
}

/// Estado compartido del server de prueba — guarda los requests recibidos para
/// que los tests puedan inspeccionar método + headers que llegaron al server.
#[derive(Clone, Default)]
struct ProbeSrv {
    seen: Arc<StdMutex<Vec<SeenReq>>>,
}

#[derive(Clone, Debug)]
struct SeenReq {
    method: String,
    headers: HashMap<String, String>,
    body: String,
}

/// Levanta un server local con rutas:
///   * GET  /ok                → 200 "alive and well"
///   * GET  /server-error      → 500 "kaboom"
///   * GET  /banner            → 200 "version=42; status=READY" (para regex)
///   * ANY  /echo              → 200 + body "<METHOD>:<x-test-header>" (para
///                                verificar método + headers que llegaron)
async fn spawn_probe_server() -> (String, ProbeSrv) {
    let srv = ProbeSrv::default();
    let app = Router::new()
        .route("/ok", get(|| async { (StatusCode::OK, "alive and well") }))
        .route(
            "/server-error",
            get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "kaboom") }),
        )
        .route(
            "/banner",
            get(|| async { (StatusCode::OK, "version=42; status=READY") }),
        )
        .route(
            "/echo",
            any(
                |AxumState(s): AxumState<ProbeSrv>,
                 method: axum::http::Method,
                 headers: AxumHeaders,
                 body: String| async move {
                    let hdrs: HashMap<String, String> = headers
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect();
                    let xtest = hdrs.get("x-test").cloned().unwrap_or_default();
                    let resp = format!("{}:{}", method.as_str(), xtest);
                    s.seen.lock().unwrap().push(SeenReq {
                        method: method.to_string(),
                        headers: hdrs,
                        body,
                    });
                    (StatusCode::OK, resp)
                },
            ),
        )
        .with_state(srv.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind probe server");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), srv)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest")
}

#[tokio::test]
async fn success_when_status_in_range() {
    let state = minimal_state().await;
    let (base, _srv) = spawn_probe_server().await;
    let id = Uuid::new_v4();
    let m = monitor(id, &format!("{base}/ok"));

    run_check(client(), m, state.clone()).await;
    let r = next_result(&state).await;

    assert_eq!(r.monitor_id, id);
    assert_eq!(r.success, 1);
    assert_eq!(r.status_code, 200);
    assert!(
        r.error_message.is_empty(),
        "error_message debe ser vacío en success"
    );
    assert!(r.response_size > 0, "response_size debe reflejar el body");
}

#[tokio::test]
async fn failure_when_status_outside_range() {
    let state = minimal_state().await;
    let (base, _srv) = spawn_probe_server().await;
    let id = Uuid::new_v4();
    let m = monitor(id, &format!("{base}/server-error"));

    run_check(client(), m, state.clone()).await;
    let r = next_result(&state).await;

    assert_eq!(r.success, 0);
    assert_eq!(r.status_code, 500);
    assert!(
        r.error_message.contains("500") && r.error_message.contains("200-299"),
        "error_message debería mencionar el código y el rango esperado, fue: {}",
        r.error_message
    );
}

#[tokio::test]
async fn custom_status_range_treats_500_as_success() {
    let state = minimal_state().await;
    let (base, _srv) = spawn_probe_server().await;
    let id = Uuid::new_v4();
    let mut m = monitor(id, &format!("{base}/server-error"));
    // Rango "raro" custom — útil para monitorear endpoints que devuelven 5xx
    // por design (probes negativas, healthchecks invertidos).
    m.expected_status_min = 500;
    m.expected_status_max = 599;

    run_check(client(), m, state.clone()).await;
    let r = next_result(&state).await;
    assert_eq!(r.success, 1, "500 dentro de [500,599] debe ser success");
    assert!(r.error_message.is_empty());
}

#[tokio::test]
async fn regex_match_passes_and_no_match_fails() {
    // match
    {
        let state = minimal_state().await;
        let (base, _srv) = spawn_probe_server().await;
        let id = Uuid::new_v4();
        let mut m = monitor(id, &format!("{base}/banner"));
        m.expected_body_regex = r"status=READY".into();
        run_check(client(), m, state.clone()).await;
        let r = next_result(&state).await;
        assert_eq!(r.success, 1, "regex matcheante → success");
    }

    // no-match
    {
        let state = minimal_state().await;
        let (base, _srv) = spawn_probe_server().await;
        let id = Uuid::new_v4();
        let mut m = monitor(id, &format!("{base}/banner"));
        m.expected_body_regex = r"status=DOWN".into();
        run_check(client(), m, state.clone()).await;
        let r = next_result(&state).await;
        assert_eq!(r.success, 0, "regex no-matcheante → failure");
        assert!(
            r.error_message.contains("body did not match"),
            "error_message debe explicar el motivo, fue: {}",
            r.error_message
        );
    }
}

#[tokio::test]
async fn method_dispatch_and_custom_headers_reach_the_server() {
    let state = minimal_state().await;
    let (base, srv) = spawn_probe_server().await;
    let id = Uuid::new_v4();
    let mut m = monitor(id, &format!("{base}/echo"));
    m.method = "POST".into();
    m.body = "payload".into();
    m.headers.insert("X-Test".into(), "hello".into());

    run_check(client(), m, state.clone()).await;
    let r = next_result(&state).await;
    assert_eq!(r.success, 1);

    let seen = srv.seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "se esperaba 1 request");
    let req = &seen[0];
    assert_eq!(req.method, "POST", "el método debe propagarse");
    assert_eq!(
        req.headers.get("x-test").map(String::as_str),
        Some("hello"),
        "el header custom debe llegar al server"
    );
    assert_eq!(req.body, "payload", "el body custom debe llegar al server");
}

#[tokio::test]
async fn network_error_is_recorded_as_failure() {
    let state = minimal_state().await;
    let id = Uuid::new_v4();
    // Puerto improbable de tener algo escuchando — Conn refused inmediato.
    // No usamos un dominio inexistente (DNS) porque puede demorar 10+ segundos
    // en entornos con DNS lento, inflando el test.
    let m = monitor(id, "http://127.0.0.1:1/never-up");

    run_check(client(), m, state.clone()).await;
    let r = next_result(&state).await;

    assert_eq!(r.success, 0);
    assert_eq!(r.status_code, 0, "sin response → status_code=0");
    assert!(
        !r.error_message.is_empty(),
        "error_message debe explicar el fallo de red"
    );
    assert_eq!(r.response_size, 0);
}
