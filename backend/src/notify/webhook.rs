//! Plugin de webhook HTTP genérico.
//!
//! Envía un POST a una URL con un body JSON. Tres modos:
//!
//! 1. **Estructurado** (default cuando no se da `body_template`): el body es
//!    el incidente serializado con campos planos — compatible con receptores
//!    custom y con Slack/Discord incoming webhooks que aceptan JSON arbitrario
//!    (los nuestros lo usaron históricamente así).
//! 2. **Template** (`body_template` no vacío): el string se interpola con
//!    `{rule_name}`, `{severity}`, `{status}`, `{value}`, `{threshold}`,
//!    `{project_id}`, `{started_at}`, `{resolved_at}`, `{text}`. El resultado
//!    se parsea como JSON antes de enviar — si no es JSON válido, el plugin
//!    falla y se loguea (mejor que mandar un body mal formado y recibir 400).
//!
//! Headers extra: `headers` es un map opcional para auth (`Authorization`,
//! `X-API-Key`, etc.) o `Content-Type` custom.

use std::collections::BTreeMap;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{plain_text, Notifier};
use crate::storage::AlertIncidentRow;

pub const KIND: &str = "webhook";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub url: String,
    /// Si está vacío, el body se construye estructurado (ver doc del módulo).
    /// Si no, se trata como template con `{placeholders}`.
    #[serde(default)]
    pub body_template: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

pub struct WebhookNotifier {
    pub config: Config,
}

impl WebhookNotifier {
    pub fn from_json(s: &str) -> Result<Self> {
        let config: Config = serde_json::from_str(s).context("webhook config inválida")?;
        if config.url.trim().is_empty() {
            return Err(anyhow!("webhook.url vacía"));
        }
        Ok(Self { config })
    }

    /// Construye un notifier "inline" para targets `https://...` (compat con
    /// el formato viejo donde la URL iba directo en `notification_targets`).
    pub fn inline(url: String) -> Self {
        Self {
            config: Config {
                url,
                body_template: String::new(),
                headers: BTreeMap::new(),
            },
        }
    }
}

#[async_trait]
impl Notifier for WebhookNotifier {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn dispatch(&self, client: &reqwest::Client, incident: &AlertIncidentRow) -> Result<()> {
        // SSRF: el webhook lo configura un admin y apunta a una URL arbitraria. Igual
        // que los monitores, rechazamos IPs privadas/metadata y hostnames internos sin
        // dominio cualificado para que un target no pueda usar al backend como proxy
        // hacia la red interna (169.254.169.254, clickhouse:8123, etc.). El cliente
        // compartido ya va con `redirect(Policy::none())` para cerrar el salto vía 3xx.
        crate::monitor_url::validate_monitor_url(&self.config.url)
            .map_err(|reason| anyhow!("webhook.url bloqueada por política SSRF: {reason}"))?;
        let body = build_body(&self.config, incident)?;
        let mut req = client.post(&self.config.url).json(&body);
        for (k, v) in &self.config.headers {
            req = req.header(k, v);
        }
        let resp = req.send().await.context("webhook send")?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("webhook respondió {status}: {body_text}"));
        }
        tracing::debug!(target = %self.config.url, "webhook OK");
        Ok(())
    }
}

fn build_body(config: &Config, incident: &AlertIncidentRow) -> Result<serde_json::Value> {
    if config.body_template.trim().is_empty() {
        return Ok(structured_body(incident));
    }
    let interpolated = interpolate(&config.body_template, incident);
    serde_json::from_str(&interpolated)
        .with_context(|| format!("body_template interpolado no es JSON válido: {interpolated}"))
}

pub(crate) fn structured_body(incident: &AlertIncidentRow) -> serde_json::Value {
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

/// Reemplazo de `{placeholders}` simple. No es un template engine completo —
/// no soporta condicionales ni loops. Para casos complejos, los plugins
/// específicos (Slack, Discord) ya tienen su body fijo.
fn interpolate(template: &str, incident: &AlertIncidentRow) -> String {
    let replacements: &[(&str, String)] = &[
        ("{rule_name}", json_escape(&incident.rule_name)),
        ("{severity}", json_escape(&incident.severity)),
        ("{status}", json_escape(&incident.status)),
        ("{value}", incident.value.to_string()),
        ("{threshold}", incident.threshold.to_string()),
        ("{project_id}", json_escape(&incident.project_id)),
        ("{incident_id}", incident.id.to_string()),
        (
            "{started_at}",
            json_escape(
                &incident
                    .started_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ),
        ),
        (
            "{resolved_at}",
            json_escape(
                &incident
                    .resolved_at
                    .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
                    .unwrap_or_default(),
            ),
        ),
        ("{text}", json_escape(&plain_text(incident))),
    ];
    let mut out = template.to_string();
    for (placeholder, value) in replacements {
        out = out.replace(placeholder, value);
    }
    out
}

/// Escapa para uso dentro de un valor JSON string (sin las comillas). El
/// template debe colocar `"{rule_name}"` con las comillas — sólo escapamos
/// el contenido.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn sample() -> AlertIncidentRow {
        AlertIncidentRow {
            id: Uuid::nil(),
            project_id: "p1".into(),
            rule_id: Uuid::nil(),
            rule_name: "rule".into(),
            started_at: Utc::now(),
            resolved_at: None,
            value: 12.5,
            threshold: 10.0,
            severity: "warn".into(),
            status: "firing".into(),
            note: String::new(),
            version: 1,
        }
    }

    #[test]
    fn template_interpolated() {
        let cfg = Config {
            url: "https://example.com".into(),
            body_template: r#"{"text":"{rule_name} {severity}"}"#.into(),
            headers: Default::default(),
        };
        let body = build_body(&cfg, &sample()).unwrap();
        assert_eq!(body["text"], "rule warn");
    }

    #[test]
    fn structured_body_has_expected_fields() {
        let body = structured_body(&sample());
        assert_eq!(body["rule_name"], "rule");
        assert_eq!(body["severity"], "warn");
        assert!(body["text"].as_str().unwrap().contains("rule"));
    }

    #[test]
    fn json_escape_handles_quotes_and_newlines() {
        assert_eq!(json_escape("a\"b\nc"), "a\\\"b\\nc");
    }

    #[test]
    fn from_json_rejects_empty_url() {
        let r = WebhookNotifier::from_json(r#"{"url":""}"#);
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn dispatch_rejects_ssrf_url() {
        // Un webhook a la IP de metadata cloud se rechaza ANTES de cualquier request.
        let n = WebhookNotifier::inline("http://169.254.169.254/latest/meta-data/".into());
        let client = reqwest::Client::new();
        let err = n.dispatch(&client, &sample()).await.unwrap_err();
        assert!(err.to_string().contains("SSRF"), "error real: {err}");
    }
}
