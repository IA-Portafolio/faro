use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::storage::{AttrMap, LogRow};

pub fn router() -> Router<SharedState> {
    Router::new().route("/logs", post(ingest_logs))
}

#[derive(Deserialize)]
struct IngestPayload {
    service: Option<String>,
    logs: Vec<RawLog>,
}

#[derive(Deserialize)]
struct RawLog {
    #[serde(default)]
    timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default = "default_level")]
    level: String,
    message: String,
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    attributes: Option<Value>,
}

fn default_level() -> String { "INFO".into() }

async fn ingest_logs(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(payload): Json<IngestPayload>,
) -> ApiResult<Json<serde_json::Value>> {
    let project = super::resolve_project(&state, &headers)?;

    let now = Utc::now();
    let svc_default = payload.service.unwrap_or_else(|| "unknown".into());
    let mut accepted = 0u64;

    for raw in payload.logs {
        let svc = raw.service.unwrap_or_else(|| svc_default.clone());
        let ts = raw.timestamp.unwrap_or(now);
        let attrs = json_to_attr_map(raw.attributes.as_ref());
        let severity = LogRow::severity_from_text(&raw.level);
        let row = LogRow {
            timestamp: ts,
            observed_timestamp: now,
            project_id: project.clone(),
            service_name: svc,
            severity_text: raw.level.to_uppercase(),
            severity_number: severity,
            body: raw.message,
            trace_id: raw.trace_id.unwrap_or_default(),
            span_id: raw.span_id.unwrap_or_default(),
            scope_name: String::new(),
            resource_attributes: AttrMap::new(),
            attributes: attrs,
        };
        let _ = state.live_bus.logs.send(row.clone());
        if state.ingest.logs_tx.try_send(row).is_ok() {
            accepted += 1;
        } else {
            tracing::warn!("log ingest channel full, dropping record");
        }
    }

    Ok(Json(serde_json::json!({ "accepted": accepted, "project": project })))
}

pub fn json_to_attr_map(v: Option<&Value>) -> AttrMap {
    let mut out = AttrMap::new();
    if let Some(Value::Object(map)) = v {
        for (k, v) in map {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            out.insert(k.clone(), s);
        }
    }
    out
}

// Re-exportado por claridad aunque ingest::resolve_project ya maneja la validación.
#[allow(dead_code)]
pub fn legacy_require_token(_h: &HeaderMap, _t: &str) -> bool {
    false
}

// Marcador para mantener en uso la variante Unauthorized de error.rs.
#[allow(dead_code)]
fn _types(_: ApiError) {}
