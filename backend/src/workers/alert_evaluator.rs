use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Deserialize;
use tokio::time::{interval, MissedTickBehavior};
use uuid::Uuid;

use crate::state::SharedState;
use crate::storage::{AlertIncidentRow, AlertRuleRow};

/// Evaluates each alert rule on its own cadence. A rule's `query` must return a
/// single Float64 value, optionally referencing :window_seconds. We append
/// FORMAT JSONEachRow at execution time.
pub fn start_alert_evaluator(state: SharedState) {
    tokio::spawn(async move {
        let mut next_run: HashMap<Uuid, Instant> = HashMap::new();
        let mut active: HashMap<Uuid, AlertIncidentRow> = HashMap::new();

        let mut reload = interval(Duration::from_secs(15));
        reload.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut tick = interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut rules: Vec<AlertRuleRow> = Vec::new();

        loop {
            tokio::select! {
                _ = reload.tick() => {
                    match load_rules(&state).await {
                        Ok(r) => rules = r,
                        Err(e) => tracing::warn!(error = %e, "alert rule reload failed"),
                    }
                }
                _ = tick.tick() => {
                    let now = Instant::now();
                    for rule in &rules {
                        if rule.enabled == 0 || rule.deleted == 1 {
                            continue;
                        }
                        let due = next_run.get(&rule.id).copied().unwrap_or(now);
                        if due > now {
                            continue;
                        }
                        next_run.insert(rule.id, now + Duration::from_secs(rule.interval_seconds as u64));
                        evaluate_rule(state.clone(), rule.clone(), &mut active).await;
                    }
                }
            }
        }
    });
}

async fn load_rules(state: &SharedState) -> anyhow::Result<Vec<AlertRuleRow>> {
    state
        .ch
        .select::<AlertRuleRow>(
            "SELECT id, project_id, name, description, source, query, condition, threshold, \
             window_seconds, interval_seconds, severity, notification_targets, enabled, \
             created_at, updated_at, deleted, version \
             FROM faro.alert_rules FINAL WHERE deleted = 0",
        )
        .await
}

#[derive(Deserialize)]
struct ScalarRow {
    #[serde(default)]
    value: Option<f64>,
}

async fn evaluate_rule(
    state: SharedState,
    rule: AlertRuleRow,
    active: &mut HashMap<Uuid, AlertIncidentRow>,
) {
    let query = rule
        .query
        .replace(":window_seconds", &rule.window_seconds.to_string());
    let sql = format!("SELECT toFloat64({query}) AS value");

    let value = match state.ch.select_one::<ScalarRow>(&sql).await {
        Ok(Some(s)) => s.value.unwrap_or(0.0),
        Ok(None) => 0.0,
        Err(e) => {
            tracing::warn!(rule = %rule.name, error = %e, "alert query failed");
            return;
        }
    };

    let fired = match rule.condition.as_str() {
        "gt" => value > rule.threshold,
        "gte" => value >= rule.threshold,
        "lt" => value < rule.threshold,
        "lte" => value <= rule.threshold,
        "eq" => (value - rule.threshold).abs() < f64::EPSILON,
        _ => {
            tracing::warn!(condition = %rule.condition, "unknown condition operator");
            return;
        }
    };

    let now = Utc::now();
    if fired {
        if !active.contains_key(&rule.id) {
            let incident = AlertIncidentRow {
                id: Uuid::new_v4(),
                project_id: rule.project_id.clone(),
                rule_id: rule.id,
                rule_name: rule.name.clone(),
                started_at: now,
                resolved_at: None,
                value,
                threshold: rule.threshold,
                severity: rule.severity.clone(),
                status: "firing".into(),
                note: String::new(),
                version: 1,
            };
            if let Err(e) = state.ch.insert("faro.alert_incidents", &[incident.clone()]).await {
                tracing::error!(error = %e, "incident insert failed");
            }
            active.insert(rule.id, incident.clone());
            let _ = crate::notify::dispatch(&rule.notification_targets, &incident).await;
            tracing::warn!(rule = %rule.name, value, threshold = rule.threshold, "alert firing");
        }
    } else if let Some(mut incident) = active.remove(&rule.id) {
        incident.resolved_at = Some(now);
        incident.status = "resolved".into();
        incident.version = incident.version.saturating_add(1);
        if let Err(e) = state.ch.insert("faro.alert_incidents", &[incident.clone()]).await {
            tracing::error!(error = %e, "incident resolve insert failed");
        }
        let _ = crate::notify::dispatch(&rule.notification_targets, &incident).await;
        tracing::info!(rule = %rule.name, "alert resolved");
    }
}
