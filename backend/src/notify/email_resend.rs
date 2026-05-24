//! Plugin Email vía Resend.
//!
//! Spec: https://resend.com/docs/api-reference/emails/send-email
//!
//! Elegimos Resend en lugar de SMTP para este plugin porque (a) es HTTP plano
//! sin agregar deps al binario, (b) es lo que stacks modernos suelen usar
//! cuando no quieren operar un MTA propio, (c) cumple lo pedido en 2.10
//! ("email vía SMTP/Resend"). Un plugin SMTP separado puede agregarse después
//! sin tocar este — la abstracción `Notifier` lo permite.
//!
//! Auth: header `Authorization: Bearer <api_key>`. El api_key empieza con `re_`.
//!
//! El `from` debe ser un dominio verificado en la cuenta de Resend; usar uno
//! no verificado devuelve 403 con un mensaje claro.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{plain_text, Notifier};
use crate::storage::AlertIncidentRow;

pub const KIND: &str = "email_resend";
const RESEND_URL: &str = "https://api.resend.com/emails";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub api_key: String,
    /// Email remitente. Debe ser de un dominio verificado en Resend.
    pub from: String,
    /// Uno o más destinatarios. Resend acepta hasta 50 por request.
    pub to: Vec<String>,
    /// Sufijo opcional del subject. El subject base es el `plain_text` del
    /// incidente — útil para añadir `[PROD]`, `[Faro]`, etc.
    #[serde(default)]
    pub subject_prefix: String,
}

pub struct EmailResendNotifier {
    pub config: Config,
}

impl EmailResendNotifier {
    pub fn from_json(s: &str) -> Result<Self> {
        let config: Config = serde_json::from_str(s).context("email_resend config inválida")?;
        if config.api_key.trim().is_empty() {
            return Err(anyhow!("email_resend.api_key vacío"));
        }
        if config.from.trim().is_empty() {
            return Err(anyhow!("email_resend.from vacío"));
        }
        if config.to.is_empty() {
            return Err(anyhow!("email_resend.to vacío"));
        }
        Ok(Self { config })
    }
}

#[async_trait]
impl Notifier for EmailResendNotifier {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn dispatch(&self, client: &reqwest::Client, incident: &AlertIncidentRow) -> Result<()> {
        let summary = plain_text(incident);
        let subject = if self.config.subject_prefix.is_empty() {
            summary.clone()
        } else {
            format!("{} {}", self.config.subject_prefix, summary)
        };
        let html = render_html(incident);
        let body = json!({
            "from": self.config.from,
            "to": self.config.to,
            "subject": subject,
            "html": html,
            "text": summary,
        });
        let resp = client
            .post(RESEND_URL)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.config.api_key),
            )
            .json(&body)
            .send()
            .await
            .context("resend send")?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("resend respondió {status}: {body_text}"));
        }
        Ok(())
    }
}

fn render_html(incident: &AlertIncidentRow) -> String {
    let color = match (incident.status.as_str(), incident.severity.as_str()) {
        ("resolved", _) => "#2ecc71",
        (_, "critical") => "#c0392b",
        (_, "error") => "#e74c3c",
        (_, "warn") => "#f39c12",
        _ => "#3498db",
    };
    // HTML minimal e inline-styles para que pase los renderers de mail más
    // estrictos (Gmail/Outlook strippean <style>).
    format!(
        r#"<!doctype html>
<html><body style="font-family: -apple-system, sans-serif; margin: 0; padding: 24px; background: #f6f7f9;">
  <div style="max-width: 560px; margin: 0 auto; background: #fff; border-radius: 8px; overflow: hidden; border: 1px solid #e1e4e8;">
    <div style="background: {color}; color: #fff; padding: 16px 20px;">
      <div style="font-size: 12px; opacity: 0.9; text-transform: uppercase; letter-spacing: 0.5px;">{sev} · {status}</div>
      <div style="font-size: 18px; font-weight: 600; margin-top: 4px;">{name}</div>
    </div>
    <div style="padding: 20px;">
      <table style="width: 100%; border-collapse: collapse; font-size: 14px;">
        <tr><td style="padding: 6px 0; color: #586069;">Valor</td><td style="padding: 6px 0; text-align: right; font-family: monospace;">{value:.4}</td></tr>
        <tr><td style="padding: 6px 0; color: #586069;">Umbral</td><td style="padding: 6px 0; text-align: right; font-family: monospace;">{threshold:.4}</td></tr>
        <tr><td style="padding: 6px 0; color: #586069;">Proyecto</td><td style="padding: 6px 0; text-align: right; font-family: monospace;">{project}</td></tr>
        <tr><td style="padding: 6px 0; color: #586069;">Iniciada</td><td style="padding: 6px 0; text-align: right; font-family: monospace;">{started}</td></tr>
      </table>
    </div>
  </div>
</body></html>"#,
        color = color,
        sev = html_escape(&incident.severity.to_uppercase()),
        status = html_escape(&incident.status),
        name = html_escape(&incident.rule_name),
        value = incident.value,
        threshold = incident.threshold,
        project = html_escape(&incident.project_id),
        started = html_escape(
            &incident
                .started_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        ),
    )
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
