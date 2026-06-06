//! Ingesta nativa de métricas (`POST /api/v1/ingest/metrics`): valida el path
//! request → channel → writer → ClickHouse, la auth por bearer del proyecto,
//! y que counters/gauges/histogramas aterricen con `metric_type` correcto.
//!
//! El receiver OTLP (`/v1/metrics`) tiene su propio test en `ingest_otlp.rs`;
//! aquí cubrimos el endpoint corto que usan los SDKs `@iaportafolio/*`.

mod common;

use common::TestApp;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct MetricRowOut {
    metric_name: String,
    metric_type: String,
    metric_unit: String,
    service_name: String,
    value: f64,
    hist_count: u64,
    hist_sum: f64,
    project_id: String,
}

#[tokio::test]
async fn ingest_metrics_persists_counter_to_clickhouse() {
    let app = TestApp::spawn().await;

    let payload = serde_json::json!({
        "service": "checkout",
        "metrics": [
            {
                "name": "http.requests.total",
                "kind": "counter",
                "unit": "1",
                "value": 7,
                "attributes": { "route": "/api/foo", "status": "200" },
            }
        ],
    });

    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/metrics", app.api_url))
        .bearer_auth(&app.project_token)
        .json(&payload)
        .send()
        .await
        .expect("send");
    assert!(resp.status().is_success(), "status: {}", resp.status());
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["accepted"], 1);
    assert_eq!(body["project"], app.project_slug);

    let arrived = app
        .wait_for(120, || async { app.count_in("metrics").await >= 1 })
        .await;
    assert!(arrived, "la métrica no llegó a faro.metrics en 6 s");

    let rows: Vec<MetricRowOut> = app
        .ch
        .select_with_params(
            "SELECT metric_name, metric_type, metric_unit, service_name, value, hist_count, hist_sum, project_id \
             FROM faro.metrics WHERE project_id = {p:String} ORDER BY timestamp DESC LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .expect("select");
    let row = rows.first().expect("una fila");
    assert_eq!(row.metric_name, "http.requests.total");
    assert_eq!(row.metric_type, "counter");
    assert_eq!(row.metric_unit, "1");
    assert_eq!(row.service_name, "checkout");
    assert_eq!(row.value, 7.0);
    assert_eq!(row.hist_count, 0);
    assert_eq!(row.hist_sum, 0.0);
    assert_eq!(row.project_id, app.project_slug);
}

#[tokio::test]
async fn ingest_metrics_persists_histogram_with_buckets() {
    let app = TestApp::spawn().await;

    let payload = serde_json::json!({
        "service": "api",
        "metrics": [
            {
                "name": "http.request.duration_ms",
                "kind": "histogram",
                "unit": "ms",
                "count": 4,
                "sum": 410.0,
                "min": 50.0,
                "max": 200.0,
                "bucket_bounds": [100.0, 250.0],
                "bucket_counts": [2, 1, 1],
            }
        ],
    });

    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/metrics", app.api_url))
        .bearer_auth(&app.project_token)
        .json(&payload)
        .send()
        .await
        .expect("send");
    assert!(resp.status().is_success(), "status: {}", resp.status());

    let arrived = app
        .wait_for(120, || async { app.count_in("metrics").await >= 1 })
        .await;
    assert!(arrived, "el histograma no llegó a faro.metrics en 6 s");

    let rows: Vec<MetricRowOut> = app
        .ch
        .select_with_params(
            "SELECT metric_name, metric_type, metric_unit, service_name, value, hist_count, hist_sum, project_id \
             FROM faro.metrics WHERE project_id = {p:String} ORDER BY timestamp DESC LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .expect("select");
    let row = rows.first().expect("una fila");
    assert_eq!(row.metric_type, "histogram");
    // Para histograma `value` guarda la suma (consistente con la ruta OTLP).
    assert_eq!(row.value, 410.0);
    assert_eq!(row.hist_count, 4);
    assert_eq!(row.hist_sum, 410.0);
}

#[tokio::test]
async fn ingest_metrics_rejects_missing_bearer() {
    let app = TestApp::spawn().await;
    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/metrics", app.api_url))
        .json(&serde_json::json!({ "metrics": [] }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ingest_metrics_rejects_unknown_token() {
    let app = TestApp::spawn().await;
    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/metrics", app.api_url))
        .bearer_auth("not-a-real-token")
        .json(&serde_json::json!({ "metrics": [{ "name": "x", "value": 1 }] }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
