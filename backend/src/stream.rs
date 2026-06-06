//! Streaming SSE (Server-Sent Events) de logs y product events en vivo.
//!
//! Convierte un receptor `broadcast` en un stream SSE filtrado por proyecto. Gestiona
//! el backpressure (un cliente lento recibe `Lagged` y pierde mensajes, pero la
//! conexión sigue) y libera su "slot" de subscripción al desconectar.

use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use regex::Regex;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::state::SseSlot;
use crate::storage::{LogRow, ProductEventRow};

/// Convierte un receptor broadcast en un stream SSE.
///
/// Comportamiento de backpressure:
/// - El productor (`live_bus.logs.send(...)`) nunca se bloquea: cuando un cliente
///   se atrasa más allá de la capacidad del broadcast channel (1024), tokio
///   sobrescribe los mensajes viejos y le marca al receiver `Lagged(skipped)`.
/// - Aquí transformamos ese `Lagged` en un `tracing::warn!` y descartamos el
///   item — el cliente lento pierde mensajes pero la conexión continúa.
/// - `slot` se mueve al closure: cuando el stream se dropea (cliente desconecta
///   o el response future termina), el Drop de `SseSlot` decrementa el contador
///   de subscripciones, liberando el slot para el siguiente cliente.
pub fn live_logs_sse(
    rx: tokio::sync::broadcast::Receiver<LogRow>,
    filter: Option<LogFilter>,
    slot: SseSlot,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(rx).filter_map(move |r| {
        // Capturamos `slot` por move para que viva mientras viva el stream.
        let _keep = &slot;
        match r {
            Ok(row) => {
                if let Some(f) = &filter {
                    if !f.matches(&row) {
                        return None;
                    }
                }
                let payload = serde_json::to_string(&row).ok()?;
                Some(Ok(Event::default().event("log").data(payload)))
            }
            Err(BroadcastStreamRecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "SSE subscriber lagged, mensajes descartados");
                None
            }
        }
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/// Modo de búsqueda dentro del body del log. La variante `Substring` viene
/// ya en minúsculas para evitar normalizar en cada matching.
#[derive(Clone, Debug)]
pub enum BodyMatcher {
    Substring(String),
    Regex(Regex),
}

#[derive(Clone, Debug, Default)]
pub struct LogFilter {
    pub project: Option<String>,
    pub service: Option<String>,
    pub min_severity: Option<u8>,
    pub body: Option<BodyMatcher>,
}

impl LogFilter {
    pub fn matches(&self, row: &LogRow) -> bool {
        if let Some(p) = &self.project {
            if &row.project_id != p {
                return false;
            }
        }
        if let Some(svc) = &self.service {
            if &row.service_name != svc {
                return false;
            }
        }
        if let Some(min) = self.min_severity {
            if row.severity_number < min {
                return false;
            }
        }
        match &self.body {
            Some(BodyMatcher::Substring(needle)) => {
                row.body.to_lowercase().contains(needle.as_str())
            }
            Some(BodyMatcher::Regex(re)) => re.is_match(&row.body),
            None => true,
        }
    }
}

// ---------- Product events live tail ----------

/// Filtros aplicables al stream de product events. Mismas reglas que `LogFilter`:
/// igualdad estricta donde tiene sentido; las queries por properties.X = Y se
/// resuelven en el servidor (`api/events.rs`), no aquí — el SSE solo expone
/// filtros baratos por columna.
#[derive(Clone, Debug, Default)]
pub struct EventFilter {
    pub project: Option<String>,
    pub event_name: Option<String>,
    pub distinct_id: Option<String>,
    pub trace_id: Option<String>,
    pub source: Option<String>,
}

impl EventFilter {
    pub fn matches(&self, row: &ProductEventRow) -> bool {
        if let Some(p) = &self.project {
            if &row.project_id != p {
                return false;
            }
        }
        if let Some(ev) = &self.event_name {
            if &row.event_name != ev {
                return false;
            }
        }
        if let Some(d) = &self.distinct_id {
            if &row.distinct_id != d {
                return false;
            }
        }
        if let Some(t) = &self.trace_id {
            if &row.trace_id != t {
                return false;
            }
        }
        if let Some(s) = &self.source {
            if &row.source != s {
                return false;
            }
        }
        true
    }
}

/// Versión events de `live_logs_sse`. Maneja `Lagged` con el mismo patrón:
/// loguea el skip y descarta el item para que la conexión no muera por un
/// cliente lento. El `SseSlot` se mueve dentro del closure y libera su contador
/// al dropearse el stream (cliente desconectó).
pub fn live_events_sse(
    rx: tokio::sync::broadcast::Receiver<ProductEventRow>,
    filter: Option<EventFilter>,
    slot: SseSlot,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(rx).filter_map(move |r| {
        let _keep = &slot;
        match r {
            Ok(row) => {
                if let Some(f) = &filter {
                    if !f.matches(&row) {
                        return None;
                    }
                }
                let payload = serde_json::to_string(&row).ok()?;
                Some(Ok(Event::default().event("event").data(payload)))
            }
            Err(BroadcastStreamRecvError::Lagged(n)) => {
                tracing::warn!(
                    skipped = n,
                    "SSE subscriber lagged en events, mensajes descartados"
                );
                None
            }
        }
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}
