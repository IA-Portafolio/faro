use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc::Receiver;
use tokio::time::{interval, MissedTickBehavior};

use crate::state::SharedState;
use crate::storage::Client;

/// Arranca un writer en segundo plano por cada canal de ingesta. Cada writer agrupa filas
/// hasta N o hasta T y vuelca a ClickHouse. En caso de fallo, las filas se descartan tras
/// loguear — el buffering durable corresponde a una capa de cola (Redis/Kafka) que podemos
/// cablear más adelante.
pub fn start_ingest_writers(state: SharedState) {
    let logs_rx = state.ingest.logs_rx.lock().take().expect("rx de logs ya tomado");
    let spans_rx = state.ingest.spans_rx.lock().take().expect("rx de spans ya tomado");
    let metrics_rx = state.ingest.metrics_rx.lock().take().expect("rx de metrics ya tomado");
    let monitor_rx = state.ingest.monitor_results_rx.lock().take().expect("rx de monitor ya tomado");

    let max = state.cfg.batch_max_rows;
    let flush_ms = state.cfg.batch_flush_ms;

    spawn_writer("faro.logs", state.ch.clone(), logs_rx, max, flush_ms);
    spawn_writer("faro.spans", state.ch.clone(), spans_rx, max, flush_ms);
    spawn_writer("faro.metrics", state.ch.clone(), metrics_rx, max, flush_ms);
    spawn_writer("faro.monitor_results", state.ch.clone(), monitor_rx, max, flush_ms);
}

fn spawn_writer<T>(
    table: &'static str,
    ch: Client,
    mut rx: Receiver<T>,
    max_rows: usize,
    flush_ms: u64,
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
            }
        }
    });
}

async fn flush<T: Serialize>(table: &str, ch: &Client, buf: &mut Vec<T>) {
    if buf.is_empty() {
        return;
    }
    let n = buf.len();
    match ch.insert(table, buf).await {
        Ok(()) => tracing::debug!(%table, rows = n, "lote volcado"),
        Err(e) => tracing::error!(%table, rows = n, error = %e, "flush failed, dropping batch"),
    }
    buf.clear();
}
