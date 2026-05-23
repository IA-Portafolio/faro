use anyhow::Result;
use serde_json::json;

use crate::storage::AlertIncidentRow;

/// Dispara una notificación a cada destino configurado. Usamos una forma JSON genérica
/// que funciona con webhooks entrantes de Slack, webhooks de Discord y cualquier receptor
/// personalizado.
pub async fn dispatch(targets: &[String], incident: &AlertIncidentRow) -> Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    let payload = json!({
        "rule_name": incident.rule_name,
        "severity": incident.severity,
        "status": incident.status,
        "value": incident.value,
        "threshold": incident.threshold,
        "started_at": incident.started_at,
        "resolved_at": incident.resolved_at,
        "incident_id": incident.id,
        "text": format!(
            "[{}] {} — {} {} {} (observado {})",
            incident.severity.to_uppercase(),
            incident.rule_name,
            incident.status,
            if incident.status == "firing" { "por encima del umbral" } else { "vuelta a la normalidad" },
            incident.threshold,
            incident.value,
        ),
    });

    let client = reqwest::Client::new();
    for url in targets {
        let res = client.post(url).json(&payload).send().await;
        match res {
            Ok(r) if r.status().is_success() => {
                tracing::info!(target = %url, "alerta despachada");
            }
            Ok(r) => {
                tracing::warn!(target = %url, status = %r.status(), "webhook de alerta no-2xx");
            }
            Err(e) => {
                tracing::warn!(target = %url, error = %e, "webhook de alerta falló");
            }
        }
    }
    Ok(())
}
