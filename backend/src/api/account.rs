//! Endpoints de seguridad del usuario actual:
//!   GET    /api/v1/me/sessions               → lista de sesiones activas
//!   POST   /api/v1/me/sessions/revoke-others → revoca todas menos la actual
//!   GET    /api/v1/me/security/2fa           → status: enabled, recovery_remaining
//!   POST   /api/v1/me/security/2fa/setup     → inicia enrolamiento (devuelve secret + QR)
//!   POST   /api/v1/me/security/2fa/enable    → verifica código y activa, devuelve recovery codes (UNA vez)
//!   POST   /api/v1/me/security/2fa/disable   → password + (TOTP o recovery) → off
//!   POST   /api/v1/me/security/2fa/recovery-codes → password + TOTP → nuevos códigos (UNA vez)

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::{
    self, count_unused_recovery_codes, lookup_user_by_id, replace_recovery_codes,
    revoke_user_sessions, AuthUser, CurrentSessionTokenHash, SessionRow,
};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::totp;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/me/sessions", get(list_sessions))
        .route("/me/sessions/revoke-others", post(revoke_others))
        .route("/me/security/2fa", get(twofa_status))
        .route("/me/security/2fa/setup", post(twofa_setup))
        .route("/me/security/2fa/enable", post(twofa_enable))
        .route("/me/security/2fa/disable", post(twofa_disable))
        .route(
            "/me/security/2fa/recovery-codes",
            post(twofa_regen_recovery),
        )
}

// ---------- Sessions ----------

#[derive(Serialize)]
pub struct SessionView {
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub is_current: bool,
}

async fn list_sessions(
    user: AuthUser,
    State(state): State<SharedState>,
    current: CurrentSessionTokenHash,
) -> ApiResult<Json<Vec<SessionView>>> {
    let id_s = user.id.to_string();
    let sql = "SELECT token_hash, user_id, user_email, user_name, user_role, \
                created_at, expires_at, revoked, version \
         FROM faro.user_sessions FINAL \
         WHERE user_id = {uid:UUID} AND revoked = 0 AND expires_at > now64(3) \
         ORDER BY created_at DESC";
    let rows: Vec<SessionRow> = state.ch.select_with_params(sql, &[("uid", &id_s)]).await?;
    let out: Vec<SessionView> = rows
        .into_iter()
        .map(|r| SessionView {
            is_current: r.token_hash == current.0,
            token_hash: r.token_hash,
            created_at: r.created_at,
            expires_at: r.expires_at,
        })
        .collect();
    Ok(Json(out))
}

async fn revoke_others(
    user: AuthUser,
    State(state): State<SharedState>,
    current: CurrentSessionTokenHash,
) -> ApiResult<Json<serde_json::Value>> {
    let revoked = revoke_user_sessions(&state, user.id, &current.0).await?;
    Ok(Json(serde_json::json!({ "revoked": revoked })))
}

// ---------- 2FA ----------

#[derive(Serialize)]
pub struct TwoFaStatus {
    pub enabled: bool,
    /// Recovery codes que quedan sin usar. Sólo significativo cuando `enabled`.
    pub recovery_codes_remaining: u64,
}

async fn twofa_status(
    user: AuthUser,
    State(state): State<SharedState>,
) -> ApiResult<Json<TwoFaStatus>> {
    let row = lookup_user_by_id(&state, user.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let remaining = if row.totp_enabled == 1 {
        count_unused_recovery_codes(&state, user.id).await?
    } else {
        0
    };
    Ok(Json(TwoFaStatus {
        enabled: row.totp_enabled == 1,
        recovery_codes_remaining: remaining,
    }))
}

#[derive(Serialize)]
pub struct TwoFaSetupView {
    /// Base32 del secreto. El cliente lo muestra para "entrada manual" en
    /// authenticators que no escanean QR.
    pub secret_base32: String,
    /// otpauth:// URL crudo. Útil para clients que ya saben generar QR por su lado.
    pub otpauth_url: String,
    /// SVG inline del QR (text/svg+xml). Se renderiza con `{@html}` dentro de un
    /// contenedor; CSP estricto permite SVG inline porque viene del mismo origen.
    pub qr_svg: String,
}

/// Genera y devuelve un secreto candidato. NO lo persiste todavía — el secreto
/// queda "in-flight" en el TOTP_PENDING_SECRETS in-memory por user_id. Cuando el
/// user llame a `enable` con un código válido contra el pendiente, se promueve a
/// persistente y se activa. Si nunca lo confirma, se descarta.
async fn twofa_setup(
    user: AuthUser,
    State(state): State<SharedState>,
) -> ApiResult<Json<TwoFaSetupView>> {
    // Si el user ya tiene 2FA activo, exigimos disable antes — evita confusión
    // sobre cuál secreto está vigente y obliga al disable explícito (que pide TOTP).
    let row = lookup_user_by_id(&state, user.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if row.totp_enabled == 1 {
        return Err(ApiError::BadRequest(
            "2FA ya está activo; deshabilítalo primero para regenerar el secreto".into(),
        ));
    }
    let secret = totp::generate_secret_base32();
    let url =
        totp::otpauth_url(&secret, &user.email).map_err(|e| ApiError::Internal(e.to_string()))?;
    let qr = totp::otpauth_qr_svg(&secret, &user.email)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    state.pending_totp.set(user.id, secret.clone());
    Ok(Json(TwoFaSetupView {
        secret_base32: secret,
        otpauth_url: url,
        qr_svg: qr,
    }))
}

#[derive(Deserialize)]
pub struct TwoFaEnableInput {
    pub code: String,
}

#[derive(Serialize)]
pub struct TwoFaEnableResult {
    pub enabled: bool,
    pub recovery_codes: Vec<String>,
}

async fn twofa_enable(
    user: AuthUser,
    State(state): State<SharedState>,
    Json(input): Json<TwoFaEnableInput>,
) -> ApiResult<Json<TwoFaEnableResult>> {
    let pending = state.pending_totp.get(user.id).ok_or_else(|| {
        ApiError::BadRequest("no hay setup en curso; llama a /setup primero".into())
    })?;

    // Rate limit también en setup — si el user mete códigos al voleo, no le
    // dejamos brute-forcear el código de verificación inicial.
    if !state.totp_rl.check_and_record(user.id) {
        return Err(ApiError::TooManyRequests {
            retry_after_secs: 60,
        });
    }
    let ok = totp::verify_totp(&pending, &user.email, &input.code)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    if !ok {
        return Err(ApiError::BadRequest("código inválido".into()));
    }
    state.totp_rl.clear(user.id);

    // Promueve el secreto a persistente y activa 2FA.
    let mut row = lookup_user_by_id(&state, user.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    row.totp_secret = pending;
    row.totp_enabled = 1;
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.users", &[row]).await?;
    state.pending_totp.clear(user.id);

    // Genera y entrega los recovery codes (UNA sola vez al user, hashes a DB).
    let codes = replace_recovery_codes(&state, user.id).await?;
    Ok(Json(TwoFaEnableResult {
        enabled: true,
        recovery_codes: codes,
    }))
}

#[derive(Deserialize)]
pub struct TwoFaDisableInput {
    pub password: String,
    /// Código TOTP de 6 dígitos. Alternativa: `recovery_code`.
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub recovery_code: String,
}

async fn twofa_disable(
    user: AuthUser,
    State(state): State<SharedState>,
    Json(input): Json<TwoFaDisableInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut row = lookup_user_by_id(&state, user.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if row.totp_enabled != 1 {
        return Err(ApiError::BadRequest("2FA no está activo".into()));
    }
    if !auth::verify_password(&input.password, &row.password_hash) {
        return Err(ApiError::Unauthorized);
    }

    if !state.totp_rl.check_and_record(user.id) {
        return Err(ApiError::TooManyRequests {
            retry_after_secs: 60,
        });
    }
    let ok = if !input.code.trim().is_empty() {
        totp::verify_totp(&row.totp_secret, &user.email, &input.code)
            .map_err(|e| ApiError::Internal(e.to_string()))?
    } else if !input.recovery_code.trim().is_empty() {
        auth::consume_recovery_code(&state, user.id, &input.recovery_code).await?
    } else {
        return Err(ApiError::BadRequest(
            "code o recovery_code es obligatorio".into(),
        ));
    };
    if !ok {
        return Err(ApiError::Unauthorized);
    }
    state.totp_rl.clear(user.id);

    row.totp_secret = String::new();
    row.totp_enabled = 0;
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.users", &[row]).await?;
    // Borra recovery codes restantes (one-shot todos).
    invalidate_all_recovery_codes(&state, user.id).await?;
    Ok(Json(serde_json::json!({ "enabled": false })))
}

#[derive(Deserialize)]
pub struct TwoFaRegenInput {
    pub password: String,
    pub code: String,
}

async fn twofa_regen_recovery(
    user: AuthUser,
    State(state): State<SharedState>,
    Json(input): Json<TwoFaRegenInput>,
) -> ApiResult<Json<TwoFaEnableResult>> {
    let row = lookup_user_by_id(&state, user.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if row.totp_enabled != 1 {
        return Err(ApiError::BadRequest("2FA no está activo".into()));
    }
    if !auth::verify_password(&input.password, &row.password_hash) {
        return Err(ApiError::Unauthorized);
    }
    if !state.totp_rl.check_and_record(user.id) {
        return Err(ApiError::TooManyRequests {
            retry_after_secs: 60,
        });
    }
    let ok = totp::verify_totp(&row.totp_secret, &user.email, &input.code)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    if !ok {
        return Err(ApiError::Unauthorized);
    }
    state.totp_rl.clear(user.id);
    let codes = replace_recovery_codes(&state, user.id).await?;
    Ok(Json(TwoFaEnableResult {
        enabled: true,
        recovery_codes: codes,
    }))
}

async fn invalidate_all_recovery_codes(state: &SharedState, user_id: uuid::Uuid) -> ApiResult<()> {
    let id_s = user_id.to_string();
    let sql = "INSERT INTO faro.user_recovery_codes \
         SELECT user_id, code_hash, created_at, \
                toNullable(now64(3)) AS used_at, \
                toUInt64(toUnixTimestamp64Milli(now64(3))) AS version \
         FROM faro.user_recovery_codes FINAL \
         WHERE user_id = {uid:UUID} AND used_at IS NULL";
    state
        .ch
        .query_raw_with_params(sql, &[("uid", &id_s)])
        .await?;
    Ok(())
}
