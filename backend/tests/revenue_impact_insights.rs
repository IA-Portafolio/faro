mod common;

use chrono::{Duration, Utc};
use common::TestApp;
use faro::storage::{AttrMap, ErrorEventRow, ProductEventRow};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct RevenueImpactIssue {
    fingerprint: String,
    service_name: String,
    exception_type: String,
    message: String,
    affected_sessions: u64,
    sessions_without_checkout: u64,
    issue_conversion_rate: f64,
    baseline_conversion_rate: f64,
    conversion_gap: f64,
    estimated_lost_revenue: f64,
}

fn product_event(
    secs_ago: i64,
    project: &str,
    session_id: &str,
    distinct_id: &str,
    event_name: &str,
) -> ProductEventRow {
    ProductEventRow {
        timestamp: Utc::now() - Duration::seconds(secs_ago),
        project_id: project.into(),
        event_name: event_name.into(),
        distinct_id: distinct_id.into(),
        anonymous_id: String::new(),
        session_id: session_id.into(),
        properties: String::new(),
        user_properties: String::new(),
        context: String::new(),
        source: "web".into(),
        trace_id: String::new(),
        span_id: String::new(),
        event_id: Uuid::new_v4().to_string(),
    }
}

fn error_event(
    secs_ago: i64,
    project: &str,
    session_id: &str,
    fingerprint: &str,
    service_name: &str,
    message: &str,
) -> ErrorEventRow {
    let mut attributes = AttrMap::new();
    attributes.insert("session_id".into(), session_id.into());

    ErrorEventRow {
        timestamp: Utc::now() - Duration::seconds(secs_ago),
        project_id: project.into(),
        fingerprint: fingerprint.into(),
        service_name: service_name.into(),
        severity_text: "ERROR".into(),
        message: message.into(),
        exception_type: "TypeError".into(),
        exception_message: message.into(),
        stack_trace: String::new(),
        trace_id: String::new(),
        span_id: String::new(),
        attributes,
    }
}

async fn seed(app: &TestApp) {
    let p = &app.project_slug;
    let product_events = vec![
        product_event(600, p, "s-1", "u-1", "checkout_started"),
        product_event(590, p, "s-1", "u-1", "checkout_completed"),
        product_event(580, p, "s-2", "u-2", "checkout_started"),
        product_event(570, p, "s-2", "u-2", "checkout_completed"),
        product_event(560, p, "s-3", "u-3", "checkout_started"),
        product_event(550, p, "s-3", "u-3", "checkout_completed"),
        product_event(540, p, "s-4", "u-4", "checkout_started"),
        product_event(530, p, "s-4", "u-4", "checkout_completed"),
        product_event(520, p, "s-5", "u-5", "checkout_started"),
        product_event(510, p, "s-5", "u-5", "checkout_completed"),
        product_event(500, p, "s-6", "u-6", "checkout_started"),
        product_event(490, p, "s-6", "u-6", "checkout_completed"),
        product_event(480, p, "s-7", "u-7", "checkout_started"),
        product_event(470, p, "s-7", "u-7", "checkout_completed"),
        product_event(460, p, "s-8", "u-8", "checkout_started"),
        product_event(450, p, "s-8", "u-8", "checkout_completed"),
        product_event(440, p, "s-9", "u-9", "checkout_started"),
        product_event(430, p, "s-10", "u-10", "checkout_started"),
    ];
    app.ch
        .insert("faro.product_events", &product_events)
        .await
        .expect("insert product events");

    let errors = vec![
        error_event(
            425,
            p,
            "s-9",
            "fp-payment",
            "checkout-api",
            "payment provider failed",
        ),
        error_event(
            415,
            p,
            "s-10",
            "fp-payment",
            "checkout-api",
            "payment provider failed",
        ),
        error_event(595, p, "s-1", "fp-ui", "web", "button label missing"),
    ];
    app.ch
        .insert("faro.error_events", &errors)
        .await
        .expect("insert error events");
}

async fn query(app: &TestApp, session: &str) -> Vec<RevenueImpactIssue> {
    let url = format!(
        "{}/api/v1/insights/revenue-impact?project={}&last_minutes=120&average_order_value=100&limit=10",
        app.api_url, app.project_slug
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
        "GET /insights/revenue-impact failed ({}): {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );

    resp.json().await.expect("decode json")
}

#[tokio::test]
async fn revenue_impact_prioritizes_errors_by_checkout_loss() {
    let app = TestApp::spawn().await;
    let email = app.create_user("hunter2-test").await;
    let session = app.login_session(&email, "hunter2-test").await;
    seed(&app).await;

    let rows = query(&app, &session).await;

    assert_eq!(rows.len(), 2);

    let top = &rows[0];
    assert_eq!(top.fingerprint, "fp-payment");
    assert_eq!(top.service_name, "checkout-api");
    assert_eq!(top.exception_type, "TypeError");
    assert_eq!(top.message, "payment provider failed");
    assert_eq!(top.affected_sessions, 2);
    assert_eq!(top.sessions_without_checkout, 2);
    assert!((top.issue_conversion_rate - 0.0).abs() < 0.0001);
    assert!((top.baseline_conversion_rate - 0.8).abs() < 0.0001);
    assert!((top.conversion_gap - 0.8).abs() < 0.0001);
    assert!((top.estimated_lost_revenue - 160.0).abs() < 0.0001);

    let second = &rows[1];
    assert_eq!(second.fingerprint, "fp-ui");
    assert_eq!(second.affected_sessions, 1);
    assert_eq!(second.sessions_without_checkout, 0);
    assert_eq!(second.estimated_lost_revenue, 0.0);
}

#[tokio::test]
async fn revenue_impact_requires_session() {
    let app = TestApp::spawn().await;

    let resp = app
        .http
        .get(format!("{}/api/v1/insights/revenue-impact", app.api_url))
        .send()
        .await
        .expect("send");

    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
