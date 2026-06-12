//! Worker que ejecuta los monitores (checks HTTP de disponibilidad).
//!
//! Hace polling de la tabla de monitores y lanza cada chequeo HTTP en su propio
//! intervalo, reutilizando un único cliente reqwest (connection pooling). Cada
//! resultado se encola como `MonitorResultRow` y alimenta el uptime y las alertas.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use regex::Regex;
use tokio::time::{interval, MissedTickBehavior};
use uuid::Uuid;

use crate::monitor_url::validate_monitor_url;
use crate::state::SharedState;
use crate::storage::{MonitorResultRow, MonitorRow};

/// Hace polling periódico de api_monitors y ejecuta cada chequeo HTTP configurado en
/// su propio intervalo. Reutiliza un único cliente reqwest para hacer connection pooling.
pub fn start_monitor_runner(state: SharedState) {
    tokio::spawn(async move {
        let mut next_run: HashMap<Uuid, Instant> = HashMap::new();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("cliente reqwest");

        let mut reload = interval(Duration::from_secs(10));
        reload.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut tick = interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut monitors: Vec<MonitorRow> = Vec::new();

        loop {
            tokio::select! {
                _ = reload.tick() => {
                    match load_monitors(&state).await {
                        Ok(m) => monitors = m,
                        Err(e) => {
                            tracing::warn!(error = %e, "falló el reload de la lista de monitores");
                            metrics::counter!(crate::observability::names::WORKER_ERRORS, "worker" => "monitor_runner").increment(1);
                        }
                    }
                }
                _ = tick.tick() => {
                    metrics::counter!(crate::observability::names::WORKER_RUNS, "worker" => "monitor_runner").increment(1);
                    let now = Instant::now();
                    for m in &monitors {
                        if m.enabled == 0 || m.deleted == 1 {
                            continue;
                        }
                        let due = next_run.get(&m.id).copied().unwrap_or(now);
                        if due > now {
                            continue;
                        }
                        next_run.insert(m.id, now + Duration::from_secs(m.interval_seconds as u64));
                        if let Err(reason) = validate_monitor_url(&m.url) {
                            tracing::warn!(
                                monitor_id = %m.id,
                                url = %m.url,
                                reason = %reason,
                                "URL de monitor bloqueada (SSRF); omitiendo check"
                            );
                            let row = MonitorResultRow {
                                monitor_id: m.id,
                                project_id: m.project_id.clone(),
                                timestamp: Utc::now(),
                                success: 0,
                                status_code: 0,
                                duration_ms: 0,
                                error_message: format!(
                                    "URL bloqueada por política de seguridad: {reason}"
                                ),
                                response_size: 0,
                            };
                            let _ = state.ingest.monitor_results_tx.try_send(row);
                            continue;
                        }
                        let client = client.clone();
                        let mc = m.clone();
                        let state = state.clone();
                        tokio::spawn(async move {
                            run_check(client, mc, state).await;
                        });
                    }
                }
            }
        }
    });
}

async fn load_monitors(state: &SharedState) -> anyhow::Result<Vec<MonitorRow>> {
    state
        .ch
        .select::<MonitorRow>(
            "SELECT id, project_id, name, method, url, headers, body, interval_seconds, timeout_seconds, \
             expected_status_min, expected_status_max, expected_body_regex, enabled, \
             created_at, updated_at, deleted, version \
             FROM faro.api_monitors FINAL WHERE deleted = 0",
        )
        .await
}

/// Ejecuta un único monitor: hace el request HTTP, evalúa éxito según rango de
/// status + regex opcional de body, y encola la fila resultado en el canal de
/// ingesta. Pub para que `tests/workers_monitor_runner.rs` la pueda invocar sin
/// spawnear el loop completo.
pub async fn run_check(client: reqwest::Client, m: MonitorRow, state: SharedState) {
    let start = Instant::now();
    let req_builder = match m.method.to_ascii_uppercase().as_str() {
        "GET" => client.get(&m.url),
        "POST" => client.post(&m.url),
        "PUT" => client.put(&m.url),
        "DELETE" => client.delete(&m.url),
        "HEAD" => client.head(&m.url),
        "PATCH" => client.patch(&m.url),
        other => {
            tracing::warn!(
                method = other,
                "unsupported monitor method, defaulting to GET"
            );
            client.get(&m.url)
        }
    };

    let mut req = req_builder.timeout(Duration::from_secs(m.timeout_seconds as u64));
    for (k, v) in &m.headers {
        req = req.header(k, v);
    }
    if !m.body.is_empty() {
        req = req.body(m.body.clone());
    }

    let res = req.send().await;
    let elapsed_ms = start.elapsed().as_millis() as u32;

    let (success, status_code, error_msg, size) = match res {
        Ok(r) => {
            let status = r.status().as_u16();
            let in_range = status >= m.expected_status_min && status <= m.expected_status_max;
            let body = r.text().await.unwrap_or_default();
            let size = body.len() as u32;
            let mut ok = in_range;
            if ok && !m.expected_body_regex.is_empty() {
                match Regex::new(&m.expected_body_regex) {
                    Ok(re) => ok = re.is_match(&body),
                    Err(e) => {
                        tracing::warn!(regex = %m.expected_body_regex, error = %e, "regex de monitor inválido");
                    }
                }
            }
            let err = if ok {
                String::new()
            } else if !in_range {
                format!(
                    "status {status} outside {}-{}",
                    m.expected_status_min, m.expected_status_max
                )
            } else {
                "body did not match expected pattern".into()
            };
            (ok as u8, status, err, size)
        }
        Err(e) => (0u8, 0u16, e.to_string(), 0u32),
    };

    let row = MonitorResultRow {
        monitor_id: m.id,
        project_id: m.project_id.clone(),
        timestamp: Utc::now(),
        success,
        status_code,
        duration_ms: elapsed_ms,
        error_message: error_msg,
        response_size: size,
    };
    let _ = state.ingest.monitor_results_tx.try_send(row);
}
