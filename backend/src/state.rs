use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::{broadcast, mpsc};

use crate::config::Config;
use crate::projects::ProjectCache;
use crate::storage::{Client, LogRow, MetricRow, MonitorResultRow, SpanRow};

/// Batching basado en canales: los handlers de ingesta empujan filas aquí y una tarea
/// writer toma lotes y los vuelca a ClickHouse.
pub struct IngestChannels {
    pub logs_tx: mpsc::Sender<LogRow>,
    pub spans_tx: mpsc::Sender<SpanRow>,
    pub metrics_tx: mpsc::Sender<MetricRow>,
    pub monitor_results_tx: mpsc::Sender<MonitorResultRow>,
    pub logs_rx: Mutex<Option<mpsc::Receiver<LogRow>>>,
    pub spans_rx: Mutex<Option<mpsc::Receiver<SpanRow>>>,
    pub metrics_rx: Mutex<Option<mpsc::Receiver<MetricRow>>>,
    pub monitor_results_rx: Mutex<Option<mpsc::Receiver<MonitorResultRow>>>,
}

impl IngestChannels {
    pub fn new() -> Self {
        let (logs_tx, logs_rx) = mpsc::channel(32_768);
        let (spans_tx, spans_rx) = mpsc::channel(32_768);
        let (metrics_tx, metrics_rx) = mpsc::channel(32_768);
        let (monitor_results_tx, monitor_results_rx) = mpsc::channel(4_096);
        Self {
            logs_tx,
            spans_tx,
            metrics_tx,
            monitor_results_tx,
            logs_rx: Mutex::new(Some(logs_rx)),
            spans_rx: Mutex::new(Some(spans_rx)),
            metrics_rx: Mutex::new(Some(metrics_rx)),
            monitor_results_rx: Mutex::new(Some(monitor_results_rx)),
        }
    }
}

/// Bus de broadcast para live tailing y notificaciones entre tareas.
#[derive(Clone)]
pub struct LiveBus {
    pub logs: broadcast::Sender<LogRow>,
}

impl LiveBus {
    pub fn new() -> Self {
        let (logs, _) = broadcast::channel(1024);
        Self { logs }
    }
}

pub struct AppState {
    pub cfg: Config,
    pub ch: Client,
    pub ingest: IngestChannels,
    pub live_bus: LiveBus,
    pub projects: ProjectCache,
}

impl AppState {
    pub fn new(cfg: Config, ch: Client) -> Self {
        Self {
            cfg,
            ch,
            ingest: IngestChannels::new(),
            live_bus: LiveBus::new(),
            projects: ProjectCache::new(),
        }
    }
}

pub type SharedState = Arc<AppState>;
