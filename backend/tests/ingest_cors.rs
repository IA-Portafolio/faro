//! CORS de la capa de ingesta (`POST /api/v1/ingest/*`).
//!
//! Regresión del incidente de prod: el browser/RUM SDK corriendo en un dominio
//! de cliente (p. ej. `https://emporio.host`) recibía
//! "No 'Access-Control-Allow-Origin' header is present on the requested resource"
//! en el preflight, porque los endpoints nativos de ingesta se servían bajo el
//! CORS restrictivo del dashboard. Estos tests fijan el contrato:
//!  - `/api/v1/ingest/*` responde el preflight con `Access-Control-Allow-Origin`
//!    para CUALQUIER origen (permisivo, `Any`).
//!  - El sub-router del dashboard sigue restringido a sus orígenes (en tests, sin
//!    `FARO_DASHBOARD_ORIGINS`, sólo los localhost de dev) — un origen ajeno NO
//!    recibe el header.

mod common;

use common::TestApp;
use reqwest::header::ACCESS_CONTROL_ALLOW_ORIGIN;
use reqwest::Method;

const CUSTOMER_ORIGIN: &str = "https://emporio.host";

#[tokio::test]
async fn ingest_preflight_allows_any_origin() {
    let app = TestApp::spawn().await;

    // Preflight tal como lo emite el browser antes del POST cross-origin.
    let resp = app
        .http
        .request(
            Method::OPTIONS,
            format!("{}/api/v1/ingest/logs", app.api_url),
        )
        .header(reqwest::header::ORIGIN, CUSTOMER_ORIGIN)
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type",
        )
        .send()
        .await
        .expect("send preflight");

    assert!(
        resp.status().is_success(),
        "el preflight debería responder 2xx; status: {}",
        resp.status()
    );
    let acao = resp
        .headers()
        .get(ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|v| v.to_str().ok());
    assert_eq!(
        acao,
        Some("*"),
        "el preflight de ingesta debe permitir cualquier origen (Access-Control-Allow-Origin: *), \
         obtuve {acao:?}"
    );
}

#[tokio::test]
async fn ingest_actual_post_carries_cors_header() {
    let app = TestApp::spawn().await;

    // El POST real (no-preflight) desde el dominio del cliente debe llevar el
    // ACAO en la respuesta y procesarse normalmente.
    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/logs", app.api_url))
        .bearer_auth(&app.project_token)
        .header(reqwest::header::ORIGIN, CUSTOMER_ORIGIN)
        .json(&serde_json::json!({
            "service": "cors-svc",
            "logs": [{ "level": "info", "message": "hello from a customer domain" }],
        }))
        .send()
        .await
        .expect("send post");

    let acao = resp
        .headers()
        .get(ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    assert!(
        resp.status().is_success(),
        "el POST de ingesta debería aceptar; status: {}",
        resp.status()
    );
    assert_eq!(
        acao.as_deref(),
        Some("*"),
        "el POST real de ingesta debe llevar Access-Control-Allow-Origin: *"
    );
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["accepted"], 1);
    assert_eq!(body["project"], app.project_slug);
}

#[tokio::test]
async fn dashboard_preflight_stays_restrictive() {
    let app = TestApp::spawn().await;

    // Un origen ajeno NO debe recibir ACAO en un endpoint del dashboard: el CORS
    // del dashboard es independiente y restrictivo (en tests, sólo localhost).
    let foreign = app
        .http
        .request(Method::OPTIONS, format!("{}/api/v1/logs", app.api_url))
        .header(reqwest::header::ORIGIN, CUSTOMER_ORIGIN)
        .header("access-control-request-method", "GET")
        .send()
        .await
        .expect("send foreign preflight");
    assert!(
        foreign.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none(),
        "el dashboard NO debe permitir un origen ajeno ({CUSTOMER_ORIGIN}); \
         no abrir el dashboard a cualquier dominio"
    );

    // Un origen de la whitelist de dev sí recibe ACAO, reflejado a ese origen.
    let allowed = app
        .http
        .request(Method::OPTIONS, format!("{}/api/v1/logs", app.api_url))
        .header(reqwest::header::ORIGIN, "http://localhost:5173")
        .header("access-control-request-method", "GET")
        .send()
        .await
        .expect("send allowed preflight");
    let acao = allowed
        .headers()
        .get(ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|v| v.to_str().ok());
    assert_eq!(
        acao,
        Some("http://localhost:5173"),
        "el dashboard debe reflejar su origen permitido; obtuve {acao:?}"
    );
}
