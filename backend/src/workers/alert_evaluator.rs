//! Worker que evalúa las reglas de alerta y abre/cierra incidentes.
//!
//! Cada regla corre en su propia cadencia: su `query` debe devolver un único
//! Float64 (opcionalmente usando `:window_seconds`). Si cruza el umbral, se abre
//! un incidente y se notifica; al normalizarse, se cierra.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Deserialize;
use tokio::time::{interval, MissedTickBehavior};
use uuid::Uuid;

use crate::state::SharedState;
use crate::storage::{AlertIncidentRow, AlertRuleRow};

/// Evalúa cada regla de alerta en su propia cadencia. El `query` de una regla debe devolver
/// un único valor Float64, opcionalmente referenciando :window_seconds. Añadimos
/// FORMAT JSONEachRow en tiempo de ejecución.
pub fn start_alert_evaluator(state: SharedState) {
    tokio::spawn(async move {
        let mut next_run: HashMap<Uuid, Instant> = HashMap::new();
        // Estado en memoria: rule_id → incidente `firing`. Repoblado desde la
        // tabla al arrancar para SOBREVIVIR a restarts (cada deploy reinicia el
        // proceso). Sin esto, tras un restart `active` arranca vacío y: (a) cada
        // regla aún disparada re-inserta un incidente nuevo y RE-NOTIFICA (spam),
        // y (b) las reglas que se normalizaron durante el downtime nunca se
        // cierran (incidentes zombi). Mismo patrón que `anomaly_detector`.
        let mut active: HashMap<Uuid, AlertIncidentRow> = HashMap::new();
        match load_active_incidents(&state).await {
            Ok(rows) => {
                let n = rows.len();
                for row in rows {
                    active.insert(row.rule_id, row);
                }
                tracing::info!(active = n, "incidentes de alerta recuperados al arrancar");
            }
            Err(e) => {
                tracing::warn!(error = %e, "no se pudieron recuperar incidentes de alerta activos — empezamos en blanco");
            }
        }

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
                        Err(e) => {
                            tracing::warn!(error = %e, "falló el reload de reglas de alerta");
                            metrics::counter!(crate::observability::names::WORKER_ERRORS, "worker" => "alert_evaluator").increment(1);
                        }
                    }
                }
                _ = tick.tick() => {
                    metrics::counter!(crate::observability::names::WORKER_RUNS, "worker" => "alert_evaluator").increment(1);
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

/// Recarga los incidentes `firing` de reglas de alerta (NO los del
/// `anomaly_detector`, que tienen su propio prefijo `anomaly:` y los gestiona
/// aquel worker) para repoblar el set `active` tras un restart. `FINAL` sobre el
/// ReplacingMergeTree da el último estado por incidente.
async fn load_active_incidents(state: &SharedState) -> anyhow::Result<Vec<AlertIncidentRow>> {
    state
        .ch
        .select::<AlertIncidentRow>(
            "SELECT id, project_id, rule_id, rule_name, started_at, resolved_at, \
                    value, threshold, severity, status, note, version \
             FROM faro.alert_incidents FINAL \
             WHERE status = 'firing' AND NOT startsWith(rule_name, 'anomaly:') LIMIT 1000",
        )
        .await
}

/// Loguea el resultado del dispatch de una notificación. Antes el resultado se
/// descartaba (`let _ = ...`): un webhook caído o un token inválido se tragaba
/// en silencio mientras el panel marcaba "firing". Ahora un fallo de entrega
/// queda en logs (y en `faro_alert_notify_total{outcome="failed"}`).
fn log_dispatch(rule_name: &str, res: anyhow::Result<crate::notify::NotifyOutcome>) {
    match res {
        Ok(o) if o.failed > 0 || o.unroutable > 0 => {
            tracing::warn!(
                rule = %rule_name,
                sent = o.sent,
                failed = o.failed,
                unroutable = o.unroutable,
                "algunos destinos de notificación no recibieron la alerta"
            );
        }
        Ok(_) => {}
        Err(e) => tracing::error!(
            rule = %rule_name,
            error = %e,
            "el dispatch de notificación falló por completo"
        ),
    }
}

#[derive(Deserialize)]
struct ScalarRow {
    #[serde(default)]
    value: Option<f64>,
}

/// Evalúa una única regla contra el state actual. `active` mantiene los
/// incidentes en estado `firing` por `rule_id`; el dedup se basa en su contenido
/// (si ya hay una entrada para esta regla, no inserta un nuevo incidente).
///
/// Pub para que los integration tests de `tests/workers_alert_evaluator.rs`
/// puedan invocarla directamente sin spawnear el loop completo.
pub async fn evaluate_rule(
    state: SharedState,
    rule: AlertRuleRow,
    active: &mut HashMap<Uuid, AlertIncidentRow>,
) {
    // Defensa en profundidad: omitir reglas con queries inseguras (SSRF/RCE vía
    // table-functions de red/fichero) aunque hayan quedado persistidas antes del
    // gate de creación en `api::alerts`. Ver `crate::alert_query`.
    if let Err(reason) = crate::alert_query::validate_alert_query(&rule.query) {
        tracing::warn!(
            rule = %rule.name,
            reason,
            "regla de alerta con query insegura — se omite la evaluación"
        );
        return;
    }
    let query = rule
        .query
        .replace(":window_seconds", &rule.window_seconds.to_string());
    // `SETTINGS` acota el blast radius de cualquier query de alerta: corta a 15s
    // de ejecución y 1 fila de resultado.
    let sql = format!(
        "SELECT toFloat64({query}) AS value SETTINGS max_execution_time = 15, max_result_rows = 1"
    );

    let value = match state.ch.select_one::<ScalarRow>(&sql).await {
        Ok(Some(s)) => s.value.unwrap_or(0.0),
        Ok(None) => 0.0,
        Err(e) => {
            tracing::warn!(rule = %rule.name, error = %e, "falló el query de alerta");
            metrics::counter!(crate::observability::names::WORKER_ERRORS, "worker" => "alert_evaluator").increment(1);
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
            tracing::warn!(condition = %rule.condition, "operador de condición desconocido");
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
            if let Err(e) = state
                .ch
                .insert("faro.alert_incidents", &[incident.clone()])
                .await
            {
                tracing::error!(error = %e, "falló el insert del incidente");
                metrics::counter!(crate::observability::names::WORKER_ERRORS, "worker" => "alert_evaluator").increment(1);
            }
            active.insert(rule.id, incident.clone());
            log_dispatch(
                &rule.name,
                crate::notify::dispatch(&state, &rule.notification_targets, &incident).await,
            );
            tracing::warn!(rule = %rule.name, value, threshold = rule.threshold, "alerta disparada");
        }
    } else if let Some(mut incident) = active.remove(&rule.id) {
        incident.resolved_at = Some(now);
        incident.status = "resolved".into();
        incident.version = incident.version.saturating_add(1);
        if let Err(e) = state
            .ch
            .insert("faro.alert_incidents", &[incident.clone()])
            .await
        {
            tracing::error!(error = %e, "falló el insert de resolución del incidente");
            metrics::counter!(crate::observability::names::WORKER_ERRORS, "worker" => "alert_evaluator").increment(1);
        }
        log_dispatch(
            &rule.name,
            crate::notify::dispatch(&state, &rule.notification_targets, &incident).await,
        );
        tracing::info!(rule = %rule.name, "alerta resuelta");
    }
}
