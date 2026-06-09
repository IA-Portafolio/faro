mod common;

use chrono::{DateTime, Duration, Timelike, Utc};
use common::TestApp;
use faro::storage::{AttrMap, ProductEventRow, SpanRow};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct LatencyFunnelImpactResult {
    span_name: String,
    service_name: String,
    funnel_from: String,
    funnel_to: String,
    bucket_minutes: u32,
    p95_threshold_ms: u32,
    slow_bucket_count: u32,
    baseline_bucket_count: u32,
    baseline_conversion_rate: f64,
    slow_conversion_rate: f64,
    conversion_drop_points: f64,
    summary: String,
    buckets: Vec<LatencyFunnelBucket>,
}

#[derive(Debug, Deserialize)]
struct LatencyFunnelBucket {
    bucket_start: String,
    p95_latency_ms: f64,
    funnel_started: u64,
    funnel_completed: u64,
    conversion_rate: f64,
    slow: bool,
}

fn span_at(ts: DateTime<Utc>, project: &str, duration_ms: u64) -> SpanRow {
    SpanRow {
        timestamp: ts,
        project_id: project.into(),
        trace_id: Uuid::new_v4().simple().to_string(),
        span_id: Uuid::new_v4().simple().to_string(),
        parent_span_id: String::new(),
        trace_state: String::new(),
        name: "/api/checkout".into(),
        kind: "SERVER".into(),
        service_name: "checkout-api".into(),
        duration_ns: duration_ms * 1_000_000,
        status_code: "OK".into(),
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

fn add_funnel_events(
    rows: &mut Vec<ProductEventRow>,
    bucket: DateTime<Utc>,
    project: &str,
    prefix: &str,
    started: u32,
    completed: u32,
) {
    for i in 0..started {
        let distinct_id = format!("{prefix}-u-{i}");
        let session_id = format!("{prefix}-s-{i}");
        rows.push(product_event(
            bucket + Duration::minutes(5),
            project,
            &distinct_id,
            &session_id,
            "checkout_started",
        ));
        if i < completed {
            rows.push(product_event(
                bucket + Duration::minutes(10),
                project,
                &distinct_id,
                &session_id,
                "checkout_completed",
            ));
        }
    }
}

async fn seed(app: &TestApp, from: DateTime<Utc>) {
    let project = &app.project_slug;
    let buckets = [
        from,
        from + Duration::hours(1),
        from + Duration::hours(2),
        from + Duration::hours(3),
    ];

    let spans = vec![
        span_at(buckets[0] + Duration::minutes(1), project, 1_100),
        span_at(buckets[0] + Duration::minutes(2), project, 1_200),
        span_at(buckets[1] + Duration::minutes(1), project, 1_300),
        span_at(buckets[1] + Duration::minutes(2), project, 1_400),
        span_at(buckets[2] + Duration::minutes(1), project, 2_300),
        span_at(buckets[2] + Duration::minutes(2), project, 2_500),
        span_at(buckets[3] + Duration::minutes(1), project, 2_400),
        span_at(buckets[3] + Duration::minutes(2), project, 2_600),
    ];
    app.ch
        .insert("faro.spans", &spans)
        .await
        .expect("insert spans");

    let mut events = Vec::new();
    add_funnel_events(&mut events, buckets[0], project, "b0", 10, 8);
    add_funnel_events(&mut events, buckets[1], project, "b1", 10, 8);
    add_funnel_events(&mut events, buckets[2], project, "b2", 10, 5);
    add_funnel_events(&mut events, buckets[3], project, "b3", 10, 5);
    app.ch
        .insert("faro.product_events", &events)
        .await
        .expect("insert product events");
}

async fn query(
    app: &TestApp,
    session: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> LatencyFunnelImpactResult {
    let url = format!("{}/api/v1/insights/latency-funnel-impact", app.api_url);
    let from_s = from.to_rfc3339();
    let to_s = to.to_rfc3339();
    let resp = app
        .http
        .get(&url)
        .query(&[
            ("project", app.project_slug.as_str()),
            ("from", from_s.as_str()),
            ("to", to_s.as_str()),
            ("span_name", "/api/checkout"),
            ("service", "checkout-api"),
            ("latency_threshold_ms", "2000"),
            ("bucket_minutes", "60"),
        ])
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send");

    assert!(
        resp.status().is_success(),
        "GET /insights/latency-funnel-impact failed ({}): {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );

    resp.json().await.expect("decode json")
}

#[tokio::test]
async fn latency_funnel_impact_reports_conversion_drop_when_p95_is_slow() {
    let app = TestApp::spawn().await;
    let email = app.create_user("hunter2-test").await;
    let session = app.login_session(&email, "hunter2-test").await;
    // Base reciente y alineada a la hora (NO una fecha fija): faro.spans tiene
    // TTL de 14 días, así que una fecha hardcodeada termina cayendo fuera del
    // TTL con el paso del tiempo y ClickHouse borra las filas en un merge →
    // test flaky. 6h atrás mantiene la ventana [from, from+4h] dentro del TTL.
    let from = (Utc::now() - Duration::hours(6))
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap();
    let to = from + Duration::hours(4);
    seed(&app, from).await;

    let result = query(&app, &session, from, to).await;

    assert_eq!(result.span_name, "/api/checkout");
    assert_eq!(result.service_name, "checkout-api");
    assert_eq!(result.funnel_from, "checkout_started");
    assert_eq!(result.funnel_to, "checkout_completed");
    assert_eq!(result.bucket_minutes, 60);
    assert_eq!(result.p95_threshold_ms, 2_000);
    assert_eq!(result.slow_bucket_count, 2);
    assert_eq!(result.baseline_bucket_count, 2);
    assert!((result.baseline_conversion_rate - 0.8).abs() < 0.0001);
    assert!((result.slow_conversion_rate - 0.5).abs() < 0.0001);
    assert!((result.conversion_drop_points - 30.0).abs() < 0.0001);
    assert_eq!(
        result.summary,
        "Cuando /api/checkout p95 supera 2s, el funnel checkout cae 30 puntos."
    );
    assert_eq!(result.buckets.len(), 4);
    assert_eq!(result.buckets.iter().filter(|b| b.slow).count(), 2);
    assert!(result
        .buckets
        .iter()
        .any(|b| !b.slow && b.funnel_started == 10 && b.funnel_completed == 8));
    assert!(result
        .buckets
        .iter()
        .any(|b| b.slow && b.funnel_started == 10 && b.funnel_completed == 5));
}

#[tokio::test]
async fn latency_funnel_impact_requires_session() {
    let app = TestApp::spawn().await;

    let resp = app
        .http
        .get(format!(
            "{}/api/v1/insights/latency-funnel-impact?span_name=/api/checkout",
            app.api_url
        ))
        .send()
        .await
        .expect("send");

    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
