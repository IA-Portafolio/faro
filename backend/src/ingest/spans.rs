//! Ingest nativo para spans (sin OTLP).
//!
//! Equivalente a `ingest::metrics` para tracing: los SDKs `@iaportafolio/*`
//! envían spans con `start` / `end` y atributos plain JSON, y aquí se traducen
//! a `SpanRow` para escribir en `faro.spans`. La ruta OTLP/HTTP+gRPC (`/v1/traces`)
//! sigue intacta para clientes OTel.
//!
//! Convenciones:
//!   - `trace_id` y `span_id` se aceptan en hex (sin guiones). Si vienen vacíos
//!     el span se descarta — sin esos dos campos no se puede mostrar nada útil.
//!   - `kind` admite los valores OTel: `internal` (default), `server`, `client`,
//!     `producer`, `consumer`.
//!   - `status_code` admite `OK`, `ERROR`, `UNSET`.
//!   - `duration_ns` se calcula desde `end - start` cuando ambos están presentes;
//!     si solo viene `duration_ns` directamente, se usa tal cual.

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::logs::json_to_attr_map;
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::storage::{AttrMap, SpanRow};

pub fn router() -> Router<SharedState> {
    Router::new().route("/spans", post(ingest_spans))
}

#[derive(Deserialize)]
struct IngestPayload {
    service: Option<String>,
    spans: Vec<RawSpan>,
}

#[derive(Deserialize)]
struct RawSpan {
    trace_id: String,
    span_id: String,
    #[serde(default)]
    parent_span_id: Option<String>,
    name: String,
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    service: Option<String>,
    /// Marca de inicio. Si falta, se descarta el span — sin `start` no hay
    /// timeline en el dashboard.
    start: DateTime<Utc>,
    /// Marca de fin. Si falta y `duration_ns` también, asumimos `start` (span
    /// de duración 0 — útil para events one-shot).
    #[serde(default)]
    end: Option<DateTime<Utc>>,
    #[serde(default)]
    duration_ns: Option<u64>,
    #[serde(default = "default_status")]
    status_code: String,
    #[serde(default)]
    status_message: String,
    #[serde(default)]
    attributes: Option<Value>,
}

fn default_kind() -> String {
    "internal".into()
}

fn default_status() -> String {
    "UNSET".into()
}

async fn ingest_spans(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(payload): Json<IngestPayload>,
) -> ApiResult<Json<serde_json::Value>> {
    let project = super::resolve_project(&state, &headers)?;
    super::check_origin(&state, &project, &headers)?;

    let n: u32 = payload.spans.len().try_into().unwrap_or(u32::MAX);
    super::otlp::enforce_limit(&state, &project, "traces", n)?;

    let svc_default = payload.service.unwrap_or_else(|| "unknown".into());
    let redaction_rules = state.projects.redaction(&project);
    let mut accepted = 0u64;
    let mut dropped_invalid = 0u64;

    for raw in payload.spans {
        if raw.trace_id.is_empty() || raw.span_id.is_empty() {
            dropped_invalid += 1;
            continue;
        }
        let svc = raw.service.unwrap_or_else(|| svc_default.clone());
        let duration_ns = raw.duration_ns.unwrap_or_else(|| {
            raw.end
                .map(|e| (e - raw.start).num_nanoseconds().unwrap_or(0).max(0) as u64)
                .unwrap_or(0)
        });
        let attrs = json_to_attr_map(raw.attributes.as_ref());

        let mut row = SpanRow {
            timestamp: raw.start,
            project_id: project.clone(),
            trace_id: raw.trace_id,
            span_id: raw.span_id,
            parent_span_id: raw.parent_span_id.unwrap_or_default(),
            trace_state: String::new(),
            name: raw.name,
            kind: normalize_kind(&raw.kind).into(),
            service_name: svc,
            duration_ns,
            status_code: normalize_status(&raw.status_code).into(),
            status_message: raw.status_message,
            resource_attributes: AttrMap::new(),
            span_attributes: attrs,
            events_timestamps: vec![],
            events_names: vec![],
            events_attributes: vec![],
            links_trace_ids: vec![],
            links_span_ids: vec![],
        };
        super::redact_span(redaction_rules.as_ref(), &mut row);

        if state.ingest.spans_tx.try_send(row).is_ok() {
            accepted += 1;
        } else {
            crate::observability::record_ingest_drop("traces");
            tracing::warn!("spans ingest channel full, dropping record");
        }
    }

    if dropped_invalid > 0 {
        tracing::warn!(
            project,
            dropped_invalid,
            "spans descartados sin trace_id/span_id"
        );
    }
    super::otlp::record_accepted(&project, "traces", accepted);

    Ok(Json(serde_json::json!({
        "accepted": accepted,
        "dropped_invalid": dropped_invalid,
        "project": project,
    })))
}

fn normalize_kind(k: &str) -> &'static str {
    match k.to_ascii_lowercase().as_str() {
        "server" => "server",
        "client" => "client",
        "producer" => "producer",
        "consumer" => "consumer",
        _ => "internal",
    }
}

fn normalize_status(s: &str) -> &'static str {
    match s.to_ascii_uppercase().as_str() {
        "OK" => "OK",
        "ERROR" => "ERROR",
        _ => "UNSET",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_kind_defaults_to_internal() {
        assert_eq!(normalize_kind("server"), "server");
        assert_eq!(normalize_kind("CLIENT"), "client");
        assert_eq!(normalize_kind("producer"), "producer");
        assert_eq!(normalize_kind("consumer"), "consumer");
        assert_eq!(normalize_kind("internal"), "internal");
        assert_eq!(normalize_kind(""), "internal");
        assert_eq!(normalize_kind("widget"), "internal");
    }

    #[test]
    fn normalize_status_clamps_to_three_values() {
        assert_eq!(normalize_status("OK"), "OK");
        assert_eq!(normalize_status("ok"), "OK");
        assert_eq!(normalize_status("ERROR"), "ERROR");
        assert_eq!(normalize_status("error"), "ERROR");
        assert_eq!(normalize_status("UNSET"), "UNSET");
        assert_eq!(normalize_status(""), "UNSET");
        assert_eq!(normalize_status("WAT"), "UNSET");
    }
}
