//! Ingesta nativa de spans (`POST /api/v1/ingest/spans`): valida el path
//! request → channel → writer → ClickHouse, auth por bearer del proyecto, y
//! que `duration_ns` se derive de `end - start` cuando no se manda explícito.
//!
//! El receiver OTLP (`/v1/traces`) tiene su propio test en `ingest_otlp.rs`.

mod common;

use common::TestApp;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SpanRowOut {
    trace_id: String,
    span_id: String,
    parent_span_id: String,
    name: String,
    kind: String,
    service_name: String,
    duration_ns: u64,
    status_code: String,
    project_id: String,
}

#[tokio::test]
async fn ingest_spans_persists_to_clickhouse() {
    let app = TestApp::spawn().await;

    // start = 1700000000.000000000, end = +500ms → duration_ns = 500_000_000.
    let payload = serde_json::json!({
        "service": "checkout",
        "spans": [
            {
                "trace_id": "0123456789abcdef0123456789abcdef",
                "span_id":  "1111111111111111",
                "parent_span_id": "2222222222222222",
                "name": "charge-order",
                "kind": "client",
                "start": "2023-11-14T22:13:20.000Z",
                "end":   "2023-11-14T22:13:20.500Z",
                "status_code": "OK",
                "attributes": { "order_id": "abc-1", "amount": "9.99" },
            }
        ],
    });

    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/spans", app.api_url))
        .bearer_auth(&app.project_token)
        .json(&payload)
        .send()
        .await
        .expect("send");
    assert!(resp.status().is_success(), "status: {}", resp.status());
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["accepted"], 1);
    assert_eq!(body["dropped_invalid"], 0);
    assert_eq!(body["project"], app.project_slug);

    let arrived = app
        .wait_for(120, || async { app.count_in("spans").await >= 1 })
        .await;
    assert!(arrived, "el span no llegó a faro.spans en 6 s");

    let rows: Vec<SpanRowOut> = app
        .ch
        .select_with_params(
            "SELECT trace_id, span_id, parent_span_id, name, kind, service_name, \
                    duration_ns, status_code, project_id \
             FROM faro.spans WHERE project_id = {p:String} ORDER BY timestamp DESC LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .expect("select");
    let row = rows.first().expect("una fila");
    assert_eq!(row.trace_id, "0123456789abcdef0123456789abcdef");
    assert_eq!(row.span_id, "1111111111111111");
    assert_eq!(row.parent_span_id, "2222222222222222");
    assert_eq!(row.name, "charge-order");
    assert_eq!(row.kind, "client");
    assert_eq!(row.service_name, "checkout");
    assert_eq!(row.duration_ns, 500_000_000);
    assert_eq!(row.status_code, "OK");
    assert_eq!(row.project_id, app.project_slug);
}

#[tokio::test]
async fn ingest_spans_drops_records_without_ids() {
    let app = TestApp::spawn().await;

    let payload = serde_json::json!({
        "service": "checkout",
        "spans": [
            {
                "trace_id": "",
                "span_id":  "",
                "name": "missing-ids",
                "start": "2023-11-14T22:13:20.000Z",
            },
            {
                "trace_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "span_id":  "bbbbbbbbbbbbbbbb",
                "name": "valid-span",
                "start": "2023-11-14T22:13:20.000Z",
            }
        ],
    });

    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/spans", app.api_url))
        .bearer_auth(&app.project_token)
        .json(&payload)
        .send()
        .await
        .expect("send");
    assert!(resp.status().is_success(), "status: {}", resp.status());
    let body: serde_json::Value = resp.json().await.expect("json");
    // Uno aceptado, uno descartado por ids vacíos.
    assert_eq!(body["accepted"], 1);
    assert_eq!(body["dropped_invalid"], 1);
}

#[tokio::test]
async fn ingest_spans_rejects_missing_bearer() {
    let app = TestApp::spawn().await;
    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/spans", app.api_url))
        .json(&serde_json::json!({ "spans": [] }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
