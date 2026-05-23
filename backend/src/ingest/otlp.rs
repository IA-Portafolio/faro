//! OTLP/HTTP+JSON receivers for logs, traces and metrics.
//! Endpoints mirror the spec: POST /v1/logs, /v1/traces, /v1/metrics.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, TimeZone, Utc};

use super::otlp_types::*;
use crate::error::ApiError;
use crate::state::SharedState;
use crate::storage::{LogRow, MetricRow, SpanRow};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/v1/logs", post(ingest_logs))
        .route("/v1/traces", post(ingest_traces))
        .route("/v1/metrics", post(ingest_metrics))
}

fn nano_to_dt(n: u64) -> DateTime<Utc> {
    let secs = (n / 1_000_000_000) as i64;
    let nsec = (n % 1_000_000_000) as u32;
    Utc.timestamp_opt(secs, nsec).single().unwrap_or_else(Utc::now)
}

fn ok() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"partialSuccess": {}})))
}

async fn ingest_logs(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<ExportLogsRequest>,
) -> Result<axum::response::Response, ApiError> {
    let project = super::resolve_project(&state, &headers)?;
    let now = Utc::now();
    let mut accepted = 0u64;

    for rl in req.resource_logs {
        let svc = service_name(&rl.resource);
        let res_attrs = rl.resource.as_ref().map(|r| attrs_to_map(&r.attributes)).unwrap_or_default();
        for sl in rl.scope_logs {
            let scope_name = sl.scope.as_ref().and_then(|s| s.name.clone()).unwrap_or_default();
            for lr in sl.log_records {
                let ts = lr.time_unix_nano.as_ref().map(|x| nano_to_dt(x.0)).unwrap_or(now);
                let obs = lr.observed_time_unix_nano.as_ref().map(|x| nano_to_dt(x.0)).unwrap_or(ts);
                let body = lr.body.as_ref().map(|v| v.to_string_value()).unwrap_or_default();
                let sev_text = lr.severity_text.unwrap_or_else(|| "INFO".into());
                let sev_num = lr.severity_number.unwrap_or_else(|| LogRow::severity_from_text(&sev_text));
                let row = LogRow {
                    timestamp: ts,
                    observed_timestamp: obs,
                    project_id: project.clone(),
                    service_name: svc.clone(),
                    severity_text: sev_text,
                    severity_number: sev_num,
                    body,
                    trace_id: lr.trace_id.unwrap_or_default(),
                    span_id: lr.span_id.unwrap_or_default(),
                    scope_name: scope_name.clone(),
                    resource_attributes: res_attrs.clone(),
                    attributes: attrs_to_map(&lr.attributes),
                };
                let _ = state.live_bus.logs.send(row.clone());
                if state.ingest.logs_tx.try_send(row).is_ok() {
                    accepted += 1;
                }
            }
        }
    }
    tracing::debug!(accepted, "logs otlp ingestados");
    Ok(ok().into_response())
}

async fn ingest_traces(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<ExportTracesRequest>,
) -> Result<axum::response::Response, ApiError> {
    let project = super::resolve_project(&state, &headers)?;
    let mut accepted = 0u64;

    for rs in req.resource_spans {
        let svc = service_name(&rs.resource);
        let res_attrs = rs.resource.as_ref().map(|r| attrs_to_map(&r.attributes)).unwrap_or_default();
        for ss in rs.scope_spans {
            for sp in ss.spans {
                let start = nano_to_dt(sp.start_time_unix_nano.0);
                let end_ns = sp.end_time_unix_nano.0;
                let start_ns = sp.start_time_unix_nano.0;
                let duration_ns = end_ns.saturating_sub(start_ns);
                let kind = match sp.kind.unwrap_or(0) {
                    1 => "INTERNAL",
                    2 => "SERVER",
                    3 => "CLIENT",
                    4 => "PRODUCER",
                    5 => "CONSUMER",
                    _ => "UNSPECIFIED",
                };
                let (status_code, status_msg) = match sp.status {
                    Some(s) => {
                        let code = match s.code.unwrap_or(0) {
                            1 => "OK",
                            2 => "ERROR",
                            _ => "UNSET",
                        };
                        (code.to_string(), s.message.unwrap_or_default())
                    }
                    None => ("UNSET".into(), String::new()),
                };
                let events_timestamps: Vec<String> = sp
                    .events
                    .iter()
                    .map(|e| nano_to_dt(e.time_unix_nano.0).to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
                    .collect();
                let events_names: Vec<String> = sp.events.iter().map(|e| e.name.clone()).collect();
                let events_attributes: Vec<String> = sp
                    .events
                    .iter()
                    .map(|e| serde_json::to_string(&attrs_to_map(&e.attributes)).unwrap_or_default())
                    .collect();
                let links_trace_ids: Vec<String> = sp.links.iter().map(|l| l.trace_id.clone()).collect();
                let links_span_ids: Vec<String> = sp.links.iter().map(|l| l.span_id.clone()).collect();

                let row = SpanRow {
                    timestamp: start,
                    project_id: project.clone(),
                    trace_id: sp.trace_id,
                    span_id: sp.span_id,
                    parent_span_id: sp.parent_span_id.unwrap_or_default(),
                    trace_state: sp.trace_state.unwrap_or_default(),
                    name: sp.name,
                    kind: kind.into(),
                    service_name: svc.clone(),
                    duration_ns,
                    status_code,
                    status_message: status_msg,
                    resource_attributes: res_attrs.clone(),
                    span_attributes: attrs_to_map(&sp.attributes),
                    events_timestamps,
                    events_names,
                    events_attributes,
                    links_trace_ids,
                    links_span_ids,
                };
                if state.ingest.spans_tx.try_send(row).is_ok() {
                    accepted += 1;
                }
            }
        }
    }
    tracing::debug!(accepted, "spans otlp ingestados");
    Ok(ok().into_response())
}

async fn ingest_metrics(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<ExportMetricsRequest>,
) -> Result<axum::response::Response, ApiError> {
    let project = super::resolve_project(&state, &headers)?;
    let mut accepted = 0u64;

    for rm in req.resource_metrics {
        let svc = service_name(&rm.resource);
        let res_attrs = rm.resource.as_ref().map(|r| attrs_to_map(&r.attributes)).unwrap_or_default();

        for sm in rm.scope_metrics {
            for m in sm.metrics {
                let unit = m.unit.unwrap_or_default();
                if let Some(g) = m.gauge {
                    for dp in g.data_points {
                        push_number(state.clone(), &project, &m.name, "gauge", &unit, &svc, &res_attrs, dp).await;
                        accepted += 1;
                    }
                }
                if let Some(s) = m.sum {
                    let kind = if s.is_monotonic.unwrap_or(false) { "counter" } else { "sum" };
                    for dp in s.data_points {
                        push_number(state.clone(), &project, &m.name, kind, &unit, &svc, &res_attrs, dp).await;
                        accepted += 1;
                    }
                }
                if let Some(h) = m.histogram {
                    for dp in h.data_points {
                        let ts = dp.time_unix_nano.as_ref().map(|x| nano_to_dt(x.0)).unwrap_or_else(Utc::now);
                        let row = MetricRow {
                            timestamp: ts,
                            project_id: project.clone(),
                            metric_name: m.name.clone(),
                            metric_type: "histogram".into(),
                            metric_unit: unit.clone(),
                            service_name: svc.clone(),
                            value: dp.sum.unwrap_or(0.0),
                            resource_attributes: res_attrs.clone(),
                            attributes: attrs_to_map(&dp.attributes),
                            hist_count: dp.count.as_ref().map(|c| c.0).unwrap_or(0),
                            hist_sum: dp.sum.unwrap_or(0.0),
                            hist_min: dp.min.unwrap_or(0.0),
                            hist_max: dp.max.unwrap_or(0.0),
                            hist_bucket_bounds: dp.explicit_bounds.clone(),
                            hist_bucket_counts: dp.bucket_counts.iter().map(|c| c.0).collect(),
                        };
                        if state.ingest.metrics_tx.try_send(row).is_ok() {
                            accepted += 1;
                        }
                    }
                }
                if let Some(sm) = m.summary {
                    for dp in sm.data_points {
                        let ts = dp.time_unix_nano.as_ref().map(|x| nano_to_dt(x.0)).unwrap_or_else(Utc::now);
                        let count = dp.count.as_ref().map(|c| c.0).unwrap_or(0);
                        let sum = dp.sum.unwrap_or(0.0);
                        let avg = if count > 0 { sum / (count as f64) } else { 0.0 };
                        let row = MetricRow {
                            timestamp: ts,
                            project_id: project.clone(),
                            metric_name: m.name.clone(),
                            metric_type: "summary".into(),
                            metric_unit: unit.clone(),
                            service_name: svc.clone(),
                            value: avg,
                            resource_attributes: res_attrs.clone(),
                            attributes: attrs_to_map(&dp.attributes),
                            hist_count: count,
                            hist_sum: sum,
                            hist_min: 0.0,
                            hist_max: 0.0,
                            hist_bucket_bounds: vec![],
                            hist_bucket_counts: vec![],
                        };
                        if state.ingest.metrics_tx.try_send(row).is_ok() {
                            accepted += 1;
                        }
                    }
                }
            }
        }
    }
    tracing::debug!(accepted, "métricas otlp ingestadas");
    Ok(ok().into_response())
}

#[allow(clippy::too_many_arguments)]
async fn push_number(
    state: SharedState,
    project: &str,
    name: &str,
    kind: &str,
    unit: &str,
    svc: &str,
    res_attrs: &crate::storage::AttrMap,
    dp: NumberDataPoint,
) {
    let ts = dp.time_unix_nano.as_ref().map(|x| nano_to_dt(x.0)).unwrap_or_else(Utc::now);
    let value = if let Some(v) = dp.as_double {
        v
    } else if let Some(i) = &dp.as_int {
        i.0 as f64
    } else {
        0.0
    };
    let row = MetricRow {
        timestamp: ts,
        project_id: project.into(),
        metric_name: name.into(),
        metric_type: kind.into(),
        metric_unit: unit.into(),
        service_name: svc.into(),
        value,
        resource_attributes: res_attrs.clone(),
        attributes: attrs_to_map(&dp.attributes),
        hist_count: 0,
        hist_sum: 0.0,
        hist_min: 0.0,
        hist_max: 0.0,
        hist_bucket_bounds: vec![],
        hist_bucket_counts: vec![],
    };
    let _ = state.ingest.metrics_tx.try_send(row);
}
