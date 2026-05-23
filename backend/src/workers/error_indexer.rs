use std::time::Duration;

use tokio::time::{interval, MissedTickBehavior};

use crate::fingerprint::fingerprint;
use crate::state::{LiveBus, SharedState};
use crate::storage::{AttrMap, ErrorEventRow, LogRow};

/// Subscribes to the live logs broadcast bus, detects error-level records, computes
/// a fingerprint, and writes them to faro.error_events. Counts are computed at read
/// time from that table — no separate counter needs to be maintained.
pub fn start_error_indexer(state: SharedState, bus: LiveBus) {
    let mut rx = bus.logs.subscribe();
    let ch = state.ch.clone();

    tokio::spawn(async move {
        let mut buf: Vec<ErrorEventRow> = Vec::with_capacity(512);
        let mut tick = interval(Duration::from_millis(750));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Ok(log) => {
                            if let Some(ev) = log_to_error(&log) {
                                buf.push(ev);
                                if buf.len() >= 500 {
                                    flush(&ch, &mut buf).await;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(missed = n, "error indexer lagged behind log bus");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = tick.tick() => {
                    if !buf.is_empty() {
                        flush(&ch, &mut buf).await;
                    }
                }
            }
        }
    });
}

fn log_to_error(log: &LogRow) -> Option<ErrorEventRow> {
    // Only consider WARN+ entries; treat ERROR/FATAL as definite errors. WARN is
    // included only if it carries an exception.* attribute (OTel convention).
    if log.severity_number < 13 {
        return None;
    }

    let exc_type = log
        .attributes
        .get("exception.type")
        .cloned()
        .unwrap_or_default();
    let exc_msg = log
        .attributes
        .get("exception.message")
        .cloned()
        .unwrap_or_else(|| log.body.clone());
    let stack = log
        .attributes
        .get("exception.stacktrace")
        .cloned()
        .unwrap_or_default();

    if log.severity_number < 17 && exc_type.is_empty() && stack.is_empty() {
        return None;
    }

    let fp = fingerprint(&exc_type, &exc_msg, &stack);
    let mut attrs = AttrMap::new();
    for (k, v) in &log.attributes {
        if !k.starts_with("exception.") {
            attrs.insert(k.clone(), v.clone());
        }
    }
    Some(ErrorEventRow {
        timestamp: log.timestamp,
        project_id: log.project_id.clone(),
        fingerprint: fp,
        service_name: log.service_name.clone(),
        severity_text: log.severity_text.clone(),
        message: log.body.clone(),
        exception_type: exc_type,
        exception_message: exc_msg,
        stack_trace: stack,
        trace_id: log.trace_id.clone(),
        span_id: log.span_id.clone(),
        attributes: attrs,
    })
}

async fn flush(ch: &crate::storage::Client, buf: &mut Vec<ErrorEventRow>) {
    if buf.is_empty() {
        return;
    }
    let n = buf.len();
    if let Err(e) = ch.insert("faro.error_events", buf).await {
        tracing::error!(rows = n, error = %e, "error_events flush failed");
    } else {
        tracing::debug!(rows = n, "error_events flushed");
    }
    buf.clear();
}
