//! `GET /api/v1/logs`: inserta N filas directo en `faro.logs` (saltando el path
//! de ingesta para no pelearnos con el flush), luego verifica filtrado por
//! service, min_severity, full-text y trace_id; ordering DESC por timestamp;
//! aislamiento por project_id; y bypass de auth (rechaza sin cookie).

mod common;

use chrono::{Duration, Utc};
use common::TestApp;
use faro::storage::{AttrMap, LogRow};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ApiLog {
    body: String,
    service_name: String,
    severity_number: u8,
    severity_text: String,
    trace_id: String,
    project_id: String,
}

fn log(secs_ago: i64, svc: &str, sev: &str, body: &str, trace: &str, project: &str) -> LogRow {
    let now = Utc::now();
    let ts = now - Duration::seconds(secs_ago);
    LogRow {
        timestamp: ts,
        observed_timestamp: ts,
        project_id: project.into(),
        service_name: svc.into(),
        severity_text: sev.into(),
        severity_number: LogRow::severity_from_text(sev),
        body: body.into(),
        trace_id: trace.into(),
        span_id: String::new(),
        scope_name: String::new(),
        resource_attributes: AttrMap::new(),
        attributes: AttrMap::new(),
    }
}

async fn seed_logs(app: &TestApp) {
    let p = &app.project_slug;
    let rows = vec![
        log(50, "api", "INFO", "hello world", "trace-aaa", p),
        log(40, "api", "ERROR", "boom: db unreachable", "trace-bbb", p),
        log(30, "api", "WARN", "slow query 1.2s", "trace-bbb", p),
        log(20, "worker", "INFO", "processed job 42", "trace-ccc", p),
        log(10, "worker", "ERROR", "panic in handler", "trace-ddd", p),
    ];
    app.ch
        .insert("faro.logs", &rows)
        .await
        .expect("insert logs");
}

async fn query(app: &TestApp, session: &str, qs: &str) -> Vec<ApiLog> {
    let url = format!(
        "{}/api/v1/logs?project={}&{}",
        app.api_url, app.project_slug, qs
    );
    let resp = app
        .http
        .get(&url)
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send");
    assert!(
        resp.status().is_success(),
        "GET /logs failed ({}): {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
    resp.json().await.expect("decode json")
}

#[tokio::test]
async fn list_logs_filters_by_service_severity_query_and_trace() {
    let app = TestApp::spawn().await;
    let email = app.create_user("hunter2-test").await;
    let session = app.login_session(&email, "hunter2-test").await;
    seed_logs(&app).await;

    // Sin filtro: cinco logs del proyecto, DESC.
    let all = query(&app, &session, "").await;
    assert_eq!(all.len(), 5, "deberían ser las 5 filas semilla");
    for r in &all {
        assert_eq!(r.project_id, app.project_slug, "aislamiento por proyecto");
    }
    // Orden DESC por timestamp → el más reciente (panic, 10s ago) primero.
    assert_eq!(all[0].body, "panic in handler");
    assert_eq!(all.last().unwrap().body, "hello world");

    // service=worker → 2 filas.
    let only_worker = query(&app, &session, "service=worker").await;
    assert_eq!(only_worker.len(), 2);
    assert!(only_worker.iter().all(|r| r.service_name == "worker"));

    // min_severity=17 (ERROR) → 2 filas (boom + panic).
    let errors = query(&app, &session, "min_severity=17").await;
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|r| r.severity_number >= 17));
    assert!(errors.iter().any(|r| r.severity_text == "ERROR"));

    // full-text case-insensitive: "PANIC" → "panic in handler".
    let by_text = query(&app, &session, "query=PANIC").await;
    assert_eq!(by_text.len(), 1);
    assert_eq!(by_text[0].body, "panic in handler");

    // trace_id=trace-bbb → 2 filas (boom + slow).
    let by_trace = query(&app, &session, "trace_id=trace-bbb").await;
    assert_eq!(by_trace.len(), 2);
    assert!(by_trace.iter().all(|r| r.trace_id == "trace-bbb"));

    // Combinación: service=api + min_severity=13 (WARN+) → boom + slow.
    let combo = query(&app, &session, "service=api&min_severity=13").await;
    assert_eq!(combo.len(), 2);
    assert!(combo
        .iter()
        .all(|r| r.service_name == "api" && r.severity_number >= 13));
}

#[tokio::test]
async fn list_logs_requires_session() {
    let app = TestApp::spawn().await;
    let resp = app
        .http
        .get(format!("{}/api/v1/logs", app.api_url))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_logs_respects_limit() {
    let app = TestApp::spawn().await;
    let email = app.create_user("hunter2-test").await;
    let session = app.login_session(&email, "hunter2-test").await;
    seed_logs(&app).await;

    let limited = query(&app, &session, "limit=2").await;
    assert_eq!(limited.len(), 2);
    // los dos más recientes — panic primero, processed segundo.
    assert_eq!(limited[0].body, "panic in handler");
    assert_eq!(limited[1].body, "processed job 42");
}
