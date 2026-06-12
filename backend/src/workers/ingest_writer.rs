//! Workers de escritura por lotes (el otro extremo de los canales de ingesta).
//!
//! Arranca un writer por canal (logs, spans, metrics, events, resultados de
//! monitores): cada uno acumula filas hasta llegar a N o a T y las vuelca a
//! ClickHouse. Ante fallo descarta el lote tras loguear (el buffer durable sería
//! una cola externa que se cablearía más adelante).

use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::mpsc::Receiver;
use tokio::time::{interval, MissedTickBehavior};

use crate::observability::names;
use crate::state::SharedState;
use crate::storage::Client;

/// Arranca un writer en segundo plano por cada canal de ingesta. Cada writer agrupa filas
/// hasta N o hasta T y vuelca a ClickHouse. En caso de fallo, las filas se descartan tras
/// loguear — el buffering durable corresponde a una capa de cola (Redis/Kafka) que podemos
/// cablear más adelante.
pub fn start_ingest_writers(state: SharedState, shutdown: tokio::sync::watch::Receiver<bool>) {
    let logs_rx = state
        .ingest
        .logs_rx
        .lock()
        .take()
        .expect("rx de logs ya tomado");
    let spans_rx = state
        .ingest
        .spans_rx
        .lock()
        .take()
        .expect("rx de spans ya tomado");
    let metrics_rx = state
        .ingest
        .metrics_rx
        .lock()
        .take()
        .expect("rx de metrics ya tomado");
    let monitor_rx = state
        .ingest
        .monitor_results_rx
        .lock()
        .take()
        .expect("rx de monitor ya tomado");
    let events_rx = state
        .ingest
        .events_rx
        .lock()
        .take()
        .expect("rx de events ya tomado");

    let max = state.cfg.batch_max_rows;
    let flush_ms = state.cfg.batch_flush_ms;

    spawn_writer(
        "faro.logs",
        state.ch.clone(),
        logs_rx,
        max,
        flush_ms,
        shutdown.clone(),
    );
    spawn_writer(
        "faro.spans",
        state.ch.clone(),
        spans_rx,
        max,
        flush_ms,
        shutdown.clone(),
    );
    spawn_writer(
        "faro.metrics",
        state.ch.clone(),
        metrics_rx,
        max,
        flush_ms,
        shutdown.clone(),
    );
    spawn_writer(
        "faro.monitor_results",
        state.ch.clone(),
        monitor_rx,
        max,
        flush_ms,
        shutdown.clone(),
    );
    spawn_writer(
        "faro.product_events",
        state.ch.clone(),
        events_rx,
        max,
        flush_ms,
        shutdown,
    );
}

fn spawn_writer<T>(
    table: &'static str,
    ch: Client,
    mut rx: Receiver<T>,
    max_rows: usize,
    flush_ms: u64,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) where
    T: Serialize + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut buf: Vec<T> = Vec::with_capacity(max_rows);
        let mut tick = interval(Duration::from_millis(flush_ms));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(row) => {
                            buf.push(row);
                            if buf.len() >= max_rows {
                                flush(table, &ch, &mut buf).await;
                            }
                        }
                        None => {
                            flush(table, &ch, &mut buf).await;
                            tracing::warn!(%table, "canal de ingesta cerrado");
                            break;
                        }
                    }
                }
                _ = tick.tick() => {
                    if !buf.is_empty() {
                        flush(table, &ch, &mut buf).await;
                    }
                }
                changed = shutdown.changed() => {
                    // `Ok` = el watch cambió (apagado señalado). `Err` = el
                    // coordinador se dropeó (la app está terminando). En ambos
                    // casos drenamos el canal + buffer y salimos, para no perder
                    // la telemetría en vuelo ni quedar girando en vacío.
                    let stop = changed.map(|_| *shutdown.borrow_and_update()).unwrap_or(true);
                    if stop {
                        // Apagado ordenado: drenar lo que quede en el canal y vaciar
                        // el buffer a ClickHouse antes de salir, para no perder la
                        // telemetría en vuelo en cada deploy/restart.
                        while let Ok(row) = rx.try_recv() {
                            buf.push(row);
                            if buf.len() >= max_rows {
                                flush(table, &ch, &mut buf).await;
                            }
                        }
                        flush(table, &ch, &mut buf).await;
                        tracing::info!(%table, "writer drenado en apagado");
                        break;
                    }
                }
            }
        }
    });
}

async fn flush<T: Serialize>(table: &'static str, ch: &Client, buf: &mut Vec<T>) {
    if buf.is_empty() {
        return;
    }
    let n = buf.len();
    let start = Instant::now();
    match ch.insert(table, buf).await {
        Ok(()) => {
            let elapsed = start.elapsed().as_secs_f64();
            // Histogram en segundos — buckets de axum-prometheus por defecto son
            // SECONDS_DURATION_BUCKETS, que cubre el rango interesante para inserts
            // a ClickHouse (1ms a varios segundos).
            metrics::histogram!(names::CH_INSERT_SECONDS, "table" => table).record(elapsed);
            metrics::counter!(names::CH_ROWS_INSERTED, "table" => table).increment(n as u64);
            tracing::debug!(%table, rows = n, "lote volcado");
        }
        Err(e) => {
            metrics::counter!(
                names::CH_ERRORS,
                "table" => table,
                "operation" => "insert",
            )
            .increment(1);
            // El lote entero se descarta (sin reintento ni buffer durable): contamos
            // las FILAS perdidas, no sólo el INSERT fallido, para dimensionar la
            // pérdida real de telemetría.
            metrics::counter!(names::CH_ROWS_DROPPED, "table" => table).increment(n as u64);
            tracing::error!(%table, rows = n, error = %e, "flush failed, dropping batch");
        }
    }
    buf.clear();
}
