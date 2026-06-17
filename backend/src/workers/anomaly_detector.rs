//! Detector de anomalías por z-score, sin ML.
//!
//! Cada `anomaly_interval_secs`, compara la tasa actual de tres señales
//! — errores, p95 de latencia y volumen de logs — contra muestras del *mismo
//! slot de hora* en los últimos 7 días. Si el z-score supera el umbral, abre
//! un incidente en `faro.alert_incidents`; cuando vuelve por debajo del umbral
//! de resolución (más bajo que el de disparo, para evitar flapping), lo cierra.
//!
//! Por qué `error_events` no: la tabla `error_events` se rellena en async desde
//! el bus de logs, así que su última ventana puede estar atrasada respecto a
//! `faro.logs`. Para errores leemos directamente de `faro.logs` filtrando por
//! `severity_number >= 17`, que es el mismo predicado que usa el indexer.
//!
//! Estado: el conjunto de incidentes activos vive en memoria (`HashMap<Uuid, ...>`).
//! En startup, repoblamos consultando incidentes con `status='firing'` cuyos
//! `rule_name` empiezan por `anomaly:` — así un restart no abandona alertas
//! abiertas ni dispara duplicados.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;
use tokio::time::{interval, MissedTickBehavior};
use uuid::Uuid;

use crate::state::SharedState;
use crate::storage::AlertIncidentRow;

/// Namespace UUID v5 estable. Cambiarlo invalidaría todos los `rule_id` de
/// anomalías ya persistidos — no tocar salvo para una migración explícita.
const ANOMALY_NAMESPACE: Uuid = Uuid::from_u128(0x6661726f_616e6f6d_616c7900_00000001);

const RULE_NAME_PREFIX: &str = "anomaly:";

#[derive(Clone, Copy, Debug)]
enum Signal {
    Errors,
    P95Latency,
    LogVolume,
}

impl Signal {
    fn slug(self) -> &'static str {
        match self {
            Signal::Errors => "errors",
            Signal::P95Latency => "p95_latency",
            Signal::LogVolume => "log_volume",
        }
    }

    fn human(self) -> &'static str {
        match self {
            Signal::Errors => "errores",
            Signal::P95Latency => "p95 latencia",
            Signal::LogVolume => "volumen de logs",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Signal::Errors => "eventos",
            Signal::P95Latency => "ms",
            Signal::LogVolume => "logs",
        }
    }
}

#[derive(Deserialize)]
struct SampleRow {
    project_id: String,
    service_name: String,
    current: f64,
    s1: f64,
    s2: f64,
    s3: f64,
    s4: f64,
    s5: f64,
    s6: f64,
    s7: f64,
}

pub fn start_anomaly_detector(state: SharedState) {
    if !state.cfg.anomaly_enabled {
        tracing::info!("detector de anomalías deshabilitado (FARO_ANOMALY_ENABLED=false)");
        return;
    }

    let interval_secs = state.cfg.anomaly_interval_secs.max(30);
    tracing::info!(
        every_secs = interval_secs,
        window_min = state.cfg.anomaly_window_minutes,
        z_fire = state.cfg.anomaly_z_fire,
        z_resolve = state.cfg.anomaly_z_resolve,
        "arrancando detector de anomalías"
    );

    tokio::spawn(async move {
        // Estado en memoria: rule_id → incidente. Repoblado desde la tabla
        // al arrancar para sobrevivir a restarts sin abandonar incidentes.
        let mut active: HashMap<Uuid, AlertIncidentRow> = HashMap::new();
        match load_active_incidents(&state).await {
            Ok(rows) => {
                for row in rows {
                    active.insert(row.rule_id, row);
                }
                tracing::info!(
                    active = active.len(),
                    "incidentes de anomalía recuperados al arrancar"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "no se pudieron recuperar incidentes activos — empezamos en blanco");
            }
        }

        // Espera inicial: 30s para que el resto del backend termine de ponerse
        // en marcha y el primer batch de logs/spans aterrice.
        tokio::time::sleep(Duration::from_secs(30)).await;

        let mut tick = interval(Duration::from_secs(interval_secs));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // El primer tick es inmediato; lo descartamos para no correr antes
        // de la espera inicial que acabamos de hacer.
        tick.tick().await;

        loop {
            tick.tick().await;
            metrics::counter!(crate::observability::names::WORKER_RUNS, "worker" => "anomaly_detector").increment(1);
            for signal in [Signal::Errors, Signal::P95Latency, Signal::LogVolume] {
                if let Err(e) = evaluate_signal(&state, signal, &mut active).await {
                    metrics::counter!(crate::observability::names::WORKER_ERRORS, "worker" => "anomaly_detector").increment(1);
                    tracing::warn!(signal = signal.slug(), error = %e, "evaluación de anomalía falló");
                }
            }
        }
    });
}

async fn load_active_incidents(state: &SharedState) -> anyhow::Result<Vec<AlertIncidentRow>> {
    // Filtramos por prefijo del rule_name. ReplacingMergeTree + FINAL nos da
    // el último estado por (rule_id, started_at, id).
    state
        .ch
        .select::<AlertIncidentRow>(
            "SELECT id, project_id, rule_id, rule_name, started_at, resolved_at, \
                    value, threshold, severity, status, note, version \
             FROM faro.alert_incidents FINAL \
             WHERE status = 'firing' AND startsWith(rule_name, 'anomaly:') LIMIT 1000",
        )
        .await
}

async fn evaluate_signal(
    state: &SharedState,
    signal: Signal,
    active: &mut HashMap<Uuid, AlertIncidentRow>,
) -> anyhow::Result<()> {
    let window_secs = (state.cfg.anomaly_window_minutes.max(1) as u32) * 60;
    let sql = build_query(signal, window_secs);
    let rows: Vec<SampleRow> = state.ch.select(&sql).await?;

    let z_fire = state.cfg.anomaly_z_fire;
    let z_resolve = state.cfg.anomaly_z_resolve;
    let min_baseline = match signal {
        Signal::Errors => state.cfg.anomaly_min_baseline_errors,
        Signal::P95Latency => state.cfg.anomaly_min_baseline_p95_ms,
        Signal::LogVolume => state.cfg.anomaly_min_baseline_logs,
    };

    for row in rows {
        let samples = [row.s1, row.s2, row.s3, row.s4, row.s5, row.s6, row.s7];
        let stat = match summarize(&samples) {
            Some(s) => s,
            None => continue, // muy pocas muestras válidas
        };

        let rule_id = anomaly_rule_id(&row.project_id, &row.service_name, signal);
        let was_firing = active.contains_key(&rule_id);

        // La decisión (z-score + histéresis fire/resolve + corte por baseline) es
        // una función PURA y testeada (`anomaly_decision`); acá sólo aplicamos sus
        // efectos (DB/notify), que es lo que no se puede unit-testear en aislamiento.
        match anomaly_decision(
            row.current,
            stat.mean,
            stat.stddev,
            min_baseline,
            z_fire,
            z_resolve,
            was_firing,
        ) {
            AnomalyDecision::Fire(z) => {
                let rule_name = format!(
                    "{}{}:{}:{}",
                    RULE_NAME_PREFIX,
                    signal.slug(),
                    row.project_id,
                    row.service_name
                );
                fire(
                    state,
                    active,
                    rule_id,
                    rule_name,
                    row.project_id.clone(),
                    row.service_name.clone(),
                    signal,
                    row.current,
                    &stat,
                    z,
                )
                .await;
            }
            AnomalyDecision::Resolve => resolve(state, active, rule_id).await,
            AnomalyDecision::Hold => {}
        }
    }

    Ok(())
}

/// Decisión del detector de anomalías para una serie (proyecto, servicio, señal).
/// PURA y testeable: aísla el z-score + la histéresis fire/resolve + el corte por
/// baseline de los efectos (DB/notify) que viven en `evaluate_signal`. Sólo mira
/// desviaciones por ARRIBA (un drop es ambiguo: ¿servicio sano con menos tráfico?
/// ¿caído? ¿fin de semana?).
#[derive(Debug, PartialEq)]
pub(crate) enum AnomalyDecision {
    /// Disparar un incidente nuevo, con el z-score que lo gatilló.
    Fire(f64),
    /// Resolver el incidente activo (bajó del umbral de resolución).
    Resolve,
    /// Sin cambio de estado (baja señal, o entre umbrales, o ya disparado).
    Hold,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn anomaly_decision(
    current: f64,
    mean: f64,
    stddev: f64,
    min_baseline: f64,
    z_fire: f64,
    z_resolve: f64,
    was_firing: bool,
) -> AnomalyDecision {
    // Serie de baja señal: no disparamos ni forzamos resolve sobre ruido.
    if mean < min_baseline {
        return AnomalyDecision::Hold;
    }
    let z = if stddev > f64::EPSILON {
        (current - mean) / stddev
    } else if current > mean * 2.0 {
        // stddev = 0 (muestras históricas iguales) y subimos sobre el doble del
        // baseline → forzamos un z grande para que dispare igual.
        10.0
    } else {
        0.0
    };
    if z >= z_fire && !was_firing {
        AnomalyDecision::Fire(z)
    } else if was_firing && z <= z_resolve {
        AnomalyDecision::Resolve
    } else {
        AnomalyDecision::Hold
    }
}

#[derive(Debug)]
struct BaselineStats {
    mean: f64,
    stddev: f64,
    n: usize,
}

/// Calcula media y desviación estándar (corrected, n-1) sobre las muestras
/// que sean finitas. Devuelve None si quedan menos de 4 muestras útiles —
/// con menos, la estimación de stddev es demasiado ruidosa para apoyar un
/// z-score con threshold de 3.
fn summarize(samples: &[f64]) -> Option<BaselineStats> {
    let valid: Vec<f64> = samples.iter().copied().filter(|v| v.is_finite()).collect();
    if valid.len() < 4 {
        return None;
    }
    let n = valid.len() as f64;
    let mean = valid.iter().sum::<f64>() / n;
    let var = valid.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Some(BaselineStats {
        mean,
        stddev: var.sqrt(),
        n: valid.len(),
    })
}

#[allow(clippy::too_many_arguments)]
async fn fire(
    state: &SharedState,
    active: &mut HashMap<Uuid, AlertIncidentRow>,
    rule_id: Uuid,
    rule_name: String,
    project_id: String,
    service_name: String,
    signal: Signal,
    current: f64,
    stat: &BaselineStats,
    z: f64,
) {
    let now = Utc::now();
    // El "threshold" de un z-score no tiene un valor humano natural; usamos
    // `mean + z_fire * stddev` para que la columna `threshold` del incidente
    // diga "este valor habría disparado". Útil cuando el dashboard renderiza
    // un row sin contexto adicional.
    let z_fire = state.cfg.anomaly_z_fire;
    let threshold = stat.mean + z_fire * stat.stddev;
    let severity = if z >= 5.0 { "critical" } else { "warn" };
    let note = format!(
        "{} en {} — actual {:.2} {}, baseline {:.2} ± {:.2} ({} muestras, z={:.2})",
        signal.human(),
        service_name,
        current,
        signal.unit(),
        stat.mean,
        stat.stddev,
        stat.n,
        z,
    );

    let incident = AlertIncidentRow {
        id: Uuid::new_v4(),
        project_id,
        rule_id,
        rule_name,
        started_at: now,
        resolved_at: None,
        value: current,
        threshold,
        severity: severity.into(),
        status: "firing".into(),
        note,
        version: incident_version(now, 1),
    };
    if let Err(e) = state
        .ch
        .insert("faro.alert_incidents", &[incident.clone()])
        .await
    {
        tracing::error!(error = %e, "no se pudo persistir incidente de anomalía");
        return;
    }
    tracing::warn!(
        signal = signal.slug(),
        service = %service_name,
        current,
        baseline_mean = stat.mean,
        baseline_stddev = stat.stddev,
        z,
        "anomalía detectada"
    );
    active.insert(rule_id, incident);

    // Por ahora no notificamos por webhook/telegram — las anomalías no tienen
    // `notification_targets` asociados (vienen sin AlertRuleRow). Se ven en
    // el dashboard de incidentes. Quizás en V2: targets por defecto por proyecto.
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
        tracing::error!(error = %e, "no se pudo persistir resolución de anomalía");
        // Lo dejamos fuera del active set igualmente — si el insert falla seguiremos
        // marcando como firing en la próxima tick si la condición se mantiene.
        return;
    }
    tracing::info!(rule = %incident.rule_name, "anomalía resuelta");
}

/// Versión monotónica para el ReplacingMergeTree de alert_incidents — usamos
/// el timestamp en milisegundos para que el orden de upserts sea correcto
/// incluso cruzando reinicios. El bump `+offset` evita colisión cuando dos
/// updates ocurren en el mismo ms.
fn incident_version(now: chrono::DateTime<Utc>, offset: u64) -> u64 {
    (now.timestamp_millis() as u64).saturating_add(offset)
}

fn anomaly_rule_id(project: &str, service: &str, signal: Signal) -> Uuid {
    let name = format!("anomaly:{}:{}:{}", signal.slug(), project, service);
    Uuid::new_v5(&ANOMALY_NAMESPACE, name.as_bytes())
}

/// Construye el SQL parametrizado por offset de días. La forma `current + s1..s7`
/// permite parsear el resultado a una struct fija sin arrays serializados.
fn build_query(signal: Signal, window_secs: u32) -> String {
    let day = 86_400u32;
    let total = 7 * day + window_secs;

    // Genera "AND ... AS sN," por cada día histórico.
    let mut samples = String::new();
    for d in 1..=7u32 {
        let start = d * day + window_secs;
        let end = d * day;
        let frag = match signal {
            Signal::Errors => format!(
                "toFloat64(countIf(timestamp >= now() - INTERVAL {start} SECOND \
                                  AND timestamp <  now() - INTERVAL {end} SECOND \
                                  AND severity_number >= 17)) AS s{d}, "
            ),
            Signal::LogVolume => format!(
                "toFloat64(countIf(timestamp >= now() - INTERVAL {start} SECOND \
                                  AND timestamp <  now() - INTERVAL {end} SECOND)) AS s{d}, "
            ),
            Signal::P95Latency => format!(
                "toFloat64(quantileExactIf(0.95)(duration_ns, \
                    timestamp >= now() - INTERVAL {start} SECOND \
                    AND timestamp <  now() - INTERVAL {end} SECOND) / 1000000.0) AS s{d}, "
            ),
        };
        samples.push_str(&frag);
    }
    // Quita la coma final + espacio.
    if samples.ends_with(", ") {
        samples.truncate(samples.len() - 2);
    }

    let (table, current_expr, having) = match signal {
        Signal::Errors => (
            "faro.logs",
            format!(
                "toFloat64(countIf(timestamp >= now() - INTERVAL {window_secs} SECOND \
                                  AND severity_number >= 17)) AS current"
            ),
            "HAVING current > 0 OR (s1 + s2 + s3 + s4 + s5 + s6 + s7) > 0",
        ),
        Signal::LogVolume => (
            "faro.logs",
            format!(
                "toFloat64(countIf(timestamp >= now() - INTERVAL {window_secs} SECOND)) AS current"
            ),
            "HAVING current > 0 OR (s1 + s2 + s3 + s4 + s5 + s6 + s7) > 0",
        ),
        Signal::P95Latency => (
            "faro.spans",
            format!(
                "toFloat64(quantileExactIf(0.95)(duration_ns, \
                    timestamp >= now() - INTERVAL {window_secs} SECOND) / 1000000.0) AS current"
            ),
            // En spans el filtro de no-cero lo hacemos en Rust (NaN posible).
            "HAVING current > 0",
        ),
    };

    format!(
        "SELECT project_id, service_name, {current_expr}, {samples} \
         FROM {table} \
         WHERE timestamp >= now() - INTERVAL {total} SECOND \
         GROUP BY project_id, service_name \
         {having} \
         LIMIT 500"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_filters_nans_and_needs_min_samples() {
        let with_nans = [f64::NAN, 1.0, 2.0, 3.0, f64::NAN, f64::NAN, f64::NAN];
        assert!(summarize(&with_nans).is_none());

        let enough = [10.0, 12.0, 11.0, 13.0, 9.0, 10.0, 11.0];
        let s = summarize(&enough).unwrap();
        assert!((s.mean - 10.857142857).abs() < 1e-6);
        assert_eq!(s.n, 7);
        assert!(s.stddev > 0.0);
    }

    #[test]
    fn rule_id_is_deterministic() {
        let a = anomaly_rule_id("p1", "svc-a", Signal::Errors);
        let b = anomaly_rule_id("p1", "svc-a", Signal::Errors);
        assert_eq!(a, b);
        let c = anomaly_rule_id("p1", "svc-a", Signal::P95Latency);
        assert_ne!(a, c);
    }

    // ---- Decisión (z-score + histéresis) ----
    // z_fire=3.0, z_resolve=1.5, min_baseline=2.0 en estos casos.

    #[test]
    fn decision_fires_when_z_exceeds_fire_and_not_already_firing() {
        // mean=10, stddev=2, current=20 → z=5 ≥ 3 → Fire(5).
        let d = anomaly_decision(20.0, 10.0, 2.0, 2.0, 3.0, 1.5, false);
        assert_eq!(d, AnomalyDecision::Fire(5.0));
    }

    #[test]
    fn decision_holds_in_hysteresis_band() {
        // Estaba firing; z=2 cae entre resolve(1.5) y fire(3) → Hold (no aletea).
        // mean=10, stddev=5, current=20 → z=2.
        let d = anomaly_decision(20.0, 10.0, 5.0, 2.0, 3.0, 1.5, true);
        assert_eq!(d, AnomalyDecision::Hold);
    }

    #[test]
    fn decision_resolves_when_below_resolve_threshold() {
        // Estaba firing; z=1 ≤ 1.5 → Resolve. mean=10, stddev=5, current=15 → z=1.
        let d = anomaly_decision(15.0, 10.0, 5.0, 2.0, 3.0, 1.5, true);
        assert_eq!(d, AnomalyDecision::Resolve);
    }

    #[test]
    fn decision_does_not_refire_when_already_firing() {
        // z alto pero ya estaba firing → Hold (no re-dispara un incidente nuevo).
        let d = anomaly_decision(20.0, 10.0, 2.0, 2.0, 3.0, 1.5, true);
        assert_eq!(d, AnomalyDecision::Hold);
    }

    #[test]
    fn decision_holds_below_min_baseline() {
        // mean (1.0) < min_baseline (2.0) → Hold aunque current sea enorme (anti-ruido).
        let d = anomaly_decision(100.0, 1.0, 0.1, 2.0, 3.0, 1.5, false);
        assert_eq!(d, AnomalyDecision::Hold);
    }

    #[test]
    fn decision_stddev_zero_fires_when_more_than_double_baseline() {
        // stddev=0 y current > 2×mean → z=10 forzado → Fire. mean=10, current=25.
        let d = anomaly_decision(25.0, 10.0, 0.0, 2.0, 3.0, 1.5, false);
        assert_eq!(d, AnomalyDecision::Fire(10.0));
        // stddev=0 pero current no supera el doble → z=0 → Hold.
        let d = anomaly_decision(15.0, 10.0, 0.0, 2.0, 3.0, 1.5, false);
        assert_eq!(d, AnomalyDecision::Hold);
    }

    #[test]
    fn query_has_all_seven_samples_and_filters() {
        let q = build_query(Signal::Errors, 300);
        for d in 1..=7 {
            assert!(q.contains(&format!("AS s{d}")), "falta sample s{d}: {q}");
        }
        assert!(q.contains("severity_number >= 17"));
        assert!(q.contains("FROM faro.logs"));

        let q = build_query(Signal::P95Latency, 300);
        assert!(q.contains("quantileExactIf(0.95)"));
        assert!(q.contains("FROM faro.spans"));
        assert!(q.contains("/ 1000000.0"));
    }
}
