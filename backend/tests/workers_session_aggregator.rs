mod common;

use chrono::{Duration, Utc};
use common::TestApp;
use faro::storage::ProductEventRow;
use faro::workers::session_aggregator::aggregate_once;
use serde::Deserialize;
use tokio::sync::Mutex;
use uuid::Uuid;

// `aggregate_once()` opera cross-project (su SQL no filtra por `project_id`)
// sobre ventanas de tiempo que se pisan entre tests, así que estos tests NO
// pueden correr en paralelo entre sí. Este Mutex solo los serializa bajo
// `cargo test` (todos los tests del binario comparten proceso); bajo nextest
// cada test corre en su PROPIO proceso y el lock no serializa nada — ahí la
// serialización real la da el test-group `session-aggregator` de
// `.config/nextest.toml`. No borrar el Mutex: sigue siendo necesario para
// `cargo test` puro.
static SESSION_AGGREGATOR_TEST_LOCK: Mutex<()> = Mutex::const_new(());

fn event(
    project_id: &str,
    seconds_ago: i64,
    event_name: &str,
    distinct_id: &str,
    anonymous_id: &str,
    session_id: &str,
) -> ProductEventRow {
    event_with_trace(
        project_id,
        seconds_ago,
        event_name,
        distinct_id,
        anonymous_id,
        session_id,
        "",
    )
}

fn event_with_trace(
    project_id: &str,
    seconds_ago: i64,
    event_name: &str,
    distinct_id: &str,
    anonymous_id: &str,
    session_id: &str,
    trace_id: &str,
) -> ProductEventRow {
    ProductEventRow {
        timestamp: Utc::now() - Duration::seconds(seconds_ago),
        project_id: project_id.to_string(),
        event_name: event_name.to_string(),
        distinct_id: distinct_id.to_string(),
        anonymous_id: anonymous_id.to_string(),
        session_id: session_id.to_string(),
        properties: "{}".into(),
        user_properties: "{}".into(),
        context: "{}".into(),
        source: "web".into(),
        trace_id: trace_id.to_string(),
        span_id: String::new(),
        event_id: Uuid::new_v4().to_string(),
    }
}

#[derive(Debug, Deserialize)]
struct SessionOut {
    session_id: String,
    distinct_id: String,
    page_count: u32,
    duration_seconds: u32,
    event_count: u32,
    pageview_count: u32,
    is_bounce: u8,
    is_engaged: u8,
    converted: u8,
    quality_score: f32,
    trace_ids: Vec<String>,
    trace_count: u32,
}

async fn ensure_session_properties_schema(app: &TestApp) {
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

async fn sessions(app: &TestApp) -> Vec<SessionOut> {
    app.ch
        .select_with_params(
            "SELECT session_id, distinct_id, page_count, duration_seconds, \
                    event_count, pageview_count, is_bounce, is_engaged, converted, quality_score, \
                    trace_ids, trace_count \
             FROM faro.product_sessions FINAL \
             WHERE project_id = {project:String} \
             ORDER BY distinct_id, started_at, session_id",
            &[("project", &app.project_slug)],
        )
        .await
        .expect("select sessions")
}

#[tokio::test]
async fn explicit_session_id_is_trusted() {
    let _guard = SESSION_AGGREGATOR_TEST_LOCK.lock().await;
    let app = TestApp::spawn().await;
    ensure_session_properties_schema(&app).await;
    let rows = vec![
        event_with_trace(
            &app.project_slug,
            3600,
            "page_view",
            "user-1",
            "",
            "sdk-session",
            "trace-sdk-a",
        ),
        event_with_trace(
            &app.project_slug,
            60,
            "checkout",
            "user-1",
            "",
            "sdk-session",
            "trace-sdk-a",
        ),
        event(&app.project_slug, 30, "hover", "user-1", "", "sdk-session"),
    ];
    app.ch
        .insert("faro.product_events", &rows)
        .await
        .expect("insert events");

    let from = Utc::now() - Duration::hours(2);
    let written = aggregate_once(&app.state, from, 30)
        .await
        .expect("aggregate");
    assert!(written >= 1);

    let out = sessions(&app).await;
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].session_id, "sdk-session");
    assert_eq!(out[0].distinct_id, "user-1");
    assert_eq!(out[0].event_count, 3);
    assert_eq!(out[0].pageview_count, 1);
    assert_eq!(out[0].page_count, 1);
    assert_eq!(out[0].is_bounce, 0);
    assert_eq!(out[0].is_engaged, 1);
    assert_eq!(out[0].converted, 0);
    assert!(out[0].duration_seconds >= 3500);
    assert!(out[0].quality_score > 35.0);
    assert!(out[0].quality_score <= 70.0);
    assert_eq!(out[0].trace_ids, vec!["trace-sdk-a"]);
    assert_eq!(out[0].trace_count, 1);
}

#[tokio::test]
async fn synthetic_sessions_split_only_after_gap_exceeds_timeout() {
    let _guard = SESSION_AGGREGATOR_TEST_LOCK.lock().await;
    let app = TestApp::spawn().await;
    ensure_session_properties_schema(&app).await;
    let now = Utc::now();
    let rows = vec![
        ProductEventRow {
            timestamp: now - Duration::minutes(61),
            project_id: app.project_slug.clone(),
            event_name: "page_view".into(),
            distinct_id: "user-gap".into(),
            anonymous_id: String::new(),
            session_id: String::new(),
            properties: "{}".into(),
            user_properties: "{}".into(),
            context: "{}".into(),
            source: "web".into(),
            trace_id: "trace-gap-a".into(),
            span_id: String::new(),
            event_id: Uuid::new_v4().to_string(),
        },
        ProductEventRow {
            timestamp: now - Duration::minutes(31),
            project_id: app.project_slug.clone(),
            event_name: "dashboard_opened".into(),
            distinct_id: "user-gap".into(),
            anonymous_id: String::new(),
            session_id: String::new(),
            properties: "{}".into(),
            user_properties: "{}".into(),
            context: "{}".into(),
            source: "web".into(),
            trace_id: "trace-gap-b".into(),
            span_id: String::new(),
            event_id: Uuid::new_v4().to_string(),
        },
        ProductEventRow {
            timestamp: now,
            project_id: app.project_slug.clone(),
            event_name: "checkout".into(),
            distinct_id: "user-gap".into(),
            anonymous_id: String::new(),
            session_id: String::new(),
            properties: "{}".into(),
            user_properties: "{}".into(),
            context: "{}".into(),
            source: "web".into(),
            trace_id: String::new(),
            span_id: String::new(),
            event_id: Uuid::new_v4().to_string(),
        },
    ];
    app.ch
        .insert("faro.product_events", &rows)
        .await
        .expect("insert events");

    let from = now - Duration::hours(2);
    let written = aggregate_once(&app.state, from, 30)
        .await
        .expect("aggregate");
    assert!(written >= 2);

    let out = sessions(&app).await;
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].distinct_id, "user-gap");
    assert_eq!(out[0].event_count, 2);
    assert_eq!(out[0].pageview_count, 1);
    assert_eq!(out[0].page_count, 1);
    assert_eq!(out[0].is_bounce, 0);
    assert_eq!(out[0].is_engaged, 1);
    assert_eq!(
        sorted(out[0].trace_ids.clone()),
        vec!["trace-gap-a", "trace-gap-b"]
    );
    assert_eq!(out[0].trace_count, 2);
    assert_eq!(out[1].distinct_id, "user-gap");
    assert_eq!(out[1].event_count, 1);
    assert_eq!(out[1].pageview_count, 0);
    assert_eq!(out[1].page_count, 0);
    assert_eq!(out[1].is_bounce, 1);
    assert_eq!(out[1].is_engaged, 0);
    assert!(out[1].trace_ids.is_empty());
    assert_eq!(out[1].trace_count, 0);
}

#[tokio::test]
async fn anonymous_only_events_are_sessionized_and_empty_actor_events_are_ignored() {
    let _guard = SESSION_AGGREGATOR_TEST_LOCK.lock().await;
    let app = TestApp::spawn().await;
    ensure_session_properties_schema(&app).await;
    let rows = vec![
        event(&app.project_slug, 600, "page_view", "", "anon-1", ""),
        event(&app.project_slug, 300, "dashboard_opened", "", "anon-1", ""),
        event(&app.project_slug, 120, "orphan", "", "", ""),
    ];
    app.ch
        .insert("faro.product_events", &rows)
        .await
        .expect("insert events");

    let from = Utc::now() - Duration::hours(1);
    let written = aggregate_once(&app.state, from, 30)
        .await
        .expect("aggregate");
    assert!(written >= 1);

    let out = sessions(&app).await;
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].distinct_id, "anon-1");
    assert_eq!(out[0].event_count, 2);
    assert_eq!(out[0].pageview_count, 1);
    assert_eq!(out[0].page_count, 1);
    assert_eq!(out[0].is_bounce, 0);
    assert_eq!(out[0].is_engaged, 1);
}

#[tokio::test]
async fn conversion_events_lift_session_quality() {
    let _guard = SESSION_AGGREGATOR_TEST_LOCK.lock().await;
    let app = TestApp::spawn().await;
    ensure_session_properties_schema(&app).await;
    let rows = vec![
        event(
            &app.project_slug,
            300,
            "page_view",
            "buyer-1",
            "",
            "buy-session",
        ),
        event(
            &app.project_slug,
            180,
            "pricing_viewed",
            "buyer-1",
            "",
            "buy-session",
        ),
        event(
            &app.project_slug,
            60,
            "checkout_completed",
            "buyer-1",
            "",
            "buy-session",
        ),
    ];
    app.ch
        .insert("faro.product_events", &rows)
        .await
        .expect("insert events");

    let from = Utc::now() - Duration::hours(1);
    let written = aggregate_once(&app.state, from, 30)
        .await
        .expect("aggregate");
    assert!(written >= 1);

    let out = sessions(&app).await;
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].session_id, "buy-session");
    assert_eq!(out[0].event_count, 3);
    assert_eq!(out[0].pageview_count, 1);
    assert_eq!(out[0].converted, 1);
    assert_eq!(out[0].is_bounce, 0);
    assert_eq!(out[0].is_engaged, 1);
    assert!(out[0].quality_score >= 40.0);
    assert!(out[0].quality_score <= 100.0);
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}
