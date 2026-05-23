//! Dashboard authentication: email + password, session-cookie based.
//! Passwords are hashed with Argon2id. Session cookies carry a 32-byte random
//! token; only the SHA-256 hash is stored in ClickHouse, so a DB leak does not
//! expose live sessions.

use std::time::Duration;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::State;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

pub const SESSION_COOKIE: &str = "faro_session";
pub const SESSION_TTL_DAYS: i64 = 30;

#[derive(Clone, Debug, Serialize)]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: String,
}

// ---------- DB row types (deserialized from ClickHouse JSONEachRow) ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_role")]
    pub role: String,
    #[serde(serialize_with = "ser_dt_ms", deserialize_with = "crate::storage::models::de_dt", default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "ser_dt_ms", deserialize_with = "crate::storage::models::de_dt", default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted: u8,
    #[serde(default = "default_one")]
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRow {
    pub token_hash: String,
    pub user_id: Uuid,
    pub user_email: String,
    pub user_name: String,
    pub user_role: String,
    #[serde(serialize_with = "ser_dt_ms", deserialize_with = "crate::storage::models::de_dt", default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "ser_dt_ms", deserialize_with = "crate::storage::models::de_dt")]
    pub expires_at: DateTime<Utc>,
    #[serde(default)]
    pub revoked: u8,
    #[serde(default = "default_one")]
    pub version: u64,
}

fn default_role() -> String { "admin".into() }
fn default_one() -> u64 { 1 }

fn ser_dt_ms<S: serde::Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

// ---------- Password hashing ----------

pub fn hash_password(plain: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash failed: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(plain: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default().verify_password(plain.as_bytes(), &parsed).is_ok()
}

// ---------- Session tokens ----------

fn new_session_token() -> String {
    let mut bytes = [0u8; 32];
    thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Busca una sesión por el token crudo de la cookie y devuelve el usuario asociado
/// si la sesión no está revocada ni expirada.
pub async fn user_from_token(state: &SharedState, token: &str) -> Option<AuthUser> {
    let hash = hash_token(token);
    let sql = format!(
        "SELECT token_hash, user_id, user_email, user_name, user_role, \
                created_at, expires_at, revoked, version \
         FROM faro.user_sessions FINAL \
         WHERE token_hash = '{hash}' AND revoked = 0 AND expires_at > now64(3) LIMIT 1"
    );
    let row: Option<SessionRow> = state.ch.select_one(&sql).await.ok().flatten();
    let row = row?;
    Some(AuthUser {
        id: row.user_id,
        email: row.user_email,
        name: row.user_name,
        role: row.user_role,
    })
}

// ---------- Login / logout / me ----------

#[derive(Deserialize)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

async fn login(
    State(state): State<SharedState>,
    jar: CookieJar,
    Json(input): Json<LoginInput>,
) -> Result<(CookieJar, Json<AuthUser>), ApiError> {
    let email = input.email.trim().to_lowercase();
    if email.is_empty() || input.password.is_empty() {
        return Err(ApiError::BadRequest("email y password son obligatorios".into()));
    }
    let escaped = email.replace('\'', "''");
    let sql = format!(
        "SELECT id, email, password_hash, name, role, \
                created_at, updated_at, deleted, version \
         FROM faro.users FINAL WHERE email = '{escaped}' AND deleted = 0 LIMIT 1"
    );
    let user: Option<UserRow> = state.ch.select_one(&sql).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    let user = user.ok_or(ApiError::Unauthorized)?;
    if !verify_password(&input.password, &user.password_hash) {
        return Err(ApiError::Unauthorized);
    }

    let token = new_session_token();
    let token_hash = hash_token(&token);
    let expires_at = Utc::now() + ChronoDuration::days(SESSION_TTL_DAYS);
    let session = SessionRow {
        token_hash,
        user_id: user.id,
        user_email: user.email.clone(),
        user_name: user.name.clone(),
        user_role: user.role.clone(),
        created_at: Utc::now(),
        expires_at,
        revoked: 0,
        version: Utc::now().timestamp_millis() as u64,
    };
    state
        .ch
        .insert("faro.user_sessions", &[session])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let cookie = build_cookie(token, expires_at);
    let jar = jar.add(cookie);
    Ok((
        jar,
        Json(AuthUser {
            id: user.id,
            email: user.email,
            name: user.name,
            role: user.role,
        }),
    ))
}

fn build_cookie(value: String, expires: DateTime<Utc>) -> Cookie<'static> {
    let mut c = Cookie::new(SESSION_COOKIE, value);
    c.set_http_only(true);
    c.set_secure(true);
    c.set_same_site(SameSite::Lax);
    c.set_path("/");
    c.set_max_age(time::Duration::seconds(
        (expires - Utc::now()).num_seconds().max(0),
    ));
    c
}

async fn logout(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<(CookieJar, Json<serde_json::Value>), ApiError> {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        let hash = hash_token(c.value());
        // Marca revoked = 1 con version aumentada para que ReplacingMergeTree la recoja.
        let sql = format!(
            "INSERT INTO faro.user_sessions \
             SELECT token_hash, user_id, user_email, user_name, user_role, created_at, expires_at, \
                    1 AS revoked, toUInt64(now64(3) * 1000) AS version \
             FROM faro.user_sessions FINAL WHERE token_hash = '{hash}'"
        );
        let _ = state.ch.query_raw(&sql).await;
    }
    // Expira la cookie también del lado del cliente.
    let mut clear = Cookie::new(SESSION_COOKIE, "");
    clear.set_path("/");
    clear.set_max_age(time::Duration::ZERO);
    let jar = jar.remove(Cookie::from(SESSION_COOKIE)).add(clear);
    Ok((jar, Json(serde_json::json!({"ok": true}))))
}

async fn me(user: AuthUser) -> Json<AuthUser> {
    Json(user)
}

// ---------- Routers ----------

pub fn open_router() -> Router<SharedState> {
    Router::new().route("/auth/login", post(login))
}

pub fn protected_router() -> Router<SharedState> {
    Router::new()
        .route("/auth/me", get(me))
        .route("/auth/logout", post(logout))
}

// ---------- Middleware ----------

/// Rutas que se saltan el chequeo de sesión por completo:
///   - `/healthz` (liveness)
///   - `/api/v1/auth/login` (no se puede estar logueado para hacer login)
///   - `/api/v1/ingest/*` (autenticación por token Bearer asociada al proyecto)
///   - `/api/v1/openapi.json` y `/docs/*` (documentación de API)
fn is_public_path(path: &str) -> bool {
    path == "/healthz"
        || path == "/api/v1/auth/login"
        || path == "/api/v1/openapi.json"
        || path == "/docs"
        || path.starts_with("/docs/")
        || path.starts_with("/api/v1/ingest/")
}

pub async fn require_session_mw(
    State(state): State<SharedState>,
    jar: CookieJar,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if is_public_path(req.uri().path()) {
        return next.run(req).await;
    }
    let Some(token) = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()) else {
        return unauthorized().into_response();
    };
    let Some(user) = user_from_token(&state, &token).await else {
        return unauthorized().into_response();
    };
    req.extensions_mut().insert(user);
    next.run(req).await
}

fn unauthorized() -> Response {
    let body = Json(serde_json::json!({"error":"unauthorized","message":"sesión requerida"}));
    (StatusCode::UNAUTHORIZED, body).into_response()
}

// El extractor AuthUser toma el usuario inyectado por el middleware.
#[axum::async_trait]
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or(ApiError::Unauthorized)
    }
}

// ---------- Bootstrap admin ----------

pub async fn bootstrap_admin_if_empty(state: &SharedState) -> anyhow::Result<()> {
    #[derive(Deserialize)]
    struct Cnt { count: u64 }
    let count: Option<Cnt> = state
        .ch
        .select_one("SELECT toUInt64(count()) AS count FROM faro.users FINAL WHERE deleted = 0")
        .await?;
    if count.map(|c| c.count).unwrap_or(0) > 0 {
        return Ok(());
    }
    let email = match std::env::var("FARO_BOOTSTRAP_ADMIN_EMAIL") {
        Ok(v) if !v.is_empty() => v.trim().to_lowercase(),
        _ => {
            tracing::warn!(
                "no admin user exists and FARO_BOOTSTRAP_ADMIN_EMAIL is not set; \
                 the dashboard cannot be logged into until a user is created"
            );
            return Ok(());
        }
    };
    let password = std::env::var("FARO_BOOTSTRAP_ADMIN_PASSWORD")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let p: String = thread_rng()
                .sample_iter(&Alphanumeric)
                .take(20)
                .map(char::from)
                .collect();
            tracing::warn!(
                generated_password = %p,
                "FARO_BOOTSTRAP_ADMIN_PASSWORD not set; generated a random one — copy it now"
            );
            p
        });
    let row = UserRow {
        id: Uuid::new_v4(),
        email: email.clone(),
        password_hash: hash_password(&password)?,
        name: std::env::var("FARO_BOOTSTRAP_ADMIN_NAME").unwrap_or_else(|_| "Admin".into()),
        role: "admin".into(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        deleted: 0,
        version: Utc::now().timestamp_millis() as u64,
    };
    state.ch.insert("faro.users", &[row]).await?;
    tracing::info!(%email, "usuario admin de bootstrap creado");
    let _ = Duration::default(); // silence unused import warning under some feature combos
    Ok(())
}
