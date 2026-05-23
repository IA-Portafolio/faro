use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::storage::LogRow;

/// Convert a broadcast receiver into an SSE stream, dropping lagged messages.
pub fn live_logs_sse(
    rx: tokio::sync::broadcast::Receiver<LogRow>,
    filter: Option<LogFilter>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(rx)
        .filter_map(move |r| {
            let row = r.ok()?;
            if let Some(f) = &filter {
                if !f.matches(&row) {
                    return None;
                }
            }
            let payload = serde_json::to_string(&row).ok()?;
            Some(Ok(Event::default().event("log").data(payload)))
        });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

#[derive(Clone, Debug)]
pub struct LogFilter {
    pub service: Option<String>,
    pub min_severity: Option<u8>,
    pub query: Option<String>,
}

impl LogFilter {
    pub fn matches(&self, row: &LogRow) -> bool {
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
        if let Some(q) = &self.query {
            let needle = q.to_lowercase();
            if !row.body.to_lowercase().contains(&needle) {
                return false;
            }
        }
        true
    }
}
