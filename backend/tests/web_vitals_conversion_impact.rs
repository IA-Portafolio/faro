mod common;

use chrono::{DateTime, Duration, TimeZone, Utc};
use common::TestApp;
use faro::storage::{AttrMap, LogRow, ProductEventRow};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct WebVitalsConversionImpactResult {
    metric: String,
    threshold_ms: f64,
    pageview_event: String,
    conversion_event: String,
    service_name: String,
    slow_sessions: u64,
    baseline_sessions: u64,
    slow_users: u64,
    baseline_users: u64,
    slow_pageviews: u64,
    baseline_pageviews: u64,
    slow_conversions: u64,
    baseline_conversions: u64,
    slow_conversion_rate: f64,
    baseline_conversion_rate: f64,
    conversion_drop_points: f64,
    summary: String,
}

fn product_event(
    ts: DateTime<Utc>,
    project: &str,
    distinct_id: &str,
    session_id: &str,
    event_name: &str,
) -> ProductEventRow {
    ProductEventRow {
        timestamp: ts,
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

fn web_vital_log(ts: DateTime<Utc>, project: &str, session_id: &str, lcp_ms: f64) -> LogRow {
    let mut attributes = AttrMap::new();
    attributes.insert("session.id".into(), session_id.into());
    attributes.insert("metric.name".into(), "LCP".into());
    attributes.insert("metric.value".into(), lcp_ms.to_string());
    attributes.insert("metric.rating".into(), "needs-improvement".into());

    LogRow {
        timestamp: ts,
        observed_timestamp: ts,
        project_id: project.into(),
        service_name: "web".into(),
        severity_text: "INFO".into(),
        severity_number: LogRow::severity_from_text("INFO"),
        body: "web-vital LCP".into(),
        trace_id: String::new(),
        span_id: String::new(),
        scope_name: "faro.nextjs".into(),
        resource_attributes: AttrMap::new(),
        attributes,
    }
}

fn add_session(
    events: &mut Vec<ProductEventRow>,
    logs: &mut Vec<LogRow>,
    base: DateTime<Utc>,
    project: &str,
    index: u32,
    lcp_ms: f64,
    converted: bool,
) {
    let distinct_id = format!("u-{index}");
    let session_id = format!("s-{index}");
    let ts = base + Duration::minutes(index.into());

    events.push(product_event(
        ts,
        project,
        &distinct_id,
        &session_id,
        "$pageview",
    ));
    if converted {
        events.push(product_event(
            ts + Duration::minutes(1),
            project,
            &distinct_id,
            &session_id,
            "checkout_completed",
        ));
    }
    logs.push(web_vital_log(
        ts + Duration::seconds(5),
        project,
        &session_id,
        lcp_ms,
    ));
}

async fn seed(app: &TestApp, from: DateTime<Utc>) {
    let project = &app.project_slug;
    let mut events = Vec::new();
    let mut logs = Vec::new();

    for i in 0..5 {
        add_session(&mut events, &mut logs, from, project, i, 2_500.0, i < 4);
    }
    for i in 5..10 {
        add_session(&mut events, &mut logs, from, project, i, 4_500.0, i < 7);
    }

    app.ch
        .insert("faro.product_events", &events)
        .await
        .expect("insert product events");
    app.ch
        .insert("faro.logs", &logs)
        .await
        .expect("insert logs");
}

async fn query(
    app: &TestApp,
    session: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> WebVitalsConversionImpactResult {
    let url = format!(
        "{}/api/v1/insights/web-vitals-conversion-impact",
        app.api_url
    );
    let from_s = from.to_rfc3339();
    let to_s = to.to_rfc3339();
    let resp = app
        .http
        .get(&url)
        .query(&[
            ("project", app.project_slug.as_str()),
            ("from", from_s.as_str()),
            ("to", to_s.as_str()),
            ("metric", "lcp"),
            ("threshold_ms", "4000"),
            ("conversion_event", "checkout_completed"),
            ("service", "web"),
        ])
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send");

    assert!(
        resp.status().is_success(),
        "GET /insights/web-vitals-conversion-impact failed ({}): {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );

    resp.json().await.expect("decode json")
}

#[tokio::test]
async fn web_vitals_conversion_impact_reports_lcp_conversion_drop() {
    let app = TestApp::spawn().await;
    let email = app.create_user("hunter2-test").await;
    let session = app.login_session(&email, "hunter2-test").await;
    let from = Utc.with_ymd_and_hms(2026, 5, 24, 10, 0, 0).unwrap();
    let to = from + Duration::hours(1);
    seed(&app, from).await;

    let result = query(&app, &session, from, to).await;

    assert_eq!(result.metric, "LCP");
    assert_eq!(result.threshold_ms, 4_000.0);
    assert_eq!(result.pageview_event, "$pageview");
    assert_eq!(result.conversion_event, "checkout_completed");
    assert_eq!(result.service_name, "web");
    assert_eq!(result.baseline_sessions, 5);
    assert_eq!(result.slow_sessions, 5);
    assert_eq!(result.baseline_users, 5);
    assert_eq!(result.slow_users, 5);
    assert_eq!(result.baseline_pageviews, 5);
    assert_eq!(result.slow_pageviews, 5);
    assert_eq!(result.baseline_conversions, 4);
    assert_eq!(result.slow_conversions, 2);
    assert!((result.baseline_conversion_rate - 0.8).abs() < 0.0001);
    assert!((result.slow_conversion_rate - 0.4).abs() < 0.0001);
    assert!((result.conversion_drop_points - 40.0).abs() < 0.0001);
    assert_eq!(
        result.summary,
        "Los usuarios con LCP > 4s convierten 40 puntos menos."
    );
}

#[tokio::test]
async fn web_vitals_conversion_impact_requires_session() {
    let app = TestApp::spawn().await;

    let resp = app
        .http
        .get(format!(
            "{}/api/v1/insights/web-vitals-conversion-impact",
            app.api_url
        ))
        .send()
        .await
        .expect("send");

    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
