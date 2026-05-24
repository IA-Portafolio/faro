//! Ingesta nativa de logs (`POST /api/v1/ingest/logs`): valida el path completo
//! request → channel → writer → ClickHouse, y la auth por bearer del proyecto.

mod common;

use common::TestApp;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct LogRowOut {
    service_name: String,
    severity_text: String,
    severity_number: u8,
    body: String,
    trace_id: String,
    project_id: String,
}

#[tokio::test]
async fn ingest_logs_persists_to_clickhouse() {
    let app = TestApp::spawn().await;

    let payload = serde_json::json!({
        "service": "checkout",
        "logs": [
            {
                "level": "error",
                "message": "card declined",
                "trace_id": "0123456789abcdef0123456789abcdef",
                "attributes": { "card.last4": "4242", "amount": "9.99" },
            }
        ],
    });

    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/logs", app.api_url))
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
        .wait_for(40, || async { app.count_in("logs").await >= 1 })
        .await;
    assert!(arrived, "el log no llegó a faro.logs en 2 s");

    let rows: Vec<LogRowOut> = app
        .ch
        .select_with_params(
            "SELECT service_name, severity_text, severity_number, body, trace_id, project_id \
             FROM faro.logs WHERE project_id = {p:String} ORDER BY timestamp DESC LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .expect("select");
    let row = rows.first().expect("una fila");
    assert_eq!(row.service_name, "checkout");
    assert_eq!(row.severity_text, "ERROR");
    assert_eq!(row.severity_number, 17);
    assert_eq!(row.body, "card declined");
    assert_eq!(row.trace_id, "0123456789abcdef0123456789abcdef");
    assert_eq!(row.project_id, app.project_slug);
}

#[tokio::test]
async fn ingest_logs_rejects_missing_bearer() {
    let app = TestApp::spawn().await;
    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/logs", app.api_url))
        .json(&serde_json::json!({ "logs": [] }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ingest_logs_rejects_unknown_token() {
    let app = TestApp::spawn().await;
    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/logs", app.api_url))
        .bearer_auth("not-a-real-token")
        .json(&serde_json::json!({ "logs": [{ "message": "x" }] }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
