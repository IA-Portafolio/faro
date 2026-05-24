//! Plugin OpsGenie Alert API v2.
//!
//! Spec: https://docs.opsgenie.com/docs/alert-api
//!
//! Auth: header `Authorization: GenieKey <api_key>` (NO Bearer). El api_key se
//! emite desde Settings → API key management.
//!
//! Diferencia con PagerDuty: OpsGenie usa `alias` como dedup key. Si una alert
//! con ese alias ya existe, una nueva request `create` la deja igual (idempotente),
//! y para cerrarla hay que llamar a `/alerts/<alias>/close` con identifierType=alias.
//!
//! Endpoints:
//!   POST /v2/alerts                              (firing)
//!   POST /v2/alerts/<alias>/close?identifierType=alias  (resolved)

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{plain_text, Notifier};
use crate::storage::AlertIncidentRow;

pub const KIND: &str = "opsgenie";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub api_key: String,
    /// Base URL del endpoint. Default `https://api.opsgenie.com` (US).
    /// Para cuentas EU: `https://api.eu.opsgenie.com`.
    #[serde(default = "default_api_base")]
    pub api_base: String,
    /// Equipos/usuarios/escalations a notificar. Si vacío, OpsGenie usa las
    /// reglas de routing del account.
    #[serde(default)]
    pub responders: Vec<String>,
    /// Tags adicionales (se agregan a los del incidente).
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_api_base() -> String {
    "https://api.opsgenie.com".into()
}

pub struct OpsGenieNotifier {
    pub config: Config,
}

impl OpsGenieNotifier {
    pub fn from_json(s: &str) -> Result<Self> {
        let mut config: Config = serde_json::from_str(s).context("opsgenie config inválida")?;
        if config.api_key.trim().is_empty() {
            return Err(anyhow!("opsgenie.api_key vacío"));
        }
        if config.api_base.trim().is_empty() {
            config.api_base = default_api_base();
        }
        Ok(Self { config })
    }
}

#[async_trait]
impl Notifier for OpsGenieNotifier {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn dispatch(&self, client: &reqwest::Client, incident: &AlertIncidentRow) -> Result<()> {
        let alias = format!("{}:{}", incident.project_id, incident.rule_id);
        let base = self.config.api_base.trim_end_matches('/');
        let auth = format!("GenieKey {}", self.config.api_key);

        let resp = if incident.status == "resolved" {
            let url = format!("{base}/v2/alerts/{alias}/close?identifierType=alias");
            let body = json!({ "source": "faro" });
            client
                .post(url)
                .header(reqwest::header::AUTHORIZATION, auth)
                .json(&body)
                .send()
                .await
                .context("opsgenie close")?
        } else {
            let priority = match incident.severity.as_str() {
                "critical" => "P1",
                "error" => "P2",
                "warn" | "warning" => "P3",
                _ => "P4",
            };
            let mut tags = self.config.tags.clone();
            tags.push(format!("project:{}", incident.project_id));
            tags.push(format!("severity:{}", incident.severity));
            let responders: Vec<_> = self
                .config
                .responders
                .iter()
                .map(|r| {
                    // Heurística: si parece UUID o id, lo tratamos como id; si no,
                    // como name (team/user). OpsGenie soporta ambos.
                    let kind = if r.len() == 36 && r.matches('-').count() == 4 {
                        "id"
                    } else if r.contains('@') {
                        "user"
                    } else {
                        "team"
                    };
                    json!({ kind: r, "type": kind })
                })
                .collect();
            let url = format!("{base}/v2/alerts");
            let body = json!({
                "message": format!("[{}] {}", incident.severity.to_uppercase(), incident.rule_name),
                "alias": alias,
                "description": plain_text(incident),
                "priority": priority,
                "source": "faro",
                "tags": tags,
                "responders": responders,
                "details": {
                    "value": incident.value.to_string(),
                    "threshold": incident.threshold.to_string(),
                    "project_id": incident.project_id,
                    "started_at": incident.started_at.to_rfc3339(),
                }
            });
            client
                .post(url)
                .header(reqwest::header::AUTHORIZATION, auth)
                .json(&body)
                .send()
                .await
                .context("opsgenie create")?
        };
        let status = resp.status();
        if !status.is_success() && status.as_u16() != 202 {
            // OpsGenie devuelve 202 Accepted en éxito (procesa async).
            let body_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("opsgenie respondió {status}: {body_text}"));
        }
        Ok(())
    }
}
