//! Plugin Telegram. Bot API + parse_mode=HTML.
//!
//! Hay DOS formas de usar este plugin:
//!   - Como entry de `notification_channels` (kind=telegram, config con bot_token
//!     y chat_id ya en el JSON). Útil para tener múltiples chats objetivo con
//!     bots distintos sin compartir token global.
//!   - Vía targets `tg://<chat_id>` inline, donde el módulo padre construye la
//!     config resolviendo el token desde la integración global o env var.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::Notifier;
use crate::storage::AlertIncidentRow;

pub const KIND: &str = "telegram";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub bot_token: String,
    pub chat_id: String,
    /// Default `https://api.telegram.org`. Configurable sólo para tests; en
    /// el path inline (sin channel) lo inyecta el módulo padre desde
    /// `Config::telegram_api_base`.
    #[serde(default = "default_api_base")]
    pub api_base: String,
}

fn default_api_base() -> String {
    "https://api.telegram.org".into()
}

pub struct TelegramNotifier {
    pub config: Config,
}

impl TelegramNotifier {
    pub fn from_json(s: &str) -> Result<Self> {
        let mut config: Config = serde_json::from_str(s).context("telegram config inválida")?;
        if config.bot_token.trim().is_empty() {
            return Err(anyhow!("telegram.bot_token vacío"));
        }
        if config.chat_id.trim().is_empty() {
            return Err(anyhow!("telegram.chat_id vacío"));
        }
        if config.api_base.trim().is_empty() {
            config.api_base = default_api_base();
        }
        Ok(Self { config })
    }
}

#[async_trait]
impl Notifier for TelegramNotifier {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn dispatch(&self, client: &reqwest::Client, incident: &AlertIncidentRow) -> Result<()> {
        let url = format!(
            "{}/bot{}/sendMessage",
            self.config.api_base.trim_end_matches('/'),
            self.config.bot_token
        );
        let body = json!({
            "chat_id": self.config.chat_id,
            "text": telegram_text(incident),
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        });
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("telegram send")?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "telegram sendMessage respondió {status}: {body_text}"
            ));
        }
        tracing::debug!(chat_id = %self.config.chat_id, "telegram OK");
        Ok(())
    }
}

/// Mensaje formateado en HTML para Telegram. Telegram acepta un subset
/// pequeño: `<b>`, `<i>`, `<code>`, `<pre>`, `<a>`. Escapamos `<>&` en los
/// campos dinámicos.
pub(crate) fn telegram_text(incident: &AlertIncidentRow) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_handles_specials() {
        assert_eq!(escape_html("a<b>&c"), "a&lt;b&gt;&amp;c");
    }

    #[test]
    fn from_json_rejects_empty_fields() {
        assert!(TelegramNotifier::from_json(r#"{"bot_token":"","chat_id":"-1"}"#).is_err());
        assert!(TelegramNotifier::from_json(r#"{"bot_token":"x:y","chat_id":""}"#).is_err());
    }
}
