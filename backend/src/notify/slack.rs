//! Plugin Slack Incoming Webhooks.
//!
//! Slack acepta POST JSON con `text` (markdown ligero) y opcionalmente `attachments`
//! o `blocks`. Para empezar simple usamos `text` con formato mrkdwn — los
//! incidentes resueltos llevan ✅ y los firing llevan 🔴/🚨/⚠️ según severidad.
//!
//! Diferencia con el plugin `webhook` genérico: aquí construimos el body con
//! la forma que Slack renderiza bonito (`*bold*`, `_italic_`, `` `code` ``).
//! Si lo único que quieres es POST JSON crudo, usa `kind=webhook`.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::Notifier;
use crate::storage::AlertIncidentRow;

pub const KIND: &str = "slack";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// URL del incoming webhook (https://hooks.slack.com/services/...).
    pub webhook_url: String,
    /// Canal opcional para override (Slack toma el del webhook por defecto).
    /// Algunos workspaces lo ignoran si el webhook está restringido a un canal.
    #[serde(default)]
    pub channel: String,
    /// Username opcional con el que aparece el mensaje.
    #[serde(default)]
    pub username: String,
}

pub struct SlackNotifier {
    pub config: Config,
}

impl SlackNotifier {
    pub fn from_json(s: &str) -> Result<Self> {
        let config: Config = serde_json::from_str(s).context("slack config inválida")?;
        if config.webhook_url.trim().is_empty() {
            return Err(anyhow!("slack.webhook_url vacío"));
        }
        Ok(Self { config })
    }
}

#[async_trait]
impl Notifier for SlackNotifier {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn dispatch(&self, client: &reqwest::Client, incident: &AlertIncidentRow) -> Result<()> {
        let emoji = match (incident.status.as_str(), incident.severity.as_str()) {
            ("resolved", _) => ":white_check_mark:",
            (_, "critical") => ":rotating_light:",
            (_, "error") => ":red_circle:",
            (_, "warn") => ":warning:",
            _ => ":information_source:",
        };
        let text = format!(
            "{emoji} *{name}* — {status} `{sev}`\n• Valor: `{value:.4}` (umbral `{threshold:.4}`)\n• Proyecto: `{project}`",
            name = incident.rule_name,
            status = incident.status,
            sev = incident.severity,
            value = incident.value,
            threshold = incident.threshold,
            project = incident.project_id,
        );
        let mut body = json!({
            "text": text,
            "mrkdwn": true,
        });
        if !self.config.channel.is_empty() {
            body["channel"] = json!(self.config.channel);
        }
        if !self.config.username.is_empty() {
            body["username"] = json!(self.config.username);
        }
        let resp = client
            .post(&self.config.webhook_url)
            .json(&body)
            .send()
            .await
            .context("slack send")?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("slack respondió {status}: {body_text}"));
        }
        Ok(())
    }
}
