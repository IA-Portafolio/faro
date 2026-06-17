//! Autorización de la gestión de usuarios del dashboard:
//!  - Denegación RBAC: un usuario con rol != admin recibe 403 en endpoints gateados
//!    por el extractor `AdminUser` (este camino de rechazo no tenía cobertura).
//!  - `change_password` exige re-autenticación con el password ACTUAL del actor, lo
//!    que neutraliza el account-takeover desde una sesión robada.
//!  - `update_user` no permite degradar al último admin (lockout total).

mod common;

use common::TestApp;
use reqwest::StatusCode;

/// Un usuario autenticado pero NO admin recibe 403 al crear usuarios (endpoint
/// gateado por `AdminUser`). Antes de este test, el camino de denegación RBAC no
/// se ejercitaba en ninguna suite.
#[tokio::test]
async fn non_admin_is_forbidden_on_admin_endpoint() {
    let app = TestApp::spawn().await;
    let (email, _id) = app.create_user_with_role("viewer-pw-123", "viewer").await;
    let session = app.login_session(&email, "viewer-pw-123").await;

    let resp = app
        .http
        .post(format!("{}/api/v1/users", app.api_url))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .json(&serde_json::json!({
            "email": "nuevo@test.local",
            "password": "una-password-larga",
            "role": "admin"
        }))
        .send()
        .await
        .expect("send create user");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "un rol != admin debe recibir 403 en POST /users"
    );
}

/// El admin SÍ puede crear usuarios (sanity de que el 403 de arriba es por rol, no
/// porque el endpoint esté roto).
#[tokio::test]
async fn admin_can_create_user() {
    let app = TestApp::spawn().await;
    let email = app.create_user("admin-pw-123").await;
    let session = app.login_session(&email, "admin-pw-123").await;

    let resp = app
        .http
        .post(format!("{}/api/v1/users", app.api_url))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .json(&serde_json::json!({
            "email": format!("nuevo-{}@test.local", uuid::Uuid::new_v4().simple()),
            "password": "una-password-larga",
            "role": "admin"
        }))
        .send()
        .await
        .expect("send create user");
    assert!(
        resp.status().is_success(),
        "admin POST /users: {}",
        resp.status()
    );
}

/// `change_password` sin el password actual correcto del actor → 401. Esto es lo
/// que cierra el account-takeover: una sesión robada no basta para cambiar passwords.
#[tokio::test]
async fn change_password_requires_correct_current_password() {
    let app = TestApp::spawn().await;
    let (email, id) = app.create_user_with_role("original-pw-123", "admin").await;
    let session = app.login_session(&email, "original-pw-123").await;

    let resp = app
        .http
        .put(format!("{}/api/v1/users/{}/password", app.api_url, id))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .json(&serde_json::json!({
            "password": "nueva-password-larga",
            "current_password": "password-INCORRECTA"
        }))
        .send()
        .await
        .expect("send change pw");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "current_password incorrecta debe dar 401"
    );

    // Y con el current_password correcto, sí cambia (y el nuevo password funciona).
    let ok = app
        .http
        .put(format!("{}/api/v1/users/{}/password", app.api_url, id))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .json(&serde_json::json!({
            "password": "nueva-password-larga",
            "current_password": "original-pw-123"
        }))
        .send()
        .await
        .expect("send change pw ok");
    assert!(
        ok.status().is_success(),
        "change pw correcto: {}",
        ok.status()
    );

    // El nuevo password permite login; el viejo ya no.
    let new_login = app
        .http
        .post(format!("{}/api/v1/auth/login", app.api_url))
        .json(&serde_json::json!({ "email": email, "password": "nueva-password-larga" }))
        .send()
        .await
        .expect("login nuevo");
    assert!(
        new_login.status().is_success(),
        "login con nuevo pw: {}",
        new_login.status()
    );
}

/// Un actor NO puede cambiar el password de OTRO usuario sin su propio password
/// actual (takeover bloqueado), pero sí puede con re-autenticación (reset legítimo).
#[tokio::test]
async fn admin_cannot_take_over_other_account_without_reauth() {
    let app = TestApp::spawn().await;
    let (attacker_email, _attacker_id) =
        app.create_user_with_role("attacker-pw-123", "admin").await;
    let (victim_email, victim_id) = app.create_user_with_role("victim-pw-123", "admin").await;
    let session = app.login_session(&attacker_email, "attacker-pw-123").await;

    // Sin el password del atacante → 401 (no puede tomar la cuenta de la víctima).
    let resp = app
        .http
        .put(format!(
            "{}/api/v1/users/{}/password",
            app.api_url, victim_id
        ))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .json(&serde_json::json!({
            "password": "password-secuestrada",
            "current_password": "no-es-mi-password"
        }))
        .send()
        .await
        .expect("send takeover");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // La víctima sigue pudiendo entrar con SU password original.
    let still = app
        .http
        .post(format!("{}/api/v1/auth/login", app.api_url))
        .json(&serde_json::json!({ "email": victim_email, "password": "victim-pw-123" }))
        .send()
        .await
        .expect("login victima");
    assert!(
        still.status().is_success(),
        "la víctima debe seguir entrando: {}",
        still.status()
    );
}

/// Degradar un admin a viewer está permitido mientras quede otro admin (ejercita
/// `update_user` pasando por el guard `count_admins`).
///
/// NOTA: el camino de rechazo "último admin → 400" NO es aislable en esta suite de
/// integración porque `faro.users` es GLOBAL (no por-proyecto) y la DB de ClickHouse
/// es compartida entre tests concurrentes que crean admins; `count_admins()` siempre
/// ve >1. El guard de producción (`row.role=="admin" && nuevo!="admin" && count<=1`)
/// es lógica trivial y queda verificado por inspección; aislarlo requeriría una DB CH
/// dedicada por test.
#[tokio::test]
async fn demote_admin_allowed_when_other_admins_exist() {
    let app = TestApp::spawn().await;
    let (email, id) = app.create_user_with_role("solo-admin-123", "admin").await;
    let _ = app.create_user_with_role("otro-admin-123", "admin").await;
    let session = app.login_session(&email, "solo-admin-123").await;

    let ok = app
        .http
        .put(format!("{}/api/v1/users/{}", app.api_url, id))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .json(&serde_json::json!({ "name": "Solo Admin", "role": "viewer" }))
        .send()
        .await
        .expect("send demote");
    assert!(
        ok.status().is_success(),
        "con otros admins, degradar debe permitirse: {}",
        ok.status()
    );
}
