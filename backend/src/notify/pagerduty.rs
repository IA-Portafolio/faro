//! Plugin PagerDuty Events API v2.
//!
//! Spec: https://developer.pagerduty.com/docs/events-api-v2/overview/
//!
//! PagerDuty distingue `trigger` (abre incidente o lo updatea si dedup_key matchea)
//! de `resolve` (cierra el incidente con esa dedup_key). Mapeo:
//!   - incident.status = "firing"   → event_action = "trigger"
//!   - incident.status = "resolved" → event_action = "resolve"
//!
//! `dedup_key` debe ser estable por regla (no por incidente) para que múltiples
//! disparos de la misma regla no abran N incidentes en PD. Usamos
//! `<project_id>:<rule_id>` que es lo que ya identifica la regla en faro.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{plain_text, Notifier};
use crate::storage::AlertIncidentRow;

pub const KIND: &str = "pagerduty";
const EVENTS_URL: &str = "https://events.pagerduty.com/v2/enqueue";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Integration Key (a.k.a. routing key) del Events API v2. PagerDuty lo
    /// llama "service integration key" en la UI del servicio; lo emite cuando
    /// añades una "Events API v2" integration a un service.
    pub integration_key: String,
}

pub struct PagerDutyNotifier {
    pub config: Config,
}

impl PagerDutyNotifier {
    pub fn from_json(s: &str) -> Result<Self> {
        let config: Config = serde_json::from_str(s).context("pagerduty config inválida")?;
        if config.integration_key.trim().is_empty() {
            return Err(anyhow!("pagerduty.integration_key vacío"));
        }
        Ok(Self { config })
    }
}

#[async_trait]
impl Notifier for PagerDutyNotifier {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn dispatch(&self, client: &reqwest::Client, incident: &AlertIncidentRow) -> Result<()> {
        let event_action = if incident.status == "resolved" {
            "resolve"
        } else {
            "trigger"
        };
        let severity = match incident.severity.as_str() {
            "critical" => "critical",
            "error" => "error",
            "warn" | "warning" => "warning",
            _ => "info",
        };
        let dedup_key = format!("{}:{}", incident.project_id, incident.rule_id);
        let body = json!({
            "routing_key": self.config.integration_key,
            "event_action": event_action,
            "dedup_key": dedup_key,
            "payload": {
                "summary": plain_text(incident),
                "severity": severity,
                "source": format!("faro/{}", incident.project_id),
                "component": incident.rule_name,
                "custom_details": {
                    "value": incident.value,
                    "threshold": incident.threshold,
                    "status": incident.status,
                    "started_at": incident.started_at,
                    "resolved_at": incident.resolved_at,
                    "note": incident.note,
                }
            }
        });
        let resp = client
            .post(EVENTS_URL)
            .json(&body)
            .send()
            .await
            .context("pagerduty send")?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("pagerduty respondió {status}: {body_text}"));
        }
        Ok(())
    }
}
