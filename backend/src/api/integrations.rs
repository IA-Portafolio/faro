//! API REST para configurar integraciones externas desde el dashboard.
//!
//! Endpoints:
//!   GET    /api/v1/integrations/telegram         → estado actual (token enmascarado)
//!   PUT    /api/v1/integrations/telegram         → upsert config
//!   DELETE /api/v1/integrations/telegram         → desactiva y vacía el token
//!   POST   /api/v1/integrations/telegram/test    → envía un mensaje de prueba

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::integrations::{self, TelegramConfig};
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route(
            "/integrations/telegram",
            get(get_telegram).put(put_telegram).delete(delete_telegram),
        )
        .route("/integrations/telegram/test", post(test_telegram))
}

#[derive(Serialize)]
struct TelegramView {
    /// `true` si hay un token guardado y la integración está habilitada.
    configured: bool,
    enabled: bool,
    /// Token enmascarado para no exponerlo al frontend. Vacío si no hay token.
    bot_token_masked: String,
    default_chat_id: String,
    updated_at: Option<String>,
    updated_by: String,
}

#[derive(Deserialize)]
struct TelegramInput {
    /// Token completo. Si llega vacío y ya hay uno guardado, se mantiene el actual.
    #[serde(default)]
    bot_token: String,
    #[serde(default)]
    default_chat_id: String,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool { true }

#[derive(Deserialize)]
struct TestInput {
    chat_id: String,
    #[serde(default)]
    text: String,
}

fn mask(token: &str) -> String {
    let t = token.trim();
    if t.is_empty() {
        return String::new();
    }
    // Telegram tokens son `<id>:<resto>`. Mostramos el `<id>:` y los últimos 4
    // caracteres para que el usuario reconozca de qué bot se trata sin filtrarlo.
    let (head, tail) = match t.split_once(':') {
        Some((h, rest)) => (h.to_string(), rest),
        None => (String::new(), t),
    };
    let last4: String = tail.chars().rev().take(4).collect::<String>().chars().rev().collect();
    if head.is_empty() {
        format!("****{last4}")
    } else {
        format!("{head}:****{last4}")
    }
}

async fn read_telegram(state: &SharedState) -> ApiResult<TelegramView> {
    use crate::storage::IntegrationRow;
    let row: Option<IntegrationRow> = state
        .ch
        .select_one(
            "SELECT kind, enabled, config, updated_at, updated_by, version \
             FROM faro.integrations FINAL WHERE kind = 'telegram' LIMIT 1",
        )
        .await?;
    Ok(match row {
        Some(r) => {
            let cfg: TelegramConfig = serde_json::from_str(&r.config).unwrap_or_default();
            TelegramView {
                configured: cfg.is_configured() && r.enabled == 1,
                enabled: r.enabled == 1,
                bot_token_masked: mask(&cfg.bot_token),
                default_chat_id: cfg.default_chat_id,
                updated_at: Some(r.updated_at.to_rfc3339()),
                updated_by: r.updated_by,
            }
        }
        None => TelegramView {
            configured: false,
            enabled: false,
            bot_token_masked: String::new(),
            default_chat_id: String::new(),
            updated_at: None,
            updated_by: String::new(),
        },
    })
}

async fn get_telegram(
    _admin: AuthUser,
    State(state): State<SharedState>,
) -> ApiResult<Json<TelegramView>> {
    Ok(Json(read_telegram(&state).await?))
}

async fn put_telegram(
    admin: AuthUser,
    State(state): State<SharedState>,
    Json(input): Json<TelegramInput>,
) -> ApiResult<Json<TelegramView>> {
    // Si llega `bot_token` vacío, conserva el actual (permite que el usuario
    // edite solo `default_chat_id` sin tener que reescribir el secreto).
    let existing = state.integrations.telegram();
    let bot_token = if input.bot_token.trim().is_empty() {
        existing.as_ref().map(|c| c.bot_token.clone()).unwrap_or_default()
    } else {
        input.bot_token.trim().to_string()
    };
    if input.enabled && bot_token.is_empty() {
        return Err(ApiError::BadRequest(
            "bot_token requerido para habilitar la integración".into(),
        ));
    }
    let cfg = TelegramConfig {
        bot_token,
        default_chat_id: input.default_chat_id.trim().to_string(),
    };
    integrations::upsert_telegram(&state.ch, &cfg, input.enabled, &admin.email).await?;
    // Refresca el cache de forma síncrona — así el siguiente uso por notify
    // ya tiene el token nuevo sin esperar al tick de 15 s.
    if let Err(e) = state.integrations.reload(&state.ch).await {
        tracing::warn!(error = %e, "falló el reload post-upsert de integraciones");
    }
    Ok(Json(read_telegram(&state).await?))
}

async fn delete_telegram(
    admin: AuthUser,
    State(state): State<SharedState>,
) -> ApiResult<Json<TelegramView>> {
    let cfg = TelegramConfig::default();
    integrations::upsert_telegram(&state.ch, &cfg, false, &admin.email).await?;
    if let Err(e) = state.integrations.reload(&state.ch).await {
        tracing::warn!(error = %e, "falló el reload post-delete de integraciones");
    }
    Ok(Json(read_telegram(&state).await?))
}

async fn test_telegram(
    _admin: AuthUser,
    State(state): State<SharedState>,
    Json(input): Json<TestInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let chat_id = input.chat_id.trim();
    if chat_id.is_empty() {
        return Err(ApiError::BadRequest("chat_id requerido".into()));
    }
    let cfg = state
        .integrations
        .telegram()
        .ok_or_else(|| ApiError::BadRequest("Telegram no está configurado".into()))?;
    if cfg.bot_token.is_empty() {
        return Err(ApiError::BadRequest("Telegram no tiene bot_token".into()));
    }
    let text = if input.text.trim().is_empty() {
        "🧪 Prueba de notificaciones desde <b>Faro</b>".to_string()
    } else {
        input.text
    };
    let url = format!(
        "{}/bot{}/sendMessage",
        state.cfg.telegram_api_base.trim_end_matches('/'),
        cfg.bot_token
    );
    let body = json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "HTML",
        "disable_web_page_preview": true,
    });
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("red: {e}")))?;
    let status = resp.status();
    let response_body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError::BadRequest(format!(
            "Telegram respondió {status}: {response_body}"
        )));
    }
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::mask;

    #[test]
    fn mask_full_token() {
        assert_eq!(mask("123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"), "123456:****ew11");
    }

    #[test]
    fn mask_short_token() {
        assert_eq!(mask("abc"), "****abc");
    }

    #[test]
    fn mask_empty() {
        assert_eq!(mask(""), "");
        assert_eq!(mask("   "), "");
    }
}
