//! Dashboard authentication.
//!
//! - Passwords con Argon2id.
//! - Sesiones por cookie (32-byte random token; sólo el SHA-256 va a DB).
//! - 2FA TOTP opcional por user. Si está habilitado, el login va en dos pasos:
//!   (1) email+password → backend devuelve un `challenge_token` de vida corta,
//!   (2) cliente repite con `challenge_token` + código TOTP/recovery → backend
//!   emite la cookie de sesión.
//! - Rotación: cada login emite un token nuevo (los anteriores siguen vivos hasta
//!   expiración o revoke explícito); cambiar el password revoca todas las demás
//!   sesiones del user dejando viva la actual.

use std::time::Duration;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rand::distr::Alphanumeric;
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::totp;

pub const SESSION_COOKIE: &str = "faro_session";
pub const SESSION_TTL_DAYS: i64 = 30;
/// Vida del `challenge_token` que el backend emite tras la fase 1 del login con 2FA.
/// Suficiente para que el user mire la app, copie el código y lo envíe; corto para
/// que un challenge filtrado expire rápido.
pub const LOGIN_CHALLENGE_TTL_SECS: i64 = 300;

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
    #[serde(
        serialize_with = "ser_dt_ms",
        deserialize_with = "crate::storage::models::de_dt",
        default = "Utc::now"
    )]
    pub created_at: DateTime<Utc>,
    #[serde(
        serialize_with = "ser_dt_ms",
        deserialize_with = "crate::storage::models::de_dt",
        default = "Utc::now"
    )]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted: u8,
    #[serde(default = "default_one")]
    pub version: u64,
    #[serde(default)]
    pub totp_secret: String,
    #[serde(default)]
    pub totp_enabled: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRow {
    pub token_hash: String,
    pub user_id: Uuid,
    pub user_email: String,
    pub user_name: String,
    pub user_role: String,
    #[serde(
        serialize_with = "ser_dt_ms",
        deserialize_with = "crate::storage::models::de_dt",
        default = "Utc::now"
    )]
    pub created_at: DateTime<Utc>,
    #[serde(
        serialize_with = "ser_dt_ms",
        deserialize_with = "crate::storage::models::de_dt"
    )]
    pub expires_at: DateTime<Utc>,
    #[serde(default)]
    pub revoked: u8,
    #[serde(default = "default_one")]
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginChallengeRow {
    pub token_hash: String,
    pub user_id: Uuid,
    pub user_email: String,
    pub user_name: String,
    pub user_role: String,
    #[serde(
        serialize_with = "ser_dt_ms",
        deserialize_with = "crate::storage::models::de_dt",
        default = "Utc::now"
    )]
    pub created_at: DateTime<Utc>,
    #[serde(
        serialize_with = "ser_dt_ms",
        deserialize_with = "crate::storage::models::de_dt"
    )]
    pub expires_at: DateTime<Utc>,
    #[serde(default)]
    pub consumed: u8,
    #[serde(default = "default_one")]
    pub version: u64,
}

fn default_role() -> String {
    "admin".into()
}
fn default_one() -> u64 {
    1
}

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
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok()
}

// ---------- Session tokens ----------

fn new_random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Busca una sesión por el token crudo de la cookie y devuelve el usuario asociado
/// si la sesión no está revocada ni expirada.
pub async fn user_from_token(state: &SharedState, token: &str) -> Option<AuthUser> {
    let hash = hash_token(token);
    let sql = "SELECT token_hash, user_id, user_email, user_name, user_role, \
                created_at, expires_at, revoked, version \
         FROM faro.user_sessions FINAL \
         WHERE token_hash = {hash:String} AND revoked = 0 AND expires_at > now64(3) LIMIT 1";
    let row: Option<SessionRow> = state
        .ch
        .select_one_with_params(sql, &[("hash", &hash)])
        .await
        .ok()
        .flatten();
    let row = row?;
    Some(AuthUser {
        id: row.user_id,
        email: row.user_email,
        name: row.user_name,
        role: row.user_role,
    })
}

/// Inserta una nueva sesión en DB y devuelve el token plaintext (para meter en la cookie).
async fn create_session(state: &SharedState, user: &UserRow) -> ApiResult<(String, DateTime<Utc>)> {
    let token = new_random_token();
    let token_hash = hash_token(&token);
    let now = Utc::now();
    let expires_at = now + ChronoDuration::days(SESSION_TTL_DAYS);
    let session = SessionRow {
        token_hash,
        user_id: user.id,
        user_email: user.email.clone(),
        user_name: user.name.clone(),
        user_role: user.role.clone(),
        created_at: now,
        expires_at,
        revoked: 0,
        version: now.timestamp_millis() as u64,
    };
    state
        .ch
        .insert("faro.user_sessions", &[session])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok((token, expires_at))
}

/// Revoca todas las sesiones de un user EXCEPTO la cuyo token_hash coincide con
/// `keep_token_hash` (puede ser vacío para revocar todas).
pub async fn revoke_user_sessions(
    state: &SharedState,
    user_id: Uuid,
    keep_token_hash: &str,
) -> ApiResult<u64> {
    let id_s = user_id.to_string();
    // Cuenta sesiones activas antes (para devolver cuántas se revocaron).
    let count_sql = "SELECT toUInt64(count()) AS count FROM faro.user_sessions FINAL \
         WHERE user_id = {uid:UUID} AND revoked = 0 \
           AND token_hash != {keep:String} AND expires_at > now64(3)";
    #[derive(Deserialize)]
    struct Cnt {
        count: u64,
    }
    let count: u64 = state
        .ch
        .select_one_with_params::<Cnt>(count_sql, &[("uid", &id_s), ("keep", keep_token_hash)])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map(|c| c.count)
        .unwrap_or(0);

    let revoke_sql = "INSERT INTO faro.user_sessions \
         SELECT token_hash, user_id, user_email, user_name, user_role, created_at, expires_at, \
                1 AS revoked, toUInt64(toUnixTimestamp64Milli(now64(3))) AS version \
         FROM faro.user_sessions FINAL \
         WHERE user_id = {uid:UUID} AND token_hash != {keep:String}";
    state
        .ch
        .query_raw_with_params(revoke_sql, &[("uid", &id_s), ("keep", keep_token_hash)])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(count)
}

// ---------- Login (dos fases si 2FA está activo) ----------

#[derive(Deserialize)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

/// Respuesta de la fase 1 del login. Cuando 2FA está habilitado el cliente recibe
/// `needs_totp: true` + un `challenge_token` y debe llamar a `/auth/login/2fa`.
/// Cuando NO está habilitado, se emite la cookie y se devuelve el user.
#[derive(Serialize)]
#[serde(untagged)]
pub enum LoginResponse {
    Authenticated(AuthUser),
    NeedsTotp {
        needs_totp: bool,
        challenge_token: String,
        /// Segundos hasta la expiración del challenge — para que el frontend
        /// muestre un countdown.
        expires_in_secs: i64,
    },
}

async fn login(
    State(state): State<SharedState>,
    jar: CookieJar,
    Json(input): Json<LoginInput>,
) -> Result<(CookieJar, Json<LoginResponse>), ApiError> {
    let email = input.email.trim().to_lowercase();
    if email.is_empty() || input.password.is_empty() {
        return Err(ApiError::BadRequest(
            "email y password son obligatorios".into(),
        ));
    }
    let user = lookup_user_by_email(&state, &email).await?;
    let user = user.ok_or(ApiError::Unauthorized)?;
    if !verify_password(&input.password, &user.password_hash) {
        return Err(ApiError::Unauthorized);
    }

    if user.totp_enabled == 1 && !user.totp_secret.is_empty() {
        // Fase 1: emitir challenge. NO sentamos la cookie ni emitimos sesión todavía.
        let (challenge_token, expires_at) = create_login_challenge(&state, &user).await?;
        return Ok((
            jar,
            Json(LoginResponse::NeedsTotp {
                needs_totp: true,
                challenge_token,
                expires_in_secs: (expires_at - Utc::now()).num_seconds().max(0),
            }),
        ));
    }

    // 2FA no activo → autenticación completa de una sola pasada.
    let (token, expires_at) = create_session(&state, &user).await?;
    let cookie = build_cookie(token, expires_at);
    let jar = jar.add(cookie);
    Ok((
        jar,
        Json(LoginResponse::Authenticated(AuthUser {
            id: user.id,
            email: user.email,
            name: user.name,
            role: user.role,
        })),
    ))
}

#[derive(Deserialize)]
pub struct LoginTotpInput {
    pub challenge_token: String,
    pub code: String,
    /// Si `true`, el `code` se interpreta como un recovery code en vez de un TOTP.
    #[serde(default)]
    pub recovery: bool,
}

async fn login_totp(
    State(state): State<SharedState>,
    jar: CookieJar,
    Json(input): Json<LoginTotpInput>,
) -> Result<(CookieJar, Json<AuthUser>), ApiError> {
    let challenge = consume_login_challenge(&state, &input.challenge_token).await?;
    // Re-lee el user para tener `totp_secret` actualizado (puede haber cambiado entre fases).
    let user = lookup_user_by_id(&state, challenge.user_id)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    // Rate limit ANTES de verificar. Si está bloqueado, no consultamos DB de recovery
    // codes — sólo devolvemos 429.
    if !state.totp_rl.check_and_record(user.id) {
        return Err(ApiError::TooManyRequests {
            retry_after_secs: 60,
        });
    }

    let ok = if input.recovery {
        consume_recovery_code(&state, user.id, &input.code).await?
    } else {
        if user.totp_secret.is_empty() {
            return Err(ApiError::Unauthorized);
        }
        totp::verify_totp(&user.totp_secret, &user.email, &input.code)
            .map_err(|e| ApiError::Internal(e.to_string()))?
    };
    if !ok {
        return Err(ApiError::Unauthorized);
    }
    state.totp_rl.clear(user.id);

    let (token, expires_at) = create_session(&state, &user).await?;
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

async fn create_login_challenge(
    state: &SharedState,
    user: &UserRow,
) -> ApiResult<(String, DateTime<Utc>)> {
    let token = new_random_token();
    let token_hash = hash_token(&token);
    let now = Utc::now();
    let expires_at = now + ChronoDuration::seconds(LOGIN_CHALLENGE_TTL_SECS);
    let row = LoginChallengeRow {
        token_hash,
        user_id: user.id,
        user_email: user.email.clone(),
        user_name: user.name.clone(),
        user_role: user.role.clone(),
        created_at: now,
        expires_at,
        consumed: 0,
        version: now.timestamp_millis() as u64,
    };
    state
        .ch
        .insert("faro.user_login_challenges", &[row])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok((token, expires_at))
}

/// Lee el challenge, valida que no esté expirado/consumido, y lo marca consumido
/// (idempotencia: un mismo token sólo puede usarse una vez aunque la verificación
/// TOTP falle — el cliente debe pedir un challenge nuevo si erra el código).
async fn consume_login_challenge(
    state: &SharedState,
    plaintext: &str,
) -> ApiResult<LoginChallengeRow> {
    let hash = hash_token(plaintext);
    let sql = "SELECT token_hash, user_id, user_email, user_name, user_role, \
                created_at, expires_at, consumed, version \
         FROM faro.user_login_challenges FINAL \
         WHERE token_hash = {hash:String} AND consumed = 0 AND expires_at > now64(3) LIMIT 1";
    let challenge: LoginChallengeRow = state
        .ch
        .select_one_with_params(sql, &[("hash", &hash)])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or(ApiError::Unauthorized)?;

    // Marca consumido escribiendo otra versión.
    let mut consumed = challenge.clone();
    consumed.consumed = 1;
    consumed.version = Utc::now().timestamp_millis() as u64;
    state
        .ch
        .insert("faro.user_login_challenges", &[consumed])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(challenge)
}

pub async fn lookup_user_by_email(state: &SharedState, email: &str) -> ApiResult<Option<UserRow>> {
    let sql = "SELECT id, email, password_hash, name, role, \
                created_at, updated_at, deleted, version, totp_secret, totp_enabled \
         FROM faro.users FINAL WHERE email = {email:String} AND deleted = 0 LIMIT 1";
    state
        .ch
        .select_one_with_params(sql, &[("email", email)])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))
}

pub async fn lookup_user_by_id(state: &SharedState, id: Uuid) -> ApiResult<Option<UserRow>> {
    let id_s = id.to_string();
    let sql = "SELECT id, email, password_hash, name, role, \
                created_at, updated_at, deleted, version, totp_secret, totp_enabled \
         FROM faro.users FINAL WHERE id = {id:UUID} AND deleted = 0 LIMIT 1";
    state
        .ch
        .select_one_with_params(sql, &[("id", &id_s)])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))
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
        let sql = "INSERT INTO faro.user_sessions \
             SELECT token_hash, user_id, user_email, user_name, user_role, created_at, expires_at, \
                    1 AS revoked, toUInt64(toUnixTimestamp64Milli(now64(3))) AS version \
             FROM faro.user_sessions FINAL WHERE token_hash = {hash:String}";
        let _ = state
            .ch
            .query_raw_with_params(sql, &[("hash", &hash)])
            .await;
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

// ---------- Recovery codes ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryCodeRow {
    pub user_id: Uuid,
    pub code_hash: String,
    #[serde(
        serialize_with = "ser_dt_ms",
        deserialize_with = "crate::storage::models::de_dt",
        default = "Utc::now"
    )]
    pub created_at: DateTime<Utc>,
    #[serde(
        default,
        deserialize_with = "de_dt_ms_opt",
        serialize_with = "ser_dt_ms_opt"
    )]
    pub used_at: Option<DateTime<Utc>>,
    #[serde(default = "default_one")]
    pub version: u64,
}

fn ser_dt_ms_opt<S: serde::Serializer>(t: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
    match t {
        Some(v) => s.serialize_str(&v.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        None => s.serialize_none(),
    }
}

fn de_dt_ms_opt<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
    let opt: Option<String> = Option::deserialize(d)?;
    match opt.as_deref() {
        None | Some("") => Ok(None),
        Some(s) => Ok(crate::storage::models::parse_dt_pub(s)),
    }
}

/// Borra los recovery codes previos del user e inserta los nuevos. Devuelve la lista
/// PLAINTEXT — el caller debe mostrarla AL USER UNA SOLA VEZ.
pub async fn replace_recovery_codes(state: &SharedState, user_id: Uuid) -> ApiResult<Vec<String>> {
    let codes = totp::generate_recovery_codes();
    let now = Utc::now();

    // Borrado lógico: re-insertar las filas existentes con `used_at = now` y version+1
    // hace que cualquier intento de uso falle por la cláusula `used_at IS NULL`.
    let id_s = user_id.to_string();
    let invalidate_sql = "INSERT INTO faro.user_recovery_codes \
         SELECT user_id, code_hash, created_at, \
                toNullable(now64(3)) AS used_at, \
                toUInt64(toUnixTimestamp64Milli(now64(3))) AS version \
         FROM faro.user_recovery_codes FINAL \
         WHERE user_id = {uid:UUID} AND used_at IS NULL";
    state
        .ch
        .query_raw_with_params(invalidate_sql, &[("uid", &id_s)])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let rows: Vec<RecoveryCodeRow> = codes
        .iter()
        .map(|c| RecoveryCodeRow {
            user_id,
            code_hash: totp::hash_recovery_code(c),
            created_at: now,
            used_at: None,
            version: now.timestamp_millis() as u64,
        })
        .collect();
    state
        .ch
        .insert("faro.user_recovery_codes", &rows)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(codes)
}

/// Si el code es uno válido y no usado, lo marca usado y devuelve `true`.
/// Si no match, devuelve `false`. No revela cuál falló — `false` significa "no entra".
pub async fn consume_recovery_code(
    state: &SharedState,
    user_id: Uuid,
    code: &str,
) -> ApiResult<bool> {
    let normalized = totp::normalize_recovery_code(code);
    if normalized.is_empty() {
        return Ok(false);
    }
    let hash = totp::hash_recovery_code(&normalized);
    let id_s = user_id.to_string();

    let sql = "SELECT user_id, code_hash, created_at, used_at, version \
         FROM faro.user_recovery_codes FINAL \
         WHERE user_id = {uid:UUID} AND code_hash = {hash:String} AND used_at IS NULL LIMIT 1";
    let row: Option<RecoveryCodeRow> = state
        .ch
        .select_one_with_params(sql, &[("uid", &id_s), ("hash", &hash)])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let Some(mut row) = row else {
        return Ok(false);
    };
    row.used_at = Some(Utc::now());
    row.version = Utc::now().timestamp_millis() as u64;
    state
        .ch
        .insert("faro.user_recovery_codes", &[row])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(true)
}

/// Cantidad de recovery codes aún sin usar para `user_id`.
pub async fn count_unused_recovery_codes(state: &SharedState, user_id: Uuid) -> ApiResult<u64> {
    #[derive(Deserialize)]
    struct Cnt {
        count: u64,
    }
    let id_s = user_id.to_string();
    let sql = "SELECT toUInt64(count()) AS count FROM faro.user_recovery_codes FINAL \
         WHERE user_id = {uid:UUID} AND used_at IS NULL";
    let res: Option<Cnt> = state
        .ch
        .select_one_with_params(sql, &[("uid", &id_s)])
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(res.map(|c| c.count).unwrap_or(0))
}

// ---------- Routers ----------

pub fn open_router() -> Router<SharedState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/login/2fa", post(login_totp))
}

pub fn protected_router() -> Router<SharedState> {
    Router::new()
        .route("/auth/me", get(me))
        .route("/auth/logout", post(logout))
}

// ---------- Middleware ----------

/// Rutas que se saltan el chequeo de sesión por completo:
///   - `/healthz` (liveness — sin dependencias, siempre 200 si el proceso vive)
///   - `/readyz` (readiness — ping a ClickHouse + Redis; 503 si CH falla)
///   - `/api/v1/auth/login` y `/api/v1/auth/login/2fa` (no se puede estar logueado para hacer login)
///   - `/api/v1/ingest/*` (autenticación por token Bearer asociada al proyecto)
///   - `/api/v1/openapi.json` y `/docs/*` (documentación de API)
///   - `/metrics` (Prometheus; auth opcional via `FARO_METRICS_TOKEN` se valida
///     dentro del handler para que un scrapper no necesite cookie de sesión)
fn is_public_path(path: &str) -> bool {
    path == "/healthz"
        || path == "/readyz"
        || path == "/api/v1/auth/login"
        || path == "/api/v1/auth/login/2fa"
        || path == "/api/v1/openapi.json"
        || path == "/metrics"
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
    // Adjuntamos también el hash del token al request para que handlers como
    // "revoke other sessions" puedan saber cuál sesión preservar (la actual).
    req.extensions_mut().insert(user);
    req.extensions_mut()
        .insert(CurrentSessionTokenHash(hash_token(&token)));
    next.run(req).await
}

/// Extractor opcional para el SHA-256 del token de la sesión actual. Sólo lo inyecta
/// `require_session_mw`; los endpoints públicos no lo tienen.
#[derive(Clone)]
pub struct CurrentSessionTokenHash(pub String);

fn unauthorized() -> Response {
    let body = Json(serde_json::json!({"error":"unauthorized","message":"sesión requerida"}));
    (StatusCode::UNAUTHORIZED, body).into_response()
}

// El extractor AuthUser toma el usuario inyectado por el middleware.
// axum 0.8 usa async-fn-in-trait nativo: ya no se necesita #[async_trait].
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

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for CurrentSessionTokenHash {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<CurrentSessionTokenHash>()
            .cloned()
            .ok_or(ApiError::Unauthorized)
    }
}

// ---------- Bootstrap admin ----------

pub async fn bootstrap_admin_if_empty(state: &SharedState) -> anyhow::Result<()> {
    #[derive(Deserialize)]
    struct Cnt {
        count: u64,
    }
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
            let p: String = rand::rng()
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
        totp_secret: String::new(),
        totp_enabled: 0,
    };
    state.ch.insert("faro.users", &[row]).await?;
    tracing::info!(%email, "usuario admin de bootstrap creado");
    let _ = Duration::default(); // silence unused import warning under some feature combos
    Ok(())
}
