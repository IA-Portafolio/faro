//! Detector automático de rollback para feature flags.
//!
//! Une los tres mundos que Faro ya tiene:
//! `$feature_exposure` en `product_events`, trazas en product events posteriores,
//! y `error_events` ligados por `trace_id`. Si la variante B tiene >=N veces la
//! tasa de errores de A, abre un incidente `feature-rollback:*`.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;
use tokio::time::{interval, MissedTickBehavior};
use uuid::Uuid;

use crate::state::SharedState;
use crate::storage::AlertIncidentRow;

const RULE_NAME_PREFIX: &str = "feature-rollback:";
const FEATURE_ROLLBACK_NAMESPACE: Uuid =
    Uuid::from_u128(0x6661726f_66656174_726f6c6c_6261636b);

#[derive(Debug, Deserialize)]
struct FlagErrorRow {
    project_id: String,
    flag_key: String,
    sample_a: u64,
    sample_b: u64,
    errors_a: u64,
    errors_b: u64,
    #[serde(default)]
    top_service: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RatioState {
    Insufficient,
    Ready(f64),
}

pub fn start_feature_rollback_detector(state: SharedState) {
    if !state.cfg.feature_rollback_enabled {
        tracing::info!(
            "detector feature rollback deshabilitado (FARO_FEATURE_ROLLBACK_ENABLED=false)"
        );
        return;
    }

    let interval_secs = state.cfg.feature_rollback_interval_secs.max(30);
    tracing::info!(
        every_secs = interval_secs,
        window_min = state.cfg.feature_rollback_window_minutes,
        ratio = state.cfg.feature_rollback_ratio,
        "arrancando detector feature rollback"
    );

    tokio::spawn(async move {
        let mut active: HashMap<Uuid, AlertIncidentRow> = HashMap::new();
        match load_active_incidents(&state).await {
            Ok(rows) => {
                for row in rows {
                    active.insert(row.rule_id, row);
                }
                tracing::info!(
                    active = active.len(),
                    "incidentes feature rollback recuperados al arrancar"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "no se pudieron recuperar incidentes feature rollback");
            }
        }

        tokio::time::sleep(Duration::from_secs(30)).await;

        let mut tick = interval(Duration::from_secs(interval_secs));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.tick().await;

        loop {
            tick.tick().await;
            if let Err(e) = evaluate(&state, &mut active).await {
                tracing::warn!(error = %e, "evaluación feature rollback falló");
            }
        }
    });
}

async fn load_active_incidents(state: &SharedState) -> anyhow::Result<Vec<AlertIncidentRow>> {
    state
        .ch
        .select::<AlertIncidentRow>(
            "SELECT id, project_id, rule_id, rule_name, started_at, resolved_at, \
                    value, threshold, severity, status, note, version \
             FROM faro.alert_incidents FINAL \
             WHERE status = 'firing' AND startsWith(rule_name, 'feature-rollback:') LIMIT 1000",
        )
        .await
}

async fn evaluate(
    state: &SharedState,
    active: &mut HashMap<Uuid, AlertIncidentRow>,
) -> anyhow::Result<()> {
    let window_secs = state.cfg.feature_rollback_window_minutes.max(1) * 60;
    let sql = build_query(window_secs);
    let rows: Vec<FlagErrorRow> = state.ch.select(&sql).await?;

    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let rule_id = feature_rule_id(&row.project_id, &row.flag_key);
        seen.insert(rule_id);

        let ratio = error_ratio(
            row.sample_a,
            row.sample_b,
            row.errors_a,
            row.errors_b,
            state.cfg.feature_rollback_min_sample,
            state.cfg.feature_rollback_min_treatment_errors,
        );
        let firing = matches!(
            ratio,
            RatioState::Ready(v) if v >= state.cfg.feature_rollback_ratio
        );
        let should_resolve = match ratio {
            RatioState::Insufficient => true,
            RatioState::Ready(v) => v <= state.cfg.feature_rollback_resolve_ratio,
        };

        if firing && !active.contains_key(&rule_id) {
            fire(state, active, rule_id, &row, ratio).await;
        } else if active.contains_key(&rule_id) && should_resolve {
            resolve(state, active, rule_id).await;
        }
    }

    // Si una flag desaparece del query, ya no tiene señal suficiente reciente.
    let stale: Vec<Uuid> = active
        .keys()
        .copied()
        .filter(|id| !seen.contains(id))
        .collect();
    for rule_id in stale {
        resolve(state, active, rule_id).await;
    }

    Ok(())
}

async fn fire(
    state: &SharedState,
    active: &mut HashMap<Uuid, AlertIncidentRow>,
    rule_id: Uuid,
    row: &FlagErrorRow,
    ratio: RatioState,
) {
    let now = Utc::now();
    let treatment_rate = rate(row.errors_b, row.sample_b);
    let control_rate = rate(row.errors_a, row.sample_a);
    let incident_value = ratio_value(ratio, state.cfg.feature_rollback_ratio);
    let ratio_label = ratio_label(ratio, state.cfg.feature_rollback_ratio);
    let top = if row.top_service.trim().is_empty() {
        "desconocido"
    } else {
        row.top_service.as_str()
    };
    let note = format!(
        "Rollback recomendado: flag {} tiene {}x más errores en variante B \
         (B {}/{} = {:.2}%, A {}/{} = {:.2}%). Top servicio: {}.",
        row.flag_key,
        ratio_label,
        row.errors_b,
        row.sample_b,
        treatment_rate * 100.0,
        row.errors_a,
        row.sample_a,
        control_rate * 100.0,
        top,
    );

    let incident = AlertIncidentRow {
        id: Uuid::new_v4(),
        project_id: row.project_id.clone(),
        rule_id,
        rule_name: format!("{}{}:{}", RULE_NAME_PREFIX, row.project_id, row.flag_key),
        started_at: now,
        resolved_at: None,
        value: incident_value,
        threshold: state.cfg.feature_rollback_ratio,
        severity: "critical".into(),
        status: "firing".into(),
        note,
        version: incident_version(now, 1),
    };

    if let Err(e) = state
        .ch
        .insert("faro.alert_incidents", &[incident.clone()])
        .await
    {
        tracing::error!(error = %e, "no se pudo persistir incidente feature rollback");
        return;
    }

    active.insert(rule_id, incident);
    tracing::warn!(
        project = %row.project_id,
        flag = %row.flag_key,
        ratio = incident_value,
        "rollback recomendado por errores ligados a feature flag"
    );
}

async fn resolve(state: &SharedState, active: &mut HashMap<Uuid, AlertIncidentRow>, rule_id: Uuid) {
    let Some(mut incident) = active.remove(&rule_id) else {
        return;
    };
    let now = Utc::now();
    incident.resolved_at = Some(now);
    incident.status = "resolved".into();
    incident.version = incident.version.saturating_add(1);
    if let Err(e) = state
        .ch
        .insert("faro.alert_incidents", &[incident.clone()])
        .await
    {
        tracing::error!(error = %e, "no se pudo persistir resolución feature rollback");
        return;
    }
    tracing::info!(rule = %incident.rule_name, "feature rollback resuelto");
}

fn rate(errors: u64, sample: u64) -> f64 {
    if sample == 0 {
        0.0
    } else {
        errors as f64 / sample as f64
    }
}

fn ratio_value(ratio: RatioState, fallback: f64) -> f64 {
    match ratio {
        RatioState::Insufficient => 0.0,
        RatioState::Ready(v) if v.is_finite() => v,
        RatioState::Ready(_) => fallback,
    }
}

fn ratio_label(ratio: RatioState, fallback: f64) -> String {
    match ratio {
        RatioState::Insufficient => "0.0".into(),
        RatioState::Ready(v) if v.is_finite() => format!("{v:.1}"),
        RatioState::Ready(_) => format!(">{fallback:.1}"),
    }
}

fn error_ratio(
    sample_a: u64,
    sample_b: u64,
    errors_a: u64,
    errors_b: u64,
    min_sample: u64,
    min_treatment_errors: u64,
) -> RatioState {
    if sample_a < min_sample || sample_b < min_sample || errors_b < min_treatment_errors {
        return RatioState::Insufficient;
    }
    if errors_a == 0 {
        return RatioState::Ready(f64::INFINITY);
    }
    RatioState::Ready(rate(errors_b, sample_b) / rate(errors_a, sample_a))
}

fn feature_rule_id(project: &str, flag_key: &str) -> Uuid {
    let name = format!("{}{}:{}", RULE_NAME_PREFIX, project, flag_key);
    Uuid::new_v5(&FEATURE_ROLLBACK_NAMESPACE, name.as_bytes())
}

fn incident_version(now: chrono::DateTime<Utc>, offset: u64) -> u64 {
    (now.timestamp_millis() as u64).saturating_add(offset)
}

fn build_query(window_secs: u32) -> String {
    format!(
        "WITH \
           exposures AS ( \
             SELECT project_id, \
                    JSONExtractString(properties, 'flag_key') AS flag_key, \
                    distinct_id, \
                    argMin(JSONExtractString(properties, 'variant'), timestamp) AS variant, \
                    min(timestamp) AS exposed_at \
             FROM faro.product_events \
             WHERE timestamp >= now() - INTERVAL {window_secs} SECOND \
               AND timestamp < now() \
               AND event_name = '$feature_exposure' \
               AND JSONExtractString(properties, 'variant') IN ('A', 'B') \
               AND JSONExtractString(properties, 'flag_key') != '' \
             GROUP BY project_id, flag_key, distinct_id \
           ), \
           samples AS ( \
             SELECT project_id, flag_key, \
                    toUInt64(uniqExactIf(tuple(project_id, distinct_id), variant = 'A')) AS sample_a, \
                    toUInt64(uniqExactIf(tuple(project_id, distinct_id), variant = 'B')) AS sample_b \
             FROM exposures \
             GROUP BY project_id, flag_key \
           ), \
           traced_actions AS ( \
             SELECT DISTINCT e.project_id AS project_id, e.flag_key AS flag_key, \
                    e.variant AS variant, e.distinct_id AS distinct_id, pe.trace_id AS trace_id \
             FROM exposures AS e \
             INNER JOIN faro.product_events AS pe \
               ON pe.project_id = e.project_id AND pe.distinct_id = e.distinct_id \
             WHERE pe.timestamp >= e.exposed_at \
               AND pe.timestamp >= now() - INTERVAL {window_secs} SECOND \
               AND pe.timestamp < now() \
               AND pe.trace_id != '' \
           ), \
           linked_errors AS ( \
             SELECT ta.project_id AS project_id, ta.flag_key AS flag_key, ta.variant AS variant, \
                    er.log_id AS log_id, er.service_name AS service_name \
             FROM traced_actions AS ta \
             INNER JOIN faro.error_events AS er \
               ON er.project_id = ta.project_id AND er.trace_id = ta.trace_id \
             WHERE er.timestamp >= now() - INTERVAL {window_secs} SECOND \
               AND er.timestamp < now() \
           ), \
           errors AS ( \
             SELECT project_id, flag_key, \
                    toUInt64(uniqExactIf(log_id, variant = 'A')) AS errors_a, \
                    toUInt64(uniqExactIf(log_id, variant = 'B')) AS errors_b, \
                    arrayElement(topKIf(1)(service_name, variant = 'B'), 1) AS top_service \
             FROM linked_errors \
             GROUP BY project_id, flag_key \
           ) \
         SELECT s.project_id AS project_id, s.flag_key AS flag_key, \
                s.sample_a AS sample_a, s.sample_b AS sample_b, \
                ifNull(e.errors_a, 0) AS errors_a, ifNull(e.errors_b, 0) AS errors_b, \
                ifNull(e.top_service, '') AS top_service \
         FROM samples AS s \
         LEFT JOIN errors AS e USING (project_id, flag_key) \
         WHERE s.sample_a > 0 AND s.sample_b > 0 \
         LIMIT 500"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_requires_sample_and_treatment_errors() {
        assert_eq!(error_ratio(19, 100, 1, 10, 20, 5), RatioState::Insufficient);
        assert_eq!(error_ratio(100, 100, 1, 4, 20, 5), RatioState::Insufficient);
    }

    #[test]
    fn ratio_handles_zero_control_as_infinite() {
        let ratio = error_ratio(100, 100, 0, 5, 20, 5);
        assert_eq!(ratio, RatioState::Ready(f64::INFINITY));
        assert_eq!(ratio_value(ratio, 5.0), 5.0);
        assert_eq!(ratio_label(ratio, 5.0), ">5.0");
    }

    #[test]
    fn ratio_compares_error_rates_not_raw_counts() {
        let RatioState::Ready(ratio) = error_ratio(1_000, 500, 10, 30, 20, 5) else {
            panic!("expected ready ratio");
        };
        assert!((ratio - 6.0).abs() < 1e-9);
    }

    #[test]
    fn rule_id_is_deterministic_per_project_and_flag() {
        let a = feature_rule_id("p1", "new-checkout");
        let b = feature_rule_id("p1", "new-checkout");
        let c = feature_rule_id("p2", "new-checkout");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn query_links_exposures_to_errors_by_trace() {
        let q = build_query(900);
        assert!(q.contains("event_name = '$feature_exposure'"));
        assert!(q.contains("pe.trace_id != ''"));
        assert!(q.contains("faro.error_events AS er"));
        assert!(q.contains("er.trace_id = ta.trace_id"));
        assert!(q.contains("uniqExactIf(log_id, variant = 'B')"));
    }
}
