//! Plugin Discord Webhooks.
//!
//! Discord acepta POST JSON con `content` (markdown) y/o `embeds`. Usamos
//! `embeds` para que el color cambie según severidad y el campo `timestamp`
//! quede ordenado en el cliente. `username` y `avatar_url` permiten que el
//! mensaje se vea como "Faro" en lugar del bot default del webhook.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::Notifier;
use crate::storage::AlertIncidentRow;

pub const KIND: &str = "discord";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// URL del webhook Discord (https://discord.com/api/webhooks/...).
    pub webhook_url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub avatar_url: String,
}

pub struct DiscordNotifier {
    pub config: Config,
}

impl DiscordNotifier {
    pub fn from_json(s: &str) -> Result<Self> {
        let config: Config = serde_json::from_str(s).context("discord config inválida")?;
        if config.webhook_url.trim().is_empty() {
            return Err(anyhow!("discord.webhook_url vacío"));
        }
        Ok(Self { config })
    }
}

#[async_trait]
impl Notifier for DiscordNotifier {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn dispatch(&self, client: &reqwest::Client, incident: &AlertIncidentRow) -> Result<()> {
        // Colores Discord en decimal (no hex). Verde = resolved, rojo/naranja según severidad.
        let color = match (incident.status.as_str(), incident.severity.as_str()) {
            ("resolved", _) => 0x2ecc71, // verde
            (_, "critical") => 0xc0392b, // rojo intenso
            (_, "error") => 0xe74c3c,    // rojo
            (_, "warn") => 0xf39c12,     // ámbar
            _ => 0x3498db,               // azul info
        };
        let title = format!(
            "[{}] {}",
            incident.severity.to_uppercase(),
            incident.rule_name
        );
        let description = format!(
            "**Status:** {}\n**Valor:** `{:.4}` (umbral `{:.4}`)\n**Proyecto:** `{}`",
            incident.status, incident.value, incident.threshold, incident.project_id,
        );
        let mut body = json!({
            "embeds": [{
                "title": title,
                "description": description,
                "color": color,
                "timestamp": incident.started_at,
            }]
        });
        if !self.config.username.is_empty() {
            body["username"] = json!(self.config.username);
        }
        if !self.config.avatar_url.is_empty() {
            body["avatar_url"] = json!(self.config.avatar_url);
        }
        let resp = client
            .post(&self.config.webhook_url)
            .json(&body)
            .send()
            .await
            .context("discord send")?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("discord respondió {status}: {body_text}"));
        }
        Ok(())
    }
}
