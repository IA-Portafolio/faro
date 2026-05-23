use anyhow::Result;
use serde_json::json;

use crate::config::Config;
use crate::storage::AlertIncidentRow;

/// Destino de notificación resuelto. Cada string crudo en `notification_targets`
/// se parsea a una de estas variantes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// Webhook genérico: POST JSON con la forma usada históricamente.
    /// Compatible con Slack incoming webhooks, Discord webhooks y receptores propios.
    Webhook(String),
    /// Telegram nativo vía Bot API. `chat_id` puede ser numérico ("-1001234567890")
    /// o un @canal (Telegram acepta ambos en `chat_id`).
    Telegram { chat_id: String, token: Option<String> },
}

/// Parsea un target crudo. Acepta:
///   - `http(s)://...` → Webhook
///   - `tg://<chat_id>` → Telegram con token global
///   - `tg://<chat_id>@<bot_token>` → Telegram con token explícito
///   - `telegram://...` como alias de `tg://`
fn parse_target(raw: &str) -> Option<Target> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(rest) = raw
        .strip_prefix("tg://")
        .or_else(|| raw.strip_prefix("telegram://"))
    {
        let (chat_id, token) = match rest.rsplit_once('@') {
            Some((chat, tok)) if !tok.is_empty() => (chat, Some(tok.to_string())),
            _ => (rest, None),
        };
        let chat_id = chat_id.trim().trim_end_matches('/').to_string();
        if chat_id.is_empty() {
            return None;
        }
        return Some(Target::Telegram { chat_id, token });
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(Target::Webhook(raw.to_string()));
    }
    None
}

/// Dispara una notificación a cada destino configurado. Soporta webhooks JSON
/// (Slack/Discord/custom) y Telegram nativo vía Bot API.
pub async fn dispatch(
    cfg: &Config,
    targets: &[String],
    incident: &AlertIncidentRow,
) -> Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    let client = reqwest::Client::new();
    let webhook_payload = webhook_payload(incident);
    let tg_text = telegram_text(incident);

    for raw in targets {
        match parse_target(raw) {
            Some(Target::Webhook(url)) => {
                send_webhook(&client, &url, &webhook_payload).await;
            }
            Some(Target::Telegram { chat_id, token }) => {
                let token = token.as_deref().or(cfg.telegram_bot_token.as_deref());
                match token {
                    Some(token) => {
                        send_telegram(&client, &cfg.telegram_api_base, token, &chat_id, &tg_text)
                            .await;
                    }
                    None => {
                        tracing::warn!(
                            target = %raw,
                            "target Telegram sin token: configura TELEGRAM_BOT_TOKEN o usa tg://<chat>@<token>"
                        );
                    }
                }
            }
            None => {
                tracing::warn!(target = %raw, "target de notificación no reconocido (se ignora)");
            }
        }
    }
    Ok(())
}

fn webhook_payload(incident: &AlertIncidentRow) -> serde_json::Value {
    json!({
        "rule_name": incident.rule_name,
        "severity": incident.severity,
        "status": incident.status,
        "value": incident.value,
        "threshold": incident.threshold,
        "started_at": incident.started_at,
        "resolved_at": incident.resolved_at,
        "incident_id": incident.id,
        "project_id": incident.project_id,
        "text": plain_text(incident),
    })
}

fn plain_text(incident: &AlertIncidentRow) -> String {
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

/// Mensaje formateado en HTML para Telegram (parse_mode=HTML). Telegram acepta
/// un subset pequeño: <b>, <i>, <code>, <pre>, <a>. Escapamos `<>&` en los
/// campos dinámicos.
fn telegram_text(incident: &AlertIncidentRow) -> String {
    let emoji = match (incident.status.as_str(), incident.severity.as_str()) {
        ("resolved", _) => "✅",
        (_, "critical") => "🚨",
        (_, "error") => "🔴",
        (_, "warn") => "⚠️",
        _ => "ℹ️",
    };
    let status_label = if incident.status == "firing" {
        "ACTIVA"
    } else {
        "RESUELTA"
    };
    let started = incident
        .started_at
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let resolved_line = incident
        .resolved_at
        .map(|t| {
            format!(
                "\n<b>Resuelta:</b> <code>{}</code>",
                escape_html(&t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            )
        })
        .unwrap_or_default();
    format!(
        "{emoji} <b>{status} · {sev}</b>\n<b>{name}</b>\n<b>Valor:</b> <code>{value:.4}</code> (umbral <code>{threshold:.4}</code>)\n<b>Proyecto:</b> <code>{project}</code>\n<b>Iniciada:</b> <code>{started}</code>{resolved_line}",
        status = status_label,
        sev = escape_html(&incident.severity.to_uppercase()),
        name = escape_html(&incident.rule_name),
        value = incident.value,
        threshold = incident.threshold,
        project = escape_html(&incident.project_id),
        started = escape_html(&started),
    )
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

async fn send_webhook(client: &reqwest::Client, url: &str, payload: &serde_json::Value) {
    match client.post(url).json(payload).send().await {
        Ok(r) if r.status().is_success() => {
            tracing::info!(target = %url, "alerta despachada (webhook)");
        }
        Ok(r) => {
            tracing::warn!(target = %url, status = %r.status(), "webhook de alerta no-2xx");
        }
        Err(e) => {
            tracing::warn!(target = %url, error = %e, "webhook de alerta falló");
        }
    }
}

async fn send_telegram(
    client: &reqwest::Client,
    api_base: &str,
    token: &str,
    chat_id: &str,
    text: &str,
) {
    let url = format!("{}/bot{}/sendMessage", api_base.trim_end_matches('/'), token);
    let body = json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "HTML",
        "disable_web_page_preview": true,
    });
    match client.post(&url).json(&body).send().await {
        Ok(r) if r.status().is_success() => {
            tracing::info!(chat_id = %chat_id, "alerta despachada (telegram)");
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            tracing::warn!(
                chat_id = %chat_id,
                %status,
                response = %body,
                "telegram sendMessage no-2xx"
            );
        }
        Err(e) => {
            tracing::warn!(chat_id = %chat_id, error = %e, "telegram sendMessage falló");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_webhook() {
        assert_eq!(
            parse_target("https://discord.com/api/webhooks/123/abc"),
            Some(Target::Webhook(
                "https://discord.com/api/webhooks/123/abc".into()
            ))
        );
    }

    #[test]
    fn parses_telegram_with_global_token() {
        assert_eq!(
            parse_target("tg://-1001234567890"),
            Some(Target::Telegram {
                chat_id: "-1001234567890".into(),
                token: None,
            })
        );
    }

    #[test]
    fn parses_telegram_with_inline_token() {
        assert_eq!(
            parse_target("tg://-1001234567890@123456:ABCDEF"),
            Some(Target::Telegram {
                chat_id: "-1001234567890".into(),
                token: Some("123456:ABCDEF".into()),
            })
        );
    }

    #[test]
    fn parses_telegram_alias() {
        assert_eq!(
            parse_target("telegram://@mychannel"),
            Some(Target::Telegram {
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
    }

    #[test]
    fn escapes_html_specials() {
        assert_eq!(escape_html("a<b>&c"), "a&lt;b&gt;&amp;c");
    }
}
