//! OTLP/HTTP+JSON: valida que un payload OTLP real (logs, traces, metrics) se
//! parsea y se persiste en `faro.logs` / `faro.spans` / `faro.metrics`. Usamos
//! shapes mínimos del spec — los campos opcionales que el server ignora no
//! aportan al test y sólo añaden ruido.

mod common;

use common::TestApp;
use serde::Deserialize;

fn ts_nano(secs_ago: i64) -> String {
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    (now - secs_ago * 1_000_000_000).to_string()
}

#[tokio::test]
async fn otlp_logs_persists_to_clickhouse() {
    let app = TestApp::spawn().await;
    let payload = serde_json::json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [
                    { "key": "service.name", "value": { "stringValue": "api-gateway" } }
                ]
            },
            "scopeLogs": [{
                "scope": { "name": "otel-test" },
                "logRecords": [{
                    "timeUnixNano": ts_nano(0),
                    "severityNumber": 17,
                    "severityText": "ERROR",
                    "body": { "stringValue": "upstream timeout" },
                    "attributes": [
                        { "key": "http.status", "value": { "intValue": "504" } }
                    ]
                }]
            }]
        }]
    });

    let resp = app
        .http
        .post(format!("{}/v1/logs", app.otlp_url))
        .bearer_auth(&app.project_token)
        .json(&payload)
        .send()
        .await
        .expect("send");
    assert!(resp.status().is_success(), "status: {}", resp.status());

    let arrived = app
        .wait_for(40, || async { app.count_in("logs").await >= 1 })
        .await;
    assert!(arrived, "log OTLP no llegó a faro.logs");

    #[derive(Deserialize)]
    struct Row {
        service_name: String,
        severity_text: String,
        body: String,
    }
    let rows: Vec<Row> = app
        .ch
        .select_with_params(
            "SELECT service_name, severity_text, body FROM faro.logs \
             WHERE project_id = {p:String} LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .expect("select");
    let r = rows.first().expect("una fila");
    assert_eq!(r.service_name, "api-gateway");
    assert_eq!(r.severity_text, "ERROR");
    assert_eq!(r.body, "upstream timeout");
}

#[tokio::test]
async fn otlp_traces_persists_to_clickhouse() {
    let app = TestApp::spawn().await;
    let start = ts_nano(1);
    let end = ts_nano(0);
    let payload = serde_json::json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [
                    { "key": "service.name", "value": { "stringValue": "orders" } }
                ]
            },
            "scopeSpans": [{
                "scope": { "name": "otel-test" },
                "spans": [{
                    "traceId": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "spanId": "bbbbbbbbbbbbbbbb",
                    "name": "POST /checkout",
                    "kind": 2,
                    "startTimeUnixNano": start,
                    "endTimeUnixNano": end,
                    "status": { "code": 1 },
                    "attributes": [
                        { "key": "http.route", "value": { "stringValue": "/checkout" } }
                    ]
                }]
            }]
        }]
    });

    let resp = app
        .http
        .post(format!("{}/v1/traces", app.otlp_url))
        .bearer_auth(&app.project_token)
        .json(&payload)
        .send()
        .await
        .expect("send");
    assert!(resp.status().is_success(), "status: {}", resp.status());

    let arrived = app
        .wait_for(40, || async { app.count_in("spans").await >= 1 })
        .await;
    assert!(arrived, "span OTLP no llegó a faro.spans");

    #[derive(Deserialize)]
    struct Row {
        service_name: String,
        name: String,
        kind: String,
        status_code: String,
        trace_id: String,
    }
    let rows: Vec<Row> = app
        .ch
        .select_with_params(
            "SELECT service_name, name, kind, status_code, trace_id FROM faro.spans \
             WHERE project_id = {p:String} LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .expect("select");
    let r = rows.first().expect("una fila");
    assert_eq!(r.service_name, "orders");
    assert_eq!(r.name, "POST /checkout");
    assert_eq!(r.kind, "SERVER");
    assert_eq!(r.status_code, "OK");
    assert_eq!(r.trace_id, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
}

#[tokio::test]
async fn otlp_metrics_persists_to_clickhouse() {
    let app = TestApp::spawn().await;
    let payload = serde_json::json!({
        "resourceMetrics": [{
            "resource": {
                "attributes": [
                    { "key": "service.name", "value": { "stringValue": "billing" } }
                ]
            },
            "scopeMetrics": [{
                "scope": { "name": "otel-test" },
                "metrics": [
                    {
                        "name": "requests_total",
                        "unit": "1",
                        "sum": {
                            "isMonotonic": true,
                            "dataPoints": [{
                                "timeUnixNano": ts_nano(0),
                                "asInt": "42"
                            }]
                        }
                    },
                    {
                        "name": "memory_bytes",
                        "unit": "By",
                        "gauge": {
                            "dataPoints": [{
                                "timeUnixNano": ts_nano(0),
                                "asDouble": 1024.5
                            }]
                        }
                    }
                ]
            }]
        }]
    });

    let resp = app
        .http
        .post(format!("{}/v1/metrics", app.otlp_url))
        .bearer_auth(&app.project_token)
        .json(&payload)
        .send()
        .await
        .expect("send");
    assert!(resp.status().is_success(), "status: {}", resp.status());

    let arrived = app
        .wait_for(40, || async { app.count_in("metrics").await >= 2 })
        .await;
    assert!(arrived, "métricas OTLP no llegaron a faro.metrics");

    #[derive(Deserialize)]
    struct Row {
        metric_name: String,
        metric_type: String,
        value: f64,
        service_name: String,
    }
    let rows: Vec<Row> = app
        .ch
        .select_with_params(
            "SELECT metric_name, metric_type, value, service_name FROM faro.metrics \
             WHERE project_id = {p:String} ORDER BY metric_name",
            &[("p", &app.project_slug)],
        )
        .await
        .expect("select");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].metric_name, "memory_bytes");
    assert_eq!(rows[0].metric_type, "gauge");
    assert!((rows[0].value - 1024.5).abs() < 1e-9);
    assert_eq!(rows[0].service_name, "billing");
    assert_eq!(rows[1].metric_name, "requests_total");
    assert_eq!(rows[1].metric_type, "counter");
    assert!((rows[1].value - 42.0).abs() < 1e-9);
}
