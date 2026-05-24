//! Rotación de token de proyecto: tras `POST /projects/{slug}/rotate` el viejo
//! token deja de autenticar (401) y el nuevo funciona (200/202).

mod common;

use common::TestApp;
use serde::Deserialize;

#[derive(Deserialize)]
struct RotateResponse {
    ingest_token: String,
}

async fn ingest_with(app: &TestApp, token: &str) -> reqwest::StatusCode {
    app.http
        .post(format!("{}/api/v1/ingest/logs", app.api_url))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "service": "rotated",
            "logs": [{ "message": "ping" }]
        }))
        .send()
        .await
        .expect("send ingest")
        .status()
}

#[tokio::test]
async fn rotate_invalidates_old_token_and_issues_new_one() {
    let app = TestApp::spawn().await;

    // El token original autentica.
    assert_eq!(
        ingest_with(&app, &app.project_token).await,
        reqwest::StatusCode::OK,
        "el token inicial debería autenticar"
    );

    // Hace falta sesión del dashboard para rotar.
    let email = app.create_user("rotate-pw").await;
    let session = app.login_session(&email, "rotate-pw").await;

    let rotated: RotateResponse = app
        .http
        .post(format!(
            "{}/api/v1/projects/{}/rotate",
            app.api_url, app.project_slug
        ))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send rotate")
        .json()
        .await
        .expect("decode rotate json");
    assert!(!rotated.ingest_token.is_empty());
    assert_ne!(rotated.ingest_token, app.project_token);

    // El handler hace `state.projects.reload(...)` sincrónico — la caché ya
    // refleja el token nuevo cuando rotate retorna.
    assert_eq!(
        ingest_with(&app, &app.project_token).await,
        reqwest::StatusCode::UNAUTHORIZED,
        "el token viejo debería estar invalidado"
    );
    assert_eq!(
        ingest_with(&app, &rotated.ingest_token).await,
        reqwest::StatusCode::OK,
        "el token nuevo debería autenticar"
    );
}

#[tokio::test]
async fn rotate_requires_session() {
    let app = TestApp::spawn().await;
    let resp = app
        .http
        .post(format!(
            "{}/api/v1/projects/{}/rotate",
            app.api_url, app.project_slug
        ))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rotate_unknown_slug_is_not_found() {
    let app = TestApp::spawn().await;
    let email = app.create_user("rotate-pw").await;
    let session = app.login_session(&email, "rotate-pw").await;

    let resp = app
        .http
        .post(format!(
            "{}/api/v1/projects/does-not-exist-{}/rotate",
            app.api_url,
            uuid::Uuid::new_v4().simple()
        ))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}
