//! Despacho de notificaciones de alertas.
//!
//! Arquitectura: trait `Notifier` + plugins. Cada plugin (Telegram, webhook
//! genérico, Slack, Discord, PagerDuty, OpsGenie, email Resend) implementa
//! [`Notifier::dispatch`] consumiendo un [`AlertIncidentRow`].
//!
//! Los destinos vienen como strings dentro de `alert_rules.notification_targets`.
//! Formatos soportados:
//!
//!  - `channel://<id>` — lookup en `notification_channels` (multi-instancia,
//!    configurada desde Settings → Integraciones).
//!  - `https://...` / `http://...` — webhook genérico inline POST JSON (compat
//!    con Slack/Discord incoming webhooks y receptores custom).
//!  - `tg://<chat_id>` / `tg://<chat_id>@<bot_token>` — Telegram nativo
//!    (resuelve token vía channel, integración global de DB, o env var).
//!  - `telegram://...` — alias de `tg://`.
//!
//! El dispatcher itera los targets en serie (no N concurrentes) — son pocos
//! por incidente, y serializar evita penalizar al endpoint más lento sin
//! ganancia real en p99.

use anyhow::Result;
use async_trait::async_trait;

use crate::state::SharedState;
use crate::storage::AlertIncidentRow;

pub mod discord;
pub mod email_resend;
pub mod opsgenie;
pub mod pagerduty;
pub mod slack;
pub mod telegram;
pub mod webhook;

pub use plain_text::plain_text;

/// Contrato común para todos los plugins. La instancia se construye **por
/// despacho** desde la fila de `notification_channels` (o equivalente legacy);
/// no se cachean instancias entre invocaciones porque deserializar el config
/// JSON es barato y mantener el contrato sin estado evita problemas de hot-reload.
#[async_trait]
pub trait Notifier: Send + Sync {
    /// Identificador human-readable usado en logs ("telegram", "pagerduty", ...).
    fn kind(&self) -> &'static str;

    /// Despacha el incidente. Cualquier error se loguea pero no aborta el
    /// dispatch de otros targets del mismo incidente.
    async fn dispatch(&self, client: &reqwest::Client, incident: &AlertIncidentRow) -> Result<()>;
}

/// Construye un Notifier dinámicamente desde una fila de `notification_channels`.
/// Devuelve error si el `kind` no se reconoce o si el JSON de config no parsea.
pub fn build_from_kind(kind: &str, config_json: &str) -> Result<Box<dyn Notifier>> {
    match kind {
        webhook::KIND => Ok(Box::new(webhook::WebhookNotifier::from_json(config_json)?)),
        slack::KIND => Ok(Box::new(slack::SlackNotifier::from_json(config_json)?)),
        discord::KIND => Ok(Box::new(discord::DiscordNotifier::from_json(config_json)?)),
        pagerduty::KIND => Ok(Box::new(pagerduty::PagerDutyNotifier::from_json(
            config_json,
        )?)),
        opsgenie::KIND => Ok(Box::new(opsgenie::OpsGenieNotifier::from_json(
            config_json,
        )?)),
        email_resend::KIND => Ok(Box::new(email_resend::EmailResendNotifier::from_json(
            config_json,
        )?)),
        telegram::KIND => Ok(Box::new(telegram::TelegramNotifier::from_json(
            config_json,
        )?)),
        other => Err(anyhow::anyhow!("kind de notifier desconocido: {other}")),
    }
}

/// Lista de kinds soportados. Útil para validar input en la API y rellenar el
/// dropdown del frontend sin que el frontend tenga que conocerlos hardcoded.
pub const SUPPORTED_KINDS: &[&str] = &[
    webhook::KIND,
    slack::KIND,
    discord::KIND,
    pagerduty::KIND,
    opsgenie::KIND,
    email_resend::KIND,
    telegram::KIND,
];

/// Target parseado a partir del string crudo.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// `channel://<id>` — lookup en `notification_channels`.
    Channel(String),
    /// `https://...` o `http://...` — webhook inline (compat).
    InlineWebhook(String),
    /// `tg://<chat_id>` o `tg://<chat_id>@<token>` (compat).
    InlineTelegram {
        chat_id: String,
        token: Option<String>,
    },
}

fn parse_target(raw: &str) -> Option<Target> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(id) = raw.strip_prefix("channel://") {
        let id = id.trim().trim_end_matches('/');
        if id.is_empty() {
            return None;
        }
        return Some(Target::Channel(id.to_string()));
    }
    if let Some(rest) = raw
        .strip_prefix("tg://")
        .or_else(|| raw.strip_prefix("telegram://"))
    {
        // Token bot Telegram = `<id>:<base64>` — el `:` distingue un token
        // explícito de un `@canal` (que nunca contiene `:`).
        let (chat_id, token) = match rest.rsplit_once('@') {
            Some((chat, tok)) if !chat.is_empty() && tok.contains(':') => {
                (chat, Some(tok.to_string()))
            }
            _ => (rest, None),
        };
        let chat_id = chat_id.trim().trim_end_matches('/').to_string();
        if chat_id.is_empty() {
            return None;
        }
        return Some(Target::InlineTelegram { chat_id, token });
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(Target::InlineWebhook(raw.to_string()));
    }
    None
}

/// Dispara una notificación a cada destino configurado. Mantiene firma estable
/// para `workers/alert_evaluator.rs`.
pub async fn dispatch(
    state: &SharedState,
    targets: &[String],
    incident: &AlertIncidentRow,
) -> Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    for raw in targets {
        let parsed = match parse_target(raw) {
            Some(p) => p,
            None => {
                tracing::warn!(target = %raw, "target de notificación no reconocido (se ignora)");
                continue;
            }
        };
        let result = dispatch_one(state, &client, parsed, incident).await;
        if let Err(e) = result {
            tracing::warn!(target = %raw, error = %e, "dispatch de notificación falló");
        }
    }
    Ok(())
}

async fn dispatch_one(
    state: &SharedState,
    client: &reqwest::Client,
    target: Target,
    incident: &AlertIncidentRow,
) -> Result<()> {
    match target {
        Target::Channel(id) => {
            let channel = state
                .notification_channels
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("channel '{id}' no existe o está deshabilitado"))?;
            let notifier = build_from_kind(&channel.kind, &channel.config)?;
            tracing::debug!(channel = %id, kind = notifier.kind(), "despachando notificación");
            notifier.dispatch(client, incident).await
        }
        Target::InlineWebhook(url) => {
            let notifier = webhook::WebhookNotifier::inline(url);
            notifier.dispatch(client, incident).await
        }
        Target::InlineTelegram { chat_id, token } => {
            // Resolución del token, en orden:
            //   1. Token explícito en el target.
            //   2. Integración Telegram global (faro.integrations, kind=telegram).
            //   3. Env var TELEGRAM_BOT_TOKEN.
            let resolved = token
                .or_else(|| {
                    state
                        .integrations
                        .telegram()
                        .filter(|c| !c.bot_token.is_empty())
                        .map(|c| c.bot_token)
                })
                .or_else(|| state.cfg.telegram_bot_token.clone());
            let token = resolved.ok_or_else(|| {
                anyhow::anyhow!(
                    "target Telegram sin token: configura la integración global o usa tg://<chat>@<token>"
                )
            })?;
            let cfg = telegram::Config {
                bot_token: token,
                chat_id,
                api_base: state.cfg.telegram_api_base.clone(),
            };
            let notifier = telegram::TelegramNotifier { config: cfg };
            notifier.dispatch(client, incident).await
        }
    }
}

// Helpers compartidos por plugins ----------

mod plain_text {
    use crate::storage::AlertIncidentRow;

    /// Texto plano corto, sirve como fallback / body de plugins que no formatean
    /// (PagerDuty `summary`, OpsGenie `message`, email subject).
    pub fn plain_text(incident: &AlertIncidentRow) -> String {
        let direction = if incident.status == "firing" {
            "por encima del umbral"
        } else {
            "vuelta a la normalidad"
        };
        format!(
            "[{}] {} — {} {} {} (observado {})",
            incident.severity.to_uppercase(),
            incident.rule_name,
            incident.status,
            direction,
            incident.threshold,
            incident.value,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channel() {
        assert_eq!(
            parse_target("channel://ops-pagerduty"),
            Some(Target::Channel("ops-pagerduty".into()))
        );
    }

    #[test]
    fn parses_https_webhook() {
        assert_eq!(
            parse_target("https://discord.com/api/webhooks/123/abc"),
            Some(Target::InlineWebhook(
                "https://discord.com/api/webhooks/123/abc".into()
            ))
        );
    }

    #[test]
    fn parses_telegram_with_global_token() {
        assert_eq!(
            parse_target("tg://-1001234567890"),
            Some(Target::InlineTelegram {
                chat_id: "-1001234567890".into(),
                token: None,
            })
        );
    }

    #[test]
    fn parses_telegram_with_inline_token() {
        assert_eq!(
            parse_target("tg://-1001234567890@123456:ABCDEF"),
            Some(Target::InlineTelegram {
                chat_id: "-1001234567890".into(),
                token: Some("123456:ABCDEF".into()),
            })
        );
    }

    #[test]
    fn parses_telegram_alias() {
        assert_eq!(
            parse_target("telegram://@mychannel"),
            Some(Target::InlineTelegram {
                chat_id: "@mychannel".into(),
                token: None,
            })
        );
    }

    #[test]
    fn rejects_unknown_scheme() {
        assert!(parse_target("mailto:a@b.com").is_none());
        assert!(parse_target("").is_none());
        assert!(parse_target("   ").is_none());
        assert!(parse_target("tg://").is_none());
        assert!(parse_target("channel://").is_none());
    }
}
