//! Estado compartido de la aplicación (`AppState` / `SharedState`).
//!
//! Reúne lo que necesitan handlers y workers: cliente de ClickHouse, `Config`,
//! caches (proyectos, feature flags, integraciones, canales) y los canales de
//! ingesta por batching (`IngestChannels`: los handlers empujan filas y un writer
//! las vuelca a ClickHouse), además del bus de eventos en vivo (SSE) y los rate
//! limiters.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::{broadcast, mpsc};

use crate::config::Config;
use crate::feature_flags::FeatureFlagsCache;
use crate::ingest::rate_limit::IngestLimiter;
use crate::integrations::IntegrationsCache;
use crate::notification_channels::NotificationChannelsCache;
use crate::projects::ProjectCache;
use crate::storage::{Client, LogRow, MetricRow, MonitorResultRow, ProductEventRow, SpanRow};
use crate::totp::TotpRateLimiter;

/// Batching basado en canales: los handlers de ingesta empujan filas aquí y una tarea
/// writer toma lotes y los vuelca a ClickHouse.
pub struct IngestChannels {
    pub logs_tx: mpsc::Sender<LogRow>,
    pub spans_tx: mpsc::Sender<SpanRow>,
    pub metrics_tx: mpsc::Sender<MetricRow>,
    pub monitor_results_tx: mpsc::Sender<MonitorResultRow>,
    /// Canal de ingesta para product events (6º pilar). Mismo modelo que el resto:
    /// el handler HTTP empuja `ProductEventRow` y un writer drena a `faro.product_events`.
    pub events_tx: mpsc::Sender<ProductEventRow>,
    pub logs_rx: Mutex<Option<mpsc::Receiver<LogRow>>>,
    pub spans_rx: Mutex<Option<mpsc::Receiver<SpanRow>>>,
    pub metrics_rx: Mutex<Option<mpsc::Receiver<MetricRow>>>,
    pub monitor_results_rx: Mutex<Option<mpsc::Receiver<MonitorResultRow>>>,
    pub events_rx: Mutex<Option<mpsc::Receiver<ProductEventRow>>>,
}

impl IngestChannels {
    pub fn new() -> Self {
        let (logs_tx, logs_rx) = mpsc::channel(32_768);
        let (spans_tx, spans_rx) = mpsc::channel(32_768);
        let (metrics_tx, metrics_rx) = mpsc::channel(32_768);
        let (monitor_results_tx, monitor_results_rx) = mpsc::channel(4_096);
        let (events_tx, events_rx) = mpsc::channel(32_768);
        Self {
            logs_tx,
            spans_tx,
            metrics_tx,
            monitor_results_tx,
            events_tx,
            logs_rx: Mutex::new(Some(logs_rx)),
            spans_rx: Mutex::new(Some(spans_rx)),
            metrics_rx: Mutex::new(Some(metrics_rx)),
            monitor_results_rx: Mutex::new(Some(monitor_results_rx)),
            events_rx: Mutex::new(Some(events_rx)),
        }
    }
}

/// Bus de broadcast para live tailing y notificaciones entre tareas.
///
/// El canal usa `broadcast::channel(N)`: cuando un subscriptor lento se atrasa
/// más de N mensajes, tokio sobrescribe los mensajes viejos y entrega un
/// `RecvError::Lagged(skipped)` en el siguiente `recv()`. Eso es exactamente
/// el comportamiento queremos para SSE — el productor (ingesta) NUNCA se
/// bloquea por un cliente lento; el cliente lento pierde mensajes. Ver
/// `stream::live_logs_sse` donde el `Lagged` se loguea y se descarta.
#[derive(Clone)]
pub struct LiveBus {
    pub logs: broadcast::Sender<LogRow>,
    /// Bus para product events. Mismo modelo que `logs`: cuando un cliente SSE
    /// se atrasa, tokio sobrescribe los mensajes viejos y emite `Lagged(n)` que
    /// `live_events_sse` loguea y descarta — la ingesta jamás se bloquea.
    pub events: broadcast::Sender<ProductEventRow>,
}

impl LiveBus {
    pub fn new() -> Self {
        let (logs, _) = broadcast::channel(1024);
        let (events, _) = broadcast::channel(1024);
        Self { logs, events }
    }
}

/// Contador de subscriptores SSE activos por proyecto, con cap por-proyecto y
/// global. Sin esto, un cliente abriendo tabs en loop o un atacante autenticado
/// puede multiplicar conexiones HTTP de larga duración + receivers de
/// broadcast indefinidamente.
#[derive(Clone)]
pub struct SseSubscriptions {
    inner: Arc<SseSubsInner>,
}

struct SseSubsInner {
    counts: Mutex<HashMap<String, usize>>,
    global: AtomicUsize,
    max_per_project: usize,
    max_global: usize,
}

impl SseSubscriptions {
    pub fn new(max_per_project: usize, max_global: usize) -> Self {
        Self {
            inner: Arc::new(SseSubsInner {
                counts: Mutex::new(HashMap::new()),
                global: AtomicUsize::new(0),
                max_per_project,
                max_global,
            }),
        }
    }

    /// Intenta reservar un slot. Devuelve `Some(SseSlot)` si hay cupo (el Drop
    /// del slot lo libera); `None` si el cliente excede el cap por-proyecto o
    /// el global. Usa "*" como clave cuando no hay filtro de proyecto.
    pub fn try_acquire(&self, project_key: &str) -> Option<SseSlot> {
        let inner = &self.inner;
        // Reservamos primero el slot global con un fetch_add optimista. Si pasa,
        // chequeamos el cap por-proyecto bajo el mutex (donde sí necesitamos
        // atomicidad relativa al HashMap). Si el por-proyecto falla, revertimos
        // el global. Esto evita tomar el mutex para el caso común (global lleno).
        let prev_global = inner.global.fetch_add(1, Ordering::AcqRel);
        if prev_global >= inner.max_global {
            inner.global.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        let mut counts = inner.counts.lock();
        let count = counts.entry(project_key.to_string()).or_insert(0);
        if *count >= inner.max_per_project {
            drop(counts);
            inner.global.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        *count += 1;
        Some(SseSlot {
            subs: self.clone(),
            project: project_key.to_string(),
        })
    }
}

/// Guard RAII: vive el tiempo que dura la conexión SSE (lo retiene el closure
/// del stream). Al dropear, decrementa los contadores.
pub struct SseSlot {
    subs: SseSubscriptions,
    project: String,
}

impl Drop for SseSlot {
    fn drop(&mut self) {
        let inner = &self.subs.inner;
        inner.global.fetch_sub(1, Ordering::AcqRel);
        let mut counts = inner.counts.lock();
        if let Some(c) = counts.get_mut(&self.project) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                counts.remove(&self.project);
            }
        }
    }
}

/// Cache in-memory de secretos TOTP en mitad del flujo de enrolamiento.
/// El user pide `/setup` → backend genera secreto → lo guarda aquí → no persiste
/// nada en DB hasta que el user confirme con `/enable` un código TOTP válido. Si
/// nunca confirma, el secreto muere con el proceso (o con TTL implícito por
/// rotación: cada `/setup` reemplaza el pendiente del user).
#[derive(Clone, Default)]
pub struct PendingTotpSecrets {
    inner: Arc<Mutex<HashMap<uuid::Uuid, String>>>,
}

impl PendingTotpSecrets {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&self, user_id: uuid::Uuid, secret: String) {
        self.inner.lock().insert(user_id, secret);
    }
    pub fn get(&self, user_id: uuid::Uuid) -> Option<String> {
        self.inner.lock().get(&user_id).cloned()
    }
    pub fn clear(&self, user_id: uuid::Uuid) {
        self.inner.lock().remove(&user_id);
    }
}

pub struct AppState {
    pub cfg: Config,
    pub ch: Client,
    pub ingest: IngestChannels,
    pub live_bus: LiveBus,
    pub sse_subs: SseSubscriptions,
    pub projects: ProjectCache,
    pub integrations: IntegrationsCache,
    /// Cache de canales de notificación configurables (webhook/PagerDuty/etc.).
    /// El dispatcher de `notify::dispatch` resuelve targets `channel://<id>` aquí.
    pub notification_channels: NotificationChannelsCache,
    /// Cache de feature flags activas por proyecto. Los SDKs la descargan por token
    /// de ingesta y evalúan localmente con refresh periódico.
    pub feature_flags: FeatureFlagsCache,
    pub limiter: IngestLimiter,
    /// Rate limiter para verificación de códigos TOTP/recovery. 5 intentos/min/user;
    /// sin esto los 6 dígitos son brute-forceables vía API en minutos.
    pub totp_rl: TotpRateLimiter,
    /// Secretos TOTP en mitad del setup, antes de que el user confirme el código.
    /// Ver `PendingTotpSecrets` para el porqué de mantenerlos in-memory.
    pub pending_totp: PendingTotpSecrets,
}

impl AppState {
    pub fn new(cfg: Config, ch: Client) -> Self {
        let limiter = IngestLimiter::new(cfg.ingest_rate_per_second);
        let sse_subs = SseSubscriptions::new(cfg.sse_max_per_project, cfg.sse_max_global);
        Self {
            cfg,
            ch,
            ingest: IngestChannels::new(),
            live_bus: LiveBus::new(),
            sse_subs,
            projects: ProjectCache::new(),
            integrations: IntegrationsCache::new(),
            notification_channels: NotificationChannelsCache::new(),
            feature_flags: FeatureFlagsCache::new(),
            limiter,
            totp_rl: TotpRateLimiter::new(),
            pending_totp: PendingTotpSecrets::new(),
        }
    }
}

pub type SharedState = Arc<AppState>;
