//! API REST para configurar integraciones externas desde el dashboard.
//!
//! Endpoints:
//!   GET    /api/v1/integrations/telegram                 → estado (token enmascarado)
//!   PUT    /api/v1/integrations/telegram                 → upsert config Telegram global
//!   DELETE /api/v1/integrations/telegram                 → desactiva y vacía el token
//!   POST   /api/v1/integrations/telegram/test            → mensaje de prueba
//!
//!   GET    /api/v1/integrations/channels                 → lista todos los canales
//!   POST   /api/v1/integrations/channels                 → crea uno nuevo
//!   GET    /api/v1/integrations/channels/:id             → uno
//!   PUT    /api/v1/integrations/channels/:id             → upsert
//!   DELETE /api/v1/integrations/channels/:id             → soft delete (deleted=1)
//!   POST   /api/v1/integrations/channels/:id/test        → notificación de prueba
//!   GET    /api/v1/integrations/channels/kinds           → kinds soportados (para el form)

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::{AdminUser, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::integrations::{self, TelegramConfig};
use crate::notification_channels;
use crate::notify;
use crate::state::SharedState;
use crate::storage::{AlertIncidentRow, NotificationChannelRow};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route(
            "/integrations/telegram",
            get(get_telegram).put(put_telegram).delete(delete_telegram),
        )
        .route("/integrations/telegram/test", post(test_telegram))
        .route(
            "/integrations/channels",
            get(list_channels).post(create_channel),
        )
        .route("/integrations/channels/kinds", get(list_kinds))
        .route(
            "/integrations/channels/{id}",
            get(get_channel).put(put_channel).delete(delete_channel),
        )
        .route("/integrations/channels/{id}/test", post(test_channel))
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

fn default_true() -> bool {
    true
}

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
    let last4: String = tail
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
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
    _admin: AdminUser,
    State(state): State<SharedState>,
) -> ApiResult<Json<TelegramView>> {
    Ok(Json(read_telegram(&state).await?))
}

async fn put_telegram(
    admin: AdminUser,
    State(state): State<SharedState>,
    Json(input): Json<TelegramInput>,
) -> ApiResult<Json<TelegramView>> {
    // Si llega `bot_token` vacío, conserva el actual (permite que el usuario
    // edite solo `default_chat_id` sin tener que reescribir el secreto).
    let existing = state.integrations.telegram();
    let bot_token = if input.bot_token.trim().is_empty() {
        existing
            .as_ref()
            .map(|c| c.bot_token.clone())
            .unwrap_or_default()
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
    admin: AdminUser,
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
    _admin: AdminUser,
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

// ============================================================
// Notification channels (multi-instancia, plugin Notifier)
// ============================================================

#[derive(Serialize)]
struct ChannelView {
    id: String,
    name: String,
    kind: String,
    enabled: bool,
    /// Config con secretos enmascarados (`bot_token`, `api_key`, `integration_key`,
    /// `webhook_url`). Para edición, el frontend envía vacío en estos campos para
    /// "conservar el valor actual".
    config: Value,
    created_at: String,
    updated_at: String,
    updated_by: String,
}

#[derive(Deserialize)]
struct ChannelInput {
    /// Sólo en POST. En PUT viene del path.
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: String,
    kind: String,
    #[serde(default = "default_true")]
    enabled: bool,
    /// JSON de config del kind. Si algún campo "secreto" llega vacío y existe
    /// versión guardada, se conserva el valor previo (igual que con Telegram).
    config: Value,
}

#[derive(Deserialize)]
struct ChannelTestInput {
    /// Texto opcional sobreescribiendo el body del incident de prueba.
    #[serde(default)]
    note: String,
}

async fn list_kinds(_admin: AdminUser) -> Json<Value> {
    Json(json!({ "kinds": notify::SUPPORTED_KINDS }))
}

async fn list_channels(
    _admin: AdminUser,
    State(state): State<SharedState>,
) -> ApiResult<Json<Vec<ChannelView>>> {
    let rows = notification_channels::list_all(&state.ch).await?;
    Ok(Json(rows.into_iter().map(row_to_view).collect()))
}

async fn get_channel(
    _admin: AdminUser,
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ChannelView>> {
    let row = notification_channels::read_one(&state.ch, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(row_to_view(row)))
}

async fn create_channel(
    admin: AdminUser,
    State(state): State<SharedState>,
    Json(input): Json<ChannelInput>,
) -> ApiResult<Json<ChannelView>> {
    let id = input
        .id
        .clone()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            slugify(&input.name)
                .unwrap_or_else(|| format!("ch-{}", &Uuid::new_v4().simple().to_string()[..8]))
        });
    validate_id(&id)?;
    // Rechaza si ya existe (POST = create estricto).
    if notification_channels::read_one(&state.ch, &id)
        .await?
        .is_some()
    {
        return Err(ApiError::BadRequest(format!(
            "ya existe un canal con id '{id}'"
        )));
    }
    upsert_channel(&state, &admin, id, input).await
}

async fn put_channel(
    admin: AdminUser,
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(input): Json<ChannelInput>,
) -> ApiResult<Json<ChannelView>> {
    validate_id(&id)?;
    upsert_channel(&state, &admin, id, input).await
}

async fn delete_channel(
    admin: AdminUser,
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    notification_channels::soft_delete(&state.ch, &id, &admin.email)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    if let Err(e) = state.notification_channels.reload(&state.ch).await {
        tracing::warn!(error = %e, "falló reload post-delete de notification_channels");
    }
    Ok(Json(json!({ "deleted": id })))
}

async fn test_channel(
    _admin: AdminUser,
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(input): Json<ChannelTestInput>,
) -> ApiResult<Json<Value>> {
    // Lee desde DB (no cache) para poder probar canales recién creados o
    // disabled que todavía no están en el cache.
    let row = notification_channels::read_one(&state.ch, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let notifier = notify::build_from_kind(&row.kind, &row.config)
        .map_err(|e| ApiError::BadRequest(format!("config inválida: {e}")))?;
    let incident = test_incident(&id, &input.note);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    notifier
        .dispatch(&client, &incident)
        .await
        .map_err(|e| ApiError::BadRequest(format!("envío falló: {e}")))?;
    Ok(Json(json!({ "ok": true, "kind": row.kind })))
}

// -------------------- Helpers internos --------------------

async fn upsert_channel(
    state: &SharedState,
    admin: &AuthUser,
    id: String,
    input: ChannelInput,
) -> ApiResult<Json<ChannelView>> {
    if input.kind.trim().is_empty() {
        return Err(ApiError::BadRequest("kind requerido".into()));
    }
    if !notify::SUPPORTED_KINDS.contains(&input.kind.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "kind '{}' no soportado",
            input.kind
        )));
    }

    // Si el caller dejó secretos vacíos y ya hay una versión previa, los
    // mergeamos para no obligar a re-tipear el token cada vez.
    let existing = notification_channels::read_one(&state.ch, &id).await?;
    let merged_config = merge_secrets(&input.kind, &input.config, existing.as_ref())?;

    // Valida el JSON intentando construir el Notifier — falla rápido si el
    // schema no es correcto, antes de persistir.
    let merged_config_str = serde_json::to_string(&merged_config)
        .map_err(|e| ApiError::Internal(format!("serializando config: {e}")))?;
    notify::build_from_kind(&input.kind, &merged_config_str)
        .map_err(|e| ApiError::BadRequest(format!("config inválida: {e}")))?;

    let now = Utc::now();
    let created_at = existing.as_ref().map(|e| e.created_at).unwrap_or(now);
    let row = NotificationChannelRow {
        id: id.clone(),
        name: input.name.trim().to_string(),
        kind: input.kind.clone(),
        enabled: if input.enabled { 1 } else { 0 },
        config: merged_config_str,
        created_at,
        updated_at: now,
        updated_by: admin.email.clone(),
        deleted: 0,
        version: now.timestamp_millis() as u64,
    };
    let saved = notification_channels::upsert(&state.ch, row, &admin.email)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    if let Err(e) = state.notification_channels.reload(&state.ch).await {
        tracing::warn!(error = %e, "falló reload post-upsert de notification_channels");
    }
    Ok(Json(row_to_view(saved)))
}

fn row_to_view(row: NotificationChannelRow) -> ChannelView {
    let masked_config = mask_secrets(&row.kind, &row.config);
    ChannelView {
        id: row.id,
        name: row.name,
        kind: row.kind,
        enabled: row.enabled == 1,
        config: masked_config,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
        updated_by: row.updated_by,
    }
}

/// Lista de claves consideradas secretas por kind. Al devolver al frontend las
/// enmascaramos; al recibir un PUT vacío en esas claves, conservamos la previa.
fn secret_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "telegram" => &["bot_token"],
        "webhook" => &["url"], // la URL puede contener tokens en el path/query
        "slack" => &["webhook_url"],
        "discord" => &["webhook_url"],
        "pagerduty" => &["integration_key"],
        "opsgenie" => &["api_key"],
        "email_resend" => &["api_key"],
        _ => &[],
    }
}

fn mask_secrets(kind: &str, config_str: &str) -> Value {
    let mut v: Value = serde_json::from_str(config_str).unwrap_or(json!({}));
    if let Value::Object(map) = &mut v {
        for key in secret_keys(kind) {
            if let Some(entry) = map.get_mut(*key) {
                if let Some(s) = entry.as_str() {
                    *entry = json!(mask_value(s));
                }
            }
        }
    }
    v
}

/// Igual a `mask` para Telegram pero genérico: muestra los últimos 4 caracteres.
/// Para URLs, mantiene la base hasta el path para que el operador reconozca
/// el destino sin filtrar el token entero.
fn mask_value(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        // Conserva esquema + host, oculta path/query.
        if let Some(end) = s.find("://").map(|i| i + 3) {
            if let Some(slash) = s[end..].find('/') {
                return format!("{}{}", &s[..end + slash + 1], "****");
            }
        }
        return s.to_string();
    }
    let last4: String = s
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if let Some((head, _)) = s.split_once(':') {
        format!("{head}:****{last4}")
    } else {
        format!("****{last4}")
    }
}

fn merge_secrets(
    kind: &str,
    incoming: &Value,
    existing: Option<&NotificationChannelRow>,
) -> ApiResult<Value> {
    let mut merged = incoming.clone();
    let Some(existing) = existing else {
        return Ok(merged);
    };
    let prev: Value = match serde_json::from_str(&existing.config) {
        Ok(v) => v,
        Err(_) => return Ok(merged),
    };
    let (Value::Object(merged_map), Value::Object(prev_map)) = (&mut merged, &prev) else {
        return Ok(merged);
    };
    for key in secret_keys(kind) {
        let incoming_empty = matches!(merged_map.get(*key), Some(Value::String(s)) if s.is_empty())
            || merged_map.get(*key).is_none();
        if incoming_empty {
            if let Some(prev_val) = prev_map.get(*key) {
                merged_map.insert((*key).into(), prev_val.clone());
            }
        }
    }
    Ok(merged)
}

fn test_incident(channel_id: &str, note: &str) -> AlertIncidentRow {
    let now = Utc::now();
    AlertIncidentRow {
        id: Uuid::new_v4(),
        project_id: "test".into(),
        rule_id: Uuid::nil(),
        rule_name: format!("Prueba de canal '{channel_id}'"),
        started_at: now,
        resolved_at: None,
        value: 1.0,
        threshold: 0.0,
        severity: "warn".into(),
        status: "firing".into(),
        note: if note.is_empty() {
            "Notificación de prueba enviada desde Faro · Settings → Integraciones".into()
        } else {
            note.to_string()
        },
        version: now.timestamp_millis() as u64,
    }
}

/// Acepta `[a-z0-9-]+`, 1..64. Estrecho a propósito — los ids van en URLs y en
/// targets `channel://<id>`, no queremos sorpresas con encoding.
fn validate_id(id: &str) -> ApiResult<()> {
    if id.is_empty() || id.len() > 64 {
        return Err(ApiError::BadRequest(
            "id debe tener entre 1 y 64 chars".into(),
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError::BadRequest("id sólo acepta [a-z0-9-]".into()));
    }
    Ok(())
}

/// Best-effort: si el `name` no produce un slug válido (e.g. sólo caracteres
/// no-ASCII), devuelve None y el caller cae al UUID-derived.
fn slugify(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let s: String = lower
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' => c,
            _ => '-',
        })
        .collect();
    let s: String = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() || s.len() > 64 {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::mask;

    #[test]
    fn mask_full_token() {
        assert_eq!(
            mask("123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"),
            "123456:****ew11"
        );
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
