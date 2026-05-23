use anyhow::Result;
use serde_json::json;

use crate::storage::AlertIncidentRow;

/// Fire a notification to each configured target. We use a generic JSON shape that
/// works with Slack incoming webhooks, Discord webhooks, and any custom receiver.
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
            "[{}] {} — {} {} {} (observed {})",
            incident.severity.to_uppercase(),
            incident.rule_name,
            incident.status,
            if incident.status == "firing" { "over threshold" } else { "back to normal" },
            incident.threshold,
            incident.value,
        ),
    });

    let client = reqwest::Client::new();
    for url in targets {
        let res = client.post(url).json(&payload).send().await;
        match res {
            Ok(r) if r.status().is_success() => {
                tracing::info!(target = %url, "alert dispatched");
            }
            Ok(r) => {
                tracing::warn!(target = %url, status = %r.status(), "alert webhook non-2xx");
            }
            Err(e) => {
                tracing::warn!(target = %url, error = %e, "alert webhook failed");
            }
        }
    }
    Ok(())
}
