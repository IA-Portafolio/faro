mod common;

use chrono::{DateTime, Duration, TimeZone, Utc};
use common::TestApp;
use faro::storage::ProductEventRow;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct MetricName {
    metric_name: String,
    metric_type: String,
    metric_unit: String,
    service_name: String,
}

#[derive(Debug, Deserialize)]
struct Point {
    ts: String,
    value: f64,
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

async fn seed(app: &TestApp, from: DateTime<Utc>) {
    let project = &app.project_slug;
    let rows = vec![
        product_event(
            from + Duration::minutes(5),
            project,
            "u-1",
            "s-1",
            "checkout_completed",
        ),
        product_event(
            from + Duration::minutes(15),
            project,
            "u-2",
            "s-2",
            "checkout_completed",
        ),
        product_event(
            from + Duration::hours(1) + Duration::minutes(5),
            project,
            "u-3",
            "s-3",
            "checkout_completed",
        ),
        product_event(
            from + Duration::hours(1) + Duration::minutes(15),
            project,
            "u-4",
            "s-4",
            "$pageview",
        ),
    ];

    app.ch
        .insert("faro.product_events", &rows)
        .await
        .expect("insert product events");
}

async fn login(app: &TestApp) -> String {
    let email = app.create_user("hunter2-test").await;
    app.login_session(&email, "hunter2-test").await
}

#[tokio::test]
async fn event_metrics_appear_in_metric_catalog_and_series() {
    let app = TestApp::spawn().await;
    let session = login(&app).await;
    let from = Utc.with_ymd_and_hms(2026, 5, 24, 10, 0, 0).unwrap();
    let to = from + Duration::hours(2);
    seed(&app, from).await;

    let names_resp = app
        .http
        .get(format!("{}/api/v1/metrics/names", app.api_url))
        .query(&[
            ("project", app.project_slug.as_str()),
            ("from", from.to_rfc3339().as_str()),
            ("to", to.to_rfc3339().as_str()),
        ])
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send names");
    assert!(
        names_resp.status().is_success(),
        "GET /metrics/names failed ({}): {}",
        names_resp.status(),
        names_resp.text().await.unwrap_or_default()
    );
    let names: Vec<MetricName> = names_resp.json().await.expect("decode names");
    assert!(names.iter().any(|m| {
        m.metric_name == "events.checkout_completed.count"
            && m.metric_type == "counter"
            && m.metric_unit == "events"
            && m.service_name == "web"
    }));

    let series_resp = app
        .http
        .get(format!("{}/api/v1/metrics/series", app.api_url))
        .query(&[
            ("project", app.project_slug.as_str()),
            ("from", from.to_rfc3339().as_str()),
            ("to", to.to_rfc3339().as_str()),
            ("name", "events.checkout_completed.count"),
            ("service", "web"),
            ("bucket_seconds", "3600"),
        ])
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send series");
    assert!(
        series_resp.status().is_success(),
        "GET /metrics/series failed ({}): {}",
        series_resp.status(),
        series_resp.text().await.unwrap_or_default()
    );
    let series: Vec<Point> = series_resp.json().await.expect("decode series");

    assert_eq!(series.len(), 2);
    assert!(series[0].ts.starts_with("2026-05-24 10:00:00"));
    assert_eq!(series[0].value, 2.0);
    assert!(series[1].ts.starts_with("2026-05-24 11:00:00"));
    assert_eq!(series[1].value, 1.0);
}

#[tokio::test]
async fn event_derived_metrics_require_session() {
    let app = TestApp::spawn().await;

    let resp = app
        .http
        .get(format!(
            "{}/api/v1/metrics/series?name=events.checkout_completed.count",
            app.api_url
        ))
        .send()
        .await
        .expect("send");

    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
