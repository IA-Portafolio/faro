//! Login → `/auth/me` → logout → revocada. Expiración se cubre fabricando una
//! `SessionRow` con `expires_at` en el pasado (no esperamos 30 días).

mod common;

use chrono::{Duration, Utc};
use common::{extract_session_cookie, TestApp};
use faro::auth::{hash_token, SessionRow};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct MeResponse {
    id: Uuid,
    email: String,
    role: String,
}

#[tokio::test]
async fn login_me_returns_authenticated_user() {
    let app = TestApp::spawn().await;
    let email = app.create_user("correct horse").await;
    let session = app.login_session(&email, "correct horse").await;

    let me: MeResponse = app
        .http
        .get(format!("{}/api/v1/auth/me", app.api_url))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send /me")
        .json()
        .await
        .expect("json");
    assert_eq!(me.email, email);
    assert_eq!(me.role, "admin");
}

#[tokio::test]
async fn login_rejects_wrong_password() {
    let app = TestApp::spawn().await;
    let email = app.create_user("password-A").await;
    let resp = app
        .http
        .post(format!("{}/api/v1/auth/login", app.api_url))
        .json(&serde_json::json!({ "email": email, "password": "password-B" }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(extract_session_cookie(resp.headers()).is_none());
}

#[tokio::test]
async fn me_without_cookie_is_unauthorized() {
    let app = TestApp::spawn().await;
    let resp = app
        .http
        .get(format!("{}/api/v1/auth/me", app.api_url))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_revokes_session() {
    let app = TestApp::spawn().await;
    let email = app.create_user("logout-pw").await;
    let session = app.login_session(&email, "logout-pw").await;
    let token_hash = hash_token(&session);

    // Sanity: la sesión funciona antes de logout.
    let pre = app
        .http
        .get(format!("{}/api/v1/auth/me", app.api_url))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send pre");
    assert!(
        pre.status().is_success(),
        "pre-logout /me: {}",
        pre.status()
    );

    // Login y logout escriben `version = now*1000` (ms desde epoch). Sin un
    // respiro caen en el mismo ms → ReplacingMergeTree no garantiza qué fila
    // gana en `FINAL`. 10 ms basta para versions distintas.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let logout = app
        .http
        .post(format!("{}/api/v1/auth/logout", app.api_url))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send logout");
    assert!(
        logout.status().is_success(),
        "logout status: {}",
        logout.status()
    );

    // Side-effect en DB: el handler insertó una fila con revoked=1 con version
    // mayor que la del login. Si la regresión vuelve (el `let _ =` en logout
    // silencia errores), este chequeo lo agarra antes que el /me siguiente.
    #[derive(serde::Deserialize, Debug)]
    struct Row {
        revoked: u8,
    }
    let final_row: Vec<Row> = app
        .ch
        .select_with_params(
            "SELECT revoked FROM faro.user_sessions FINAL \
             WHERE token_hash = {h:String}",
            &[("h", &token_hash)],
        )
        .await
        .expect("select session row");
    assert_eq!(
        final_row.first().map(|r| r.revoked),
        Some(1),
        "tras logout, la fila visible por FINAL debe tener revoked=1"
    );

    // Y end-to-end: `/me` con la cookie vieja devuelve 401.
    let post = app
        .http
        .get(format!("{}/api/v1/auth/me", app.api_url))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send post");
    assert_eq!(post.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn expired_session_is_rejected() {
    let app = TestApp::spawn().await;
    let email = app.create_user("expired-pw").await;

    // Insertamos una SessionRow ya expirada (1 hora atrás) directamente, en vez
    // de esperar 30 días. La query de middleware filtra por
    // `expires_at > now64(3)` → debe rechazar.
    let token = format!("expired-{}", Uuid::new_v4().simple());
    let user_id = Uuid::new_v4();
    let now = Utc::now();
    let row = SessionRow {
        token_hash: hash_token(&token),
        user_id,
        user_email: email.clone(),
        user_name: "Test".into(),
        user_role: "admin".into(),
        created_at: now - Duration::hours(2),
        expires_at: now - Duration::hours(1),
        revoked: 0,
        version: now.timestamp_millis() as u64,
    };
    app.ch
        .insert("faro.user_sessions", &[row])
        .await
        .expect("insert expired session");

    let resp = app
        .http
        .get(format!("{}/api/v1/auth/me", app.api_url))
        .header(reqwest::header::COOKIE, format!("faro_session={token}"))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
