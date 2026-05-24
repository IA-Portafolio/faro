//! Detector de servicios inactivos.
//!
//! Cada [`Config::stale_detector_interval_secs`] (default 1h) consulta
//! `faro.services_seen` (MV que agrega `max(timestamp)` por `(project, service)`
//! cruzando logs+spans+metrics) y compara contra el umbral
//! [`Config::stale_threshold_hours`] (default 24).
//!
//! El estado vivo de cada servicio se mantiene en memoria. En el primer tick
//! post-restart, se inicializa leyendo el último evento de
//! `faro.service_stale_events` por servicio — sin esto, todo arrancaría como
//! "active" y emitiríamos un montón de transiciones falsas tras cada reinicio.
//!
//! Cuando detecta una transición `active → stale` o `stale → recovered`, inserta
//! una fila en `faro.service_stale_events`. La vista del dashboard `/services/stale`
//! consulta `services_seen` directo (con el filtro de umbral) en lugar de leer
//! del log de eventos — el log es para audit/histórico.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::time::{interval, MissedTickBehavior};

use crate::state::SharedState;
use crate::storage::ServiceStaleEventRow;

#[derive(Deserialize, Debug)]
struct ServiceSeenRow {
    project_id: String,
    service_name: String,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    last_seen_at: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
struct LastEventRow {
    project_id: String,
    service_name: String,
    last_event: String,
}

pub fn start_stale_detector(state: SharedState) {
    if !state.cfg.stale_detector_enabled {
        tracing::info!("detector de stale deshabilitado");
        return;
    }
    let interval_secs = state.cfg.stale_detector_interval_secs.max(60);
    let threshold_hours = state.cfg.stale_threshold_hours.max(1);

    tracing::info!(
        every_secs = interval_secs,
        threshold_hours,
        "arrancando detector de servicios stale"
    );

    tokio::spawn(async move {
        // Estado en memoria: por (project, service) si lo consideramos stale ahora.
        // Cargado al arrancar desde el último evento persistido para evitar
        // emitir un mar de transiciones falsas tras un restart.
        let mut is_stale: HashMap<(String, String), bool> = match load_last_events(&state).await {
            Ok(rows) => {
                let map: HashMap<(String, String), bool> = rows
                    .into_iter()
                    .map(|r| ((r.project_id, r.service_name), r.last_event == "stale"))
                    .collect();
                tracing::info!(services = map.len(), "stale: estado inicial recuperado");
                map
            }
            Err(e) => {
                tracing::warn!(error = %e, "stale: no se pudo recuperar estado inicial — empezamos vacío");
                HashMap::new()
            }
        };

        // Espera inicial para que el resto del backend arranque.
        tokio::time::sleep(Duration::from_secs(60)).await;

        let mut tick = interval(Duration::from_secs(interval_secs));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.tick().await; // descarta tick inmediato

        loop {
            tick.tick().await;
            match detect_once(&state, &mut is_stale, threshold_hours).await {
                Ok((stale_now, transitions)) => {
                    if transitions > 0 {
                        tracing::info!(
                            transitions,
                            stale_now,
                            "stale: tick completo con transiciones"
                        );
                    } else {
                        tracing::debug!(stale_now, "stale: tick sin transiciones");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "stale: tick falló"),
            }
        }
    });
}

async fn load_last_events(state: &SharedState) -> anyhow::Result<Vec<LastEventRow>> {
    state
        .ch
        .select::<LastEventRow>(
            "SELECT project_id, service_name, argMax(event, timestamp) AS last_event
             FROM faro.service_stale_events
             GROUP BY project_id, service_name",
        )
        .await
}

/// Devuelve `(stale_now, transitions_emitted)`.
async fn detect_once(
    state: &SharedState,
    is_stale: &mut HashMap<(String, String), bool>,
    threshold_hours: u32,
) -> anyhow::Result<(usize, usize)> {
    // services_seen es AggregatingMergeTree: necesitamos maxMerge para reducir
    // los partial states a un DateTime real.
    let rows: Vec<ServiceSeenRow> = state
        .ch
        .select(
            "SELECT project_id, service_name,
                    maxMerge(last_seen_at) AS last_seen_at
             FROM faro.services_seen
             GROUP BY project_id, service_name",
        )
        .await?;

    let now = Utc::now();
    let threshold_secs = (threshold_hours as i64) * 3600;
    let mut events: Vec<ServiceStaleEventRow> = Vec::new();
    let mut stale_count = 0usize;

    for r in rows {
        let silence = (now - r.last_seen_at).num_seconds();
        let currently_stale = silence > threshold_secs;
        if currently_stale {
            stale_count += 1;
        }

        let key = (r.project_id.clone(), r.service_name.clone());
        let was_stale = *is_stale.get(&key).unwrap_or(&false);

        if currently_stale != was_stale {
            // Transición. Emitir evento.
            let silence_hours = silence as f64 / 3600.0;
            let event_name = if currently_stale {
                "stale"
            } else {
                "recovered"
            };
            events.push(ServiceStaleEventRow {
                timestamp: now,
                project_id: r.project_id.clone(),
                service_name: r.service_name.clone(),
                event: event_name.to_string(),
                last_seen_at: r.last_seen_at,
                silence_hours,
            });
            is_stale.insert(key, currently_stale);
            tracing::info!(
                project = r.project_id,
                service = r.service_name,
                event = event_name,
                silence_hours,
                "transición de stale detectada"
            );
        } else {
            // Sin cambio — sólo asegurar que el HashMap refleja el estado actual
            // (caso primer tick post-restart sin evento previo).
            is_stale.entry(key).or_insert(currently_stale);
        }
    }

    let transitions = events.len();
    if !events.is_empty() {
        state
            .ch
            .insert("faro.service_stale_events", &events)
            .await?;
    }

    Ok((stale_count, transitions))
}
