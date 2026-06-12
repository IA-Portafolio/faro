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

use crate::observability::names;
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

/// Resumen del despacho de un incidente a todos sus destinos. Antes el resultado
/// se descartaba con `let _ = dispatch(...)`; devolverlo permite al evaluador
/// loguear/alertar cuando una notificación no salió aunque el incidente quede
/// "firing" en el panel.
#[derive(Debug, Default, Clone, Copy)]
pub struct NotifyOutcome {
    /// Destinos despachados con éxito.
    pub sent: usize,
    /// Destinos cuyo envío falló (webhook caído, token inválido, canal borrado…).
    pub failed: usize,
    /// Targets que no se pudieron parsear (string mal formado / scheme no soportado).
    pub unroutable: usize,
}

/// Etiqueta de baja cardinalidad para la métrica `faro_alert_notify_total`.
fn target_kind(t: &Target) -> &'static str {
    match t {
        Target::Channel(_) => "channel",
        Target::InlineWebhook(_) => "webhook",
        Target::InlineTelegram { .. } => "telegram",
    }
}

/// Dispara una notificación a cada destino configurado. Mantiene firma estable
/// para `workers/alert_evaluator.rs` (salvo el tipo de retorno, ahora un resumen).
/// Cada destino emite `faro_alert_notify_total{kind,outcome}` para que un fallo
/// de entrega sea visible en `/metrics` en vez de tragarse en silencio.
pub async fn dispatch(
    state: &SharedState,
    targets: &[String],
    incident: &AlertIncidentRow,
) -> Result<NotifyOutcome> {
    let mut outcome = NotifyOutcome::default();
    if targets.is_empty() {
        return Ok(outcome);
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    for raw in targets {
        let parsed = match parse_target(raw) {
            Some(p) => p,
            None => {
                outcome.unroutable += 1;
                metrics::counter!(names::ALERT_NOTIFY, "kind" => "unknown", "outcome" => "unroutable")
                    .increment(1);
                tracing::warn!(target = %raw, "target de notificación no reconocido (se ignora)");
                continue;
            }
        };
        let kind = target_kind(&parsed);
        match dispatch_one(state, &client, parsed, incident).await {
            Ok(()) => {
                outcome.sent += 1;
                metrics::counter!(names::ALERT_NOTIFY, "kind" => kind, "outcome" => "sent")
                    .increment(1);
            }
            Err(e) => {
                outcome.failed += 1;
                metrics::counter!(names::ALERT_NOTIFY, "kind" => kind, "outcome" => "failed")
                    .increment(1);
                tracing::warn!(target = %raw, error = %e, "dispatch de notificación falló");
            }
        }
    }
    Ok(outcome)
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

    #[test]
    fn build_from_kind_rejects_unknown_kind() {
        // `Box<dyn Notifier>` no es Debug, así que usamos `.err()` en vez de
        // `.unwrap_err()` (que necesitaría formatear el valor Ok).
        let err = build_from_kind("no-such-kind", "{}")
            .err()
            .expect("un kind desconocido debe devolver Err");
        assert!(err.to_string().contains("desconocido"));
    }

    #[test]
    fn build_from_kind_slack_validates_webhook_url() {
        assert!(build_from_kind(
            "slack",
            r#"{"webhook_url":"https://hooks.slack.com/services/x"}"#
        )
        .is_ok());
        // webhook_url vacío → error (pero NO "desconocido": el kind sí existe).
        let err = build_from_kind("slack", r#"{"webhook_url":""}"#)
            .err()
            .expect("webhook_url vacío debe devolver Err");
        assert!(!err.to_string().contains("desconocido"));
        // JSON inválido → error de parseo.
        assert!(build_from_kind("slack", "no json").is_err());
    }

    #[test]
    fn every_supported_kind_is_routable() {
        // Un kind anunciado en SUPPORTED_KINDS (lo usa el dropdown del frontend)
        // que `build_from_kind` no reconozca sería un bug silencioso. Config "{}"
        // puede fallar por campos requeridos, pero nunca como "kind desconocido".
        for kind in SUPPORTED_KINDS {
            if let Err(e) = build_from_kind(kind, "{}") {
                assert!(
                    !e.to_string().contains("desconocido"),
                    "kind '{kind}' está en SUPPORTED_KINDS pero build_from_kind lo rechaza como desconocido"
                );
            }
        }
    }

    fn sample_incident(status: &str) -> crate::storage::AlertIncidentRow {
        crate::storage::AlertIncidentRow {
            id: uuid::Uuid::nil(),
            project_id: "proj-1".into(),
            rule_id: uuid::Uuid::nil(),
            rule_name: "Errores 5xx".into(),
            started_at: chrono::Utc::now(),
            resolved_at: None,
            value: 42.0,
            threshold: 10.0,
            severity: "error".into(),
            status: status.into(),
            note: String::new(),
            version: 1,
        }
    }

    #[test]
    fn plain_text_firing_mentions_rule_severity_and_direction() {
        let txt = plain_text::plain_text(&sample_incident("firing"));
        assert!(txt.contains("Errores 5xx"));
        assert!(txt.contains("ERROR")); // severidad en mayúsculas
        assert!(txt.contains("firing"));
        assert!(txt.contains("por encima del umbral"));
    }

    #[test]
    fn plain_text_resolved_reads_back_to_normal() {
        let txt = plain_text::plain_text(&sample_incident("resolved"));
        assert!(txt.contains("vuelta a la normalidad"));
    }
}
