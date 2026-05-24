//! `GET /api/v1/sessions/:session_id/traces`: una sesión materializa los
//! trace_id servidos durante esa navegación; el endpoint resuelve esos ids
//! contra `faro.spans` para mostrar qué traces backend estuvieron involucrados.

mod common;

use chrono::{Duration, Utc};
use common::TestApp;
use faro::api::traces::TraceSummary;
use faro::storage::{AttrMap, ProductSessionRow, SpanRow};

fn session(app: &TestApp, session_id: &str, trace_ids: Vec<String>) -> ProductSessionRow {
    let now = Utc::now();
    ProductSessionRow {
        project_id: app.project_slug.clone(),
        session_id: session_id.into(),
        distinct_id: "user-1".into(),
        started_at: now - Duration::minutes(5),
        ended_at: now,
        page_count: 2,
        duration_seconds: 300,
        event_count: 3,
        pageview_count: 2,
        is_bounce: 0,
        is_engaged: 1,
        converted: 0,
        quality_score: 0.55,
        trace_count: trace_ids.len() as u32,
        trace_ids,
        source: "sdk".into(),
    }
}

fn span(app: &TestApp, trace_id: &str, span_id: &str, name: &str, status: &str) -> SpanRow {
    SpanRow {
        timestamp: Utc::now(),
        project_id: app.project_slug.clone(),
        trace_id: trace_id.into(),
        span_id: span_id.into(),
        parent_span_id: String::new(),
        trace_state: String::new(),
        name: name.into(),
        kind: "SERVER".into(),
        service_name: "api".into(),
        duration_ns: 25_000_000,
        status_code: status.into(),
        status_message: String::new(),
        resource_attributes: AttrMap::new(),
        span_attributes: AttrMap::new(),
        events_timestamps: Vec::new(),
        events_names: Vec::new(),
        events_attributes: Vec::new(),
        links_trace_ids: Vec::new(),
        links_span_ids: Vec::new(),
    }
}

async fn auth_session(app: &TestApp) -> String {
    let email = app.create_user("hunter2-test").await;
    app.login_session(&email, "hunter2-test").await
}

async fn ensure_session_trace_schema(app: &TestApp) {
    let ddl = [
        "ALTER TABLE faro.product_sessions ADD COLUMN IF NOT EXISTS event_count UInt32 DEFAULT 0 CODEC(T64, ZSTD(1))",
        "ALTER TABLE faro.product_sessions ADD COLUMN IF NOT EXISTS pageview_count UInt32 DEFAULT 0 CODEC(T64, ZSTD(1))",
        "ALTER TABLE faro.product_sessions ADD COLUMN IF NOT EXISTS is_bounce UInt8 DEFAULT 0",
        "ALTER TABLE faro.product_sessions ADD COLUMN IF NOT EXISTS is_engaged UInt8 DEFAULT 0",
        "ALTER TABLE faro.product_sessions ADD COLUMN IF NOT EXISTS converted UInt8 DEFAULT 0",
        "ALTER TABLE faro.product_sessions ADD COLUMN IF NOT EXISTS quality_score Float32 DEFAULT 0 CODEC(ZSTD(1))",
        "ALTER TABLE faro.product_sessions ADD COLUMN IF NOT EXISTS trace_ids Array(String) DEFAULT [] CODEC(ZSTD(1))",
        "ALTER TABLE faro.product_sessions ADD COLUMN IF NOT EXISTS trace_count UInt32 DEFAULT 0 CODEC(T64, ZSTD(1))",
    ];
    for sql in ddl {
        app.ch.query_raw(sql).await.expect("alter product_sessions");
    }
}

async fn query_traces(app: &TestApp, cookie: &str, session_id: &str) -> reqwest::Response {
    app.http
        .get(format!(
            "{}/api/v1/sessions/{}/traces?project={}",
            app.api_url, session_id, app.project_slug
        ))
        .header(reqwest::header::COOKIE, format!("faro_session={cookie}"))
        .send()
        .await
        .expect("send")
}

#[tokio::test]
async fn session_traces_resolves_materialized_trace_ids_to_summaries() {
    let app = TestApp::spawn().await;
    ensure_session_trace_schema(&app).await;
    let cookie = auth_session(&app).await;

    app.ch
        .insert(
            "faro.product_sessions",
            &[session(
                &app,
                "sess-traced",
                vec!["trace-ok".into(), "trace-error".into()],
            )],
        )
        .await
        .expect("insert session");
    app.ch
        .insert(
            "faro.spans",
            &[
                span(&app, "trace-ok", "span-ok", "GET /cart", "OK"),
                span(&app, "trace-error", "span-error", "POST /checkout", "ERROR"),
                span(&app, "trace-other", "span-other", "GET /ignored", "OK"),
            ],
        )
        .await
        .expect("insert spans");

    let resp = query_traces(&app, &cookie, "sess-traced").await;
    assert!(
        resp.status().is_success(),
        "GET /sessions/:id/traces failed ({}): {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
    let rows: Vec<TraceSummary> = resp.json().await.expect("decode json");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|r| {
        r.trace_id == "trace-ok" && r.root_name == "GET /cart" && r.status_code == "OK"
    }));
    assert!(rows.iter().any(|r| {
        r.trace_id == "trace-error" && r.root_name == "POST /checkout" && r.status_code == "ERROR"
    }));
    assert!(rows.iter().all(|r| r.service_name == "api"));
}

#[tokio::test]
async fn session_traces_returns_empty_for_session_without_traces() {
    let app = TestApp::spawn().await;
    ensure_session_trace_schema(&app).await;
    let cookie = auth_session(&app).await;

    app.ch
        .insert(
            "faro.product_sessions",
            &[session(&app, "sess-empty", Vec::new())],
        )
        .await
        .expect("insert session");

    let resp = query_traces(&app, &cookie, "sess-empty").await;
    assert!(resp.status().is_success());
    let rows: Vec<TraceSummary> = resp.json().await.expect("decode json");
    assert!(rows.is_empty());
}

#[tokio::test]
async fn session_traces_returns_404_for_unknown_session() {
    let app = TestApp::spawn().await;
    ensure_session_trace_schema(&app).await;
    let cookie = auth_session(&app).await;

    let resp = query_traces(&app, &cookie, "missing-session").await;
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}
