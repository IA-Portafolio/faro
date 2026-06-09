//! OTLP/gRPC receivers para logs, traces y métricas en `:4317`.
//!
//! Los SDKs oficiales de OpenTelemetry (Java, Go, Python, .NET, Ruby, ...) usan
//! por defecto OTLP sobre gRPC+protobuf. Sin este endpoint un cliente "stock"
//! falla en silencio o tira `connection refused`. Reusamos los mismos canales y
//! filas de storage que el path HTTP/JSON ([`super::otlp`]); lo único distinto
//! es el decoder (prost en vez de serde_json).

use std::net::SocketAddr;

use chrono::{DateTime, TimeZone, Utc};
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::{
            logs_service_server::{LogsService, LogsServiceServer},
            ExportLogsServiceRequest, ExportLogsServiceResponse,
        },
        metrics::v1::{
            metrics_service_server::{MetricsService, MetricsServiceServer},
            ExportMetricsServiceRequest, ExportMetricsServiceResponse,
        },
        trace::v1::{
            trace_service_server::{TraceService, TraceServiceServer},
            ExportTraceServiceRequest, ExportTraceServiceResponse,
        },
    },
    common::v1::{any_value, AnyValue, KeyValue},
    metrics::v1::{metric, number_data_point, NumberDataPoint},
    resource::v1::Resource,
};
use tonic::{metadata::MetadataMap, transport::Server, Request, Response, Status as TonicStatus};

use super::rate_limit::LimitOutcome;
use crate::observability::names;
use crate::state::SharedState;
use crate::storage::{AttrMap, LogRow, MetricRow, SpanRow};

/// Levanta los tres servicios OTLP sobre tonic. Bloquea hasta que el servidor
/// termine; pensado para correr en una tarea tokio dedicada desde `main`.
pub async fn serve(state: SharedState, addr: SocketAddr) -> anyhow::Result<()> {
    let logs = FaroLogsService {
        state: state.clone(),
    };
    let traces = FaroTraceService {
        state: state.clone(),
    };
    let metrics = FaroMetricsService { state };

    tracing::info!(%addr, "escuchando otlp/grpc");
    Server::builder()
        .add_service(LogsServiceServer::new(logs))
        .add_service(TraceServiceServer::new(traces))
        .add_service(MetricsServiceServer::new(metrics))
        .serve(addr)
        .await?;
    Ok(())
}

// ---------- Auth ----------

fn resolve_project(state: &SharedState, meta: &MetadataMap) -> Result<String, TonicStatus> {
    let token =
        extract_token(meta).ok_or_else(|| TonicStatus::unauthenticated("missing bearer token"))?;
    state
        .projects
        .lookup(&token)
        .ok_or_else(|| TonicStatus::unauthenticated("unknown project token"))
}

fn extract_token(meta: &MetadataMap) -> Option<String> {
    if let Some(v) = meta.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(rest) = v.strip_prefix("Bearer ") {
            return Some(rest.trim().to_string());
        }
    }
    if let Some(v) = meta.get("x-faro-token").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    None
}

/// Convierte el chequeo del limiter en `Status::resource_exhausted` cuando
/// bloquea, añadiendo `retry-after` como metadata custom — equivalente al
/// header HTTP. tonic la expone al cliente como trailer.
fn enforce_limit(
    state: &SharedState,
    project: &str,
    signal: &'static str,
    n: u32,
) -> Result<(), TonicStatus> {
    match state.limiter.check(project, n) {
        LimitOutcome::Allowed => Ok(()),
        other => {
            let secs = other.retry_after_secs();
            tracing::warn!(
                project,
                signal,
                records = n,
                retry_after_secs = secs,
                "ingest OTLP/gRPC rate-limited"
            );
            metrics::counter!(
                names::RATE_LIMITED,
                "project" => project.to_string(),
                "signal" => signal,
            )
            .increment(1);
            metrics::counter!(
                names::INGEST_RECORDS,
                "project" => project.to_string(),
                "signal" => signal,
                "outcome" => "rate_limited",
            )
            .increment(n as u64);
            let mut status = TonicStatus::resource_exhausted(format!(
                "rate limit por proyecto excedido (reintenta en {secs}s)"
            ));
            if let Ok(v) = secs.to_string().parse() {
                status.metadata_mut().insert("retry-after", v);
            }
            Err(status)
        }
    }
}

fn record_accepted(project: &str, signal: &'static str, accepted: u64) {
    if accepted == 0 {
        return;
    }
    metrics::counter!(
        names::INGEST_RECORDS,
        "project" => project.to_string(),
        "signal" => signal,
        "outcome" => "accepted",
    )
    .increment(accepted);
}

// ---------- Conversores prost → storage ----------

fn nano_to_dt(n: u64) -> DateTime<Utc> {
    let secs = (n / 1_000_000_000) as i64;
    let nsec = (n % 1_000_000_000) as u32;
    Utc.timestamp_opt(secs, nsec)
        .single()
        .unwrap_or_else(Utc::now)
}

fn any_to_string(v: &AnyValue) -> String {
    match &v.value {
        Some(any_value::Value::StringValue(s)) => s.clone(),
        Some(any_value::Value::BoolValue(b)) => b.to_string(),
        Some(any_value::Value::IntValue(i)) => i.to_string(),
        Some(any_value::Value::DoubleValue(d)) => d.to_string(),
        Some(any_value::Value::BytesValue(b)) => hex::encode(b),
        Some(any_value::Value::ArrayValue(a)) => {
            let parts: Vec<String> = a.values.iter().map(any_to_string).collect();
            format!("[{}]", parts.join(","))
        }
        Some(any_value::Value::KvlistValue(kv)) => {
            let parts: Vec<String> = kv
                .values
                .iter()
                .map(|p| {
                    let val = p.value.as_ref().map(any_to_string).unwrap_or_default();
                    format!("\"{}\":\"{}\"", p.key, val)
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        // `StringValueStrindex` (otel-proto ≥0.32) referencia un string por índice
        // en la string-table del request (encoding de diccionario de OTLP). No
        // tenemos esa tabla en este punto de conversión, así que lo tratamos como
        // ausente — igual que `None`.
        Some(any_value::Value::StringValueStrindex(_)) => String::new(),
        None => String::new(),
    }
}

fn attrs_to_map(attrs: &[KeyValue]) -> AttrMap {
    let mut out = AttrMap::new();
    for kv in attrs {
        if let Some(v) = &kv.value {
            out.insert(kv.key.clone(), any_to_string(v));
        }
    }
    out
}

fn service_name(res: &Option<Resource>) -> String {
    if let Some(r) = res {
        for kv in &r.attributes {
            if kv.key == "service.name" {
                if let Some(v) = &kv.value {
                    return any_to_string(v);
                }
            }
        }
    }
    "unknown".into()
}

fn id_to_hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        String::new()
    } else {
        hex::encode(bytes)
    }
}

fn span_kind_str(kind: i32) -> &'static str {
    match kind {
        1 => "INTERNAL",
        2 => "SERVER",
        3 => "CLIENT",
        4 => "PRODUCER",
        5 => "CONSUMER",
        _ => "UNSPECIFIED",
    }
}

fn status_code_str(code: i32) -> &'static str {
    match code {
        1 => "OK",
        2 => "ERROR",
        _ => "UNSET",
    }
}

fn number_dp_value(dp: &NumberDataPoint) -> f64 {
    match &dp.value {
        Some(number_data_point::Value::AsDouble(v)) => *v,
        Some(number_data_point::Value::AsInt(i)) => *i as f64,
        None => 0.0,
    }
}

fn count_log_records(req: &ExportLogsServiceRequest) -> u32 {
    let n: usize = req
        .resource_logs
        .iter()
        .flat_map(|rl| rl.scope_logs.iter())
        .map(|sl| sl.log_records.len())
        .sum();
    n.try_into().unwrap_or(u32::MAX)
}

fn count_spans(req: &ExportTraceServiceRequest) -> u32 {
    let n: usize = req
        .resource_spans
        .iter()
        .flat_map(|rs| rs.scope_spans.iter())
        .map(|ss| ss.spans.len())
        .sum();
    n.try_into().unwrap_or(u32::MAX)
}

fn count_metric_dps(req: &ExportMetricsServiceRequest) -> u32 {
    let mut n: usize = 0;
    for rm in &req.resource_metrics {
        for sm in &rm.scope_metrics {
            for m in &sm.metrics {
                match &m.data {
                    Some(metric::Data::Gauge(g)) => n += g.data_points.len(),
                    Some(metric::Data::Sum(s)) => n += s.data_points.len(),
                    Some(metric::Data::Histogram(h)) => n += h.data_points.len(),
                    Some(metric::Data::Summary(s)) => n += s.data_points.len(),
                    Some(metric::Data::ExponentialHistogram(_)) | None => {}
                }
            }
        }
    }
    n.try_into().unwrap_or(u32::MAX)
}

// ---------- Servicios ----------

struct FaroLogsService {
    state: SharedState,
}

#[tonic::async_trait]
impl LogsService for FaroLogsService {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, TonicStatus> {
        let project = resolve_project(&self.state, request.metadata())?;
        let req = request.into_inner();
        enforce_limit(&self.state, &project, "logs", count_log_records(&req))?;
        let now = Utc::now();
        let mut accepted = 0u64;
        let redaction_rules = self.state.projects.redaction(&project);

        for rl in req.resource_logs {
            let svc = service_name(&rl.resource);
            let res_attrs = rl
                .resource
                .as_ref()
                .map(|r| attrs_to_map(&r.attributes))
                .unwrap_or_default();
            for sl in rl.scope_logs {
                let scope_name = sl
                    .scope
                    .as_ref()
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                for lr in sl.log_records {
                    let ts = if lr.time_unix_nano > 0 {
                        nano_to_dt(lr.time_unix_nano)
                    } else {
                        now
                    };
                    let obs = if lr.observed_time_unix_nano > 0 {
                        nano_to_dt(lr.observed_time_unix_nano)
                    } else {
                        ts
                    };
                    let body = lr.body.as_ref().map(any_to_string).unwrap_or_default();
                    let sev_text = if lr.severity_text.is_empty() {
                        "INFO".to_string()
                    } else {
                        lr.severity_text.clone()
                    };
                    let sev_num = if lr.severity_number != 0 {
                        lr.severity_number as u8
                    } else {
                        LogRow::severity_from_text(&sev_text)
                    };
                    let mut row = LogRow {
                        timestamp: ts,
                        observed_timestamp: obs,
                        project_id: project.clone(),
                        service_name: svc.clone(),
                        severity_text: sev_text,
                        severity_number: sev_num,
                        body,
                        trace_id: id_to_hex(&lr.trace_id),
                        span_id: id_to_hex(&lr.span_id),
                        scope_name: scope_name.clone(),
                        resource_attributes: res_attrs.clone(),
                        attributes: attrs_to_map(&lr.attributes),
                    };
                    super::redact_log(redaction_rules.as_ref(), &mut row);
                    let _ = self.state.live_bus.logs.send(row.clone());
                    if self.state.ingest.logs_tx.try_send(row).is_ok() {
                        accepted += 1;
                    } else {
                        crate::observability::record_ingest_drop("logs");
                    }
                }
            }
        }
        record_accepted(&project, "logs", accepted);
        tracing::debug!(accepted, "logs otlp/grpc ingestados");

        Ok(Response::new(ExportLogsServiceResponse {
            partial_success: None,
        }))
    }
}

struct FaroTraceService {
    state: SharedState,
}

#[tonic::async_trait]
impl TraceService for FaroTraceService {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, TonicStatus> {
        let project = resolve_project(&self.state, request.metadata())?;
        let req = request.into_inner();
        enforce_limit(&self.state, &project, "traces", count_spans(&req))?;
        let mut accepted = 0u64;
        let redaction_rules = self.state.projects.redaction(&project);

        for rs in req.resource_spans {
            let svc = service_name(&rs.resource);
            let res_attrs = rs
                .resource
                .as_ref()
                .map(|r| attrs_to_map(&r.attributes))
                .unwrap_or_default();
            for ss in rs.scope_spans {
                for sp in ss.spans {
                    let start = nano_to_dt(sp.start_time_unix_nano);
                    let duration_ns = sp
                        .end_time_unix_nano
                        .saturating_sub(sp.start_time_unix_nano);
                    let kind = span_kind_str(sp.kind);
                    let (status_code, status_message) = match sp.status {
                        Some(s) => (status_code_str(s.code).to_string(), s.message),
                        None => ("UNSET".to_string(), String::new()),
                    };
                    let events_timestamps: Vec<String> = sp
                        .events
                        .iter()
                        .map(|e| {
                            nano_to_dt(e.time_unix_nano)
                                .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
                        })
                        .collect();
                    let events_names: Vec<String> =
                        sp.events.iter().map(|e| e.name.clone()).collect();
                    let events_attributes: Vec<String> = sp
                        .events
                        .iter()
                        .map(|e| {
                            serde_json::to_string(&attrs_to_map(&e.attributes)).unwrap_or_default()
                        })
                        .collect();
                    let links_trace_ids: Vec<String> =
                        sp.links.iter().map(|l| id_to_hex(&l.trace_id)).collect();
                    let links_span_ids: Vec<String> =
                        sp.links.iter().map(|l| id_to_hex(&l.span_id)).collect();

                    let mut row = SpanRow {
                        timestamp: start,
                        project_id: project.clone(),
                        trace_id: id_to_hex(&sp.trace_id),
                        span_id: id_to_hex(&sp.span_id),
                        parent_span_id: id_to_hex(&sp.parent_span_id),
                        trace_state: sp.trace_state,
                        name: sp.name,
                        kind: kind.into(),
                        service_name: svc.clone(),
                        duration_ns,
                        status_code,
                        status_message,
                        resource_attributes: res_attrs.clone(),
                        span_attributes: attrs_to_map(&sp.attributes),
                        events_timestamps,
                        events_names,
                        events_attributes,
                        links_trace_ids,
                        links_span_ids,
                    };
                    super::redact_span(redaction_rules.as_ref(), &mut row);
                    if self.state.ingest.spans_tx.try_send(row).is_ok() {
                        accepted += 1;
                    } else {
                        crate::observability::record_ingest_drop("traces");
                    }
                }
            }
        }
        record_accepted(&project, "traces", accepted);
        tracing::debug!(accepted, "spans otlp/grpc ingestados");

        Ok(Response::new(ExportTraceServiceResponse {
            partial_success: None,
        }))
    }
}

struct FaroMetricsService {
    state: SharedState,
}

#[tonic::async_trait]
impl MetricsService for FaroMetricsService {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, TonicStatus> {
        let project = resolve_project(&self.state, request.metadata())?;
        let req = request.into_inner();
        enforce_limit(&self.state, &project, "metrics", count_metric_dps(&req))?;
        let mut accepted = 0u64;

        for rm in req.resource_metrics {
            let svc = service_name(&rm.resource);
            let res_attrs = rm
                .resource
                .as_ref()
                .map(|r| attrs_to_map(&r.attributes))
                .unwrap_or_default();

            for sm in rm.scope_metrics {
                for m in sm.metrics {
                    let unit = m.unit;
                    match m.data {
                        Some(metric::Data::Gauge(g)) => {
                            for dp in g.data_points {
                                push_number(
                                    &self.state,
                                    &project,
                                    &m.name,
                                    "gauge",
                                    &unit,
                                    &svc,
                                    &res_attrs,
                                    dp,
                                );
                                accepted += 1;
                            }
                        }
                        Some(metric::Data::Sum(s)) => {
                            let kind = if s.is_monotonic { "counter" } else { "sum" };
                            for dp in s.data_points {
                                push_number(
                                    &self.state,
                                    &project,
                                    &m.name,
                                    kind,
                                    &unit,
                                    &svc,
                                    &res_attrs,
                                    dp,
                                );
                                accepted += 1;
                            }
                        }
                        Some(metric::Data::Histogram(h)) => {
                            for dp in h.data_points {
                                let ts = if dp.time_unix_nano > 0 {
                                    nano_to_dt(dp.time_unix_nano)
                                } else {
                                    Utc::now()
                                };
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
                                    hist_count: dp.count,
                                    hist_sum: dp.sum.unwrap_or(0.0),
                                    hist_min: dp.min.unwrap_or(0.0),
                                    hist_max: dp.max.unwrap_or(0.0),
                                    hist_bucket_bounds: dp.explicit_bounds.clone(),
                                    hist_bucket_counts: dp.bucket_counts.clone(),
                                };
                                if self.state.ingest.metrics_tx.try_send(row).is_ok() {
                                    accepted += 1;
                                } else {
                                    crate::observability::record_ingest_drop("metrics");
                                }
                            }
                        }
                        Some(metric::Data::Summary(s)) => {
                            for dp in s.data_points {
                                let ts = if dp.time_unix_nano > 0 {
                                    nano_to_dt(dp.time_unix_nano)
                                } else {
                                    Utc::now()
                                };
                                let avg = if dp.count > 0 {
                                    dp.sum / (dp.count as f64)
                                } else {
                                    0.0
                                };
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
                                    hist_count: dp.count,
                                    hist_sum: dp.sum,
                                    hist_min: 0.0,
                                    hist_max: 0.0,
                                    hist_bucket_bounds: vec![],
                                    hist_bucket_counts: vec![],
                                };
                                if self.state.ingest.metrics_tx.try_send(row).is_ok() {
                                    accepted += 1;
                                } else {
                                    crate::observability::record_ingest_drop("metrics");
                                }
                            }
                        }
                        // ExponentialHistogram queda fuera de scope: el path HTTP/JSON
                        // tampoco lo soporta y el storage no tiene buckets exponenciales.
                        Some(metric::Data::ExponentialHistogram(_)) | None => {}
                    }
                }
            }
        }
        record_accepted(&project, "metrics", accepted);
        tracing::debug!(accepted, "métricas otlp/grpc ingestadas");

        Ok(Response::new(ExportMetricsServiceResponse {
            partial_success: None,
        }))
    }
}

#[allow(clippy::too_many_arguments)]
fn push_number(
    state: &SharedState,
    project: &str,
    name: &str,
    kind: &str,
    unit: &str,
    svc: &str,
    res_attrs: &AttrMap,
    dp: NumberDataPoint,
) {
    let ts = if dp.time_unix_nano > 0 {
        nano_to_dt(dp.time_unix_nano)
    } else {
        Utc::now()
    };
    let value = number_dp_value(&dp);
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
    if state.ingest.metrics_tx.try_send(row).is_err() {
        crate::observability::record_ingest_drop("metrics");
    }
}
