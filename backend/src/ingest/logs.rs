use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::error::{ApiError, ApiResult};
use crate::observability::names;
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

fn default_level() -> String {
    "INFO".into()
}

async fn ingest_logs(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(payload): Json<IngestPayload>,
) -> ApiResult<Json<serde_json::Value>> {
    let project = super::resolve_project(&state, &headers)?;
    // Si el request trae `Origin`, validar contra la whitelist del proyecto.
    // Server-side SDKs no envían Origin y pasan sin chequeo (el bearer alcanza).
    super::check_origin(&state, &project, &headers)?;

    // Mismo bucket que OTLP — un cliente no puede esquivar el límite saltando
    // de transporte. Contamos los records del payload antes de procesar.
    let n: u32 = payload.logs.len().try_into().unwrap_or(u32::MAX);
    match state.limiter.check(&project, n) {
        super::rate_limit::LimitOutcome::Allowed => {}
        other => {
            let secs = other.retry_after_secs();
            tracing::warn!(
                project,
                records = n,
                retry_after_secs = secs,
                "ingest /logs rate-limited"
            );
            metrics::counter!(
                names::RATE_LIMITED,
                "project" => project.clone(),
                "signal" => "logs",
            )
            .increment(1);
            metrics::counter!(
                names::INGEST_RECORDS,
                "project" => project.clone(),
                "signal" => "logs",
                "outcome" => "rate_limited",
            )
            .increment(n as u64);
            return Err(ApiError::TooManyRequests {
                retry_after_secs: secs,
            });
        }
    }

    // Telemetría de compatibilidad de SDK — por ahora solo loggeamos, no
    // rechazamos. La política de rechazo (cuando `Unsupported`) se activa
    // en un PR de seguimiento; ver ADR-0008.
    log_sdk_compat(&headers);

    let now = Utc::now();
    let svc_default = payload.service.unwrap_or_else(|| "unknown".into());
    let mut accepted = 0u64;
    // Resolvemos las reglas UNA vez por batch — `redaction()` toma el RwLock por
    // lectura, no queremos hacerlo por cada log. Si entre tanto el admin guarda
    // un cambio, los logs siguientes del próximo POST ya verán la versión nueva.
    let redaction_rules = state.projects.redaction(&project);

    for raw in payload.logs {
        let svc = raw.service.unwrap_or_else(|| svc_default.clone());
        let ts = raw.timestamp.unwrap_or(now);
        let attrs = json_to_attr_map(raw.attributes.as_ref());
        let severity = LogRow::severity_from_text(&raw.level);
        let mut row = LogRow {
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
        super::redact_log(redaction_rules.as_ref(), &mut row);
        let _ = state.live_bus.logs.send(row.clone());
        if state.ingest.logs_tx.try_send(row).is_ok() {
            accepted += 1;
        } else {
            tracing::warn!("log ingest channel full, dropping record");
        }
    }

    if accepted > 0 {
        metrics::counter!(
            names::INGEST_RECORDS,
            "project" => project.clone(),
            "signal" => "logs",
            "outcome" => "accepted",
        )
        .increment(accepted);
    }

    Ok(Json(
        serde_json::json!({ "accepted": accepted, "project": project }),
    ))
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

/// Loggea un warning si el SDK declara un protocolo desfasado. No
/// rechaza la request — la política de rechazo se activa en un PR
/// posterior; ver ADR-0008. Útil ahora para tener visibilidad real
/// de qué versiones de SDK están en uso antes de subir mínimos.
fn log_sdk_compat(headers: &HeaderMap) {
    use crate::versions::{
        classify_protocol, CompatStatus, HEADER_PROTOCOL, HEADER_SDK_NAME, HEADER_SDK_VERSION,
    };
    let proto = headers.get(HEADER_PROTOCOL).and_then(|v| v.to_str().ok());
    let status = classify_protocol(proto);
    if status == CompatStatus::Ok {
        return;
    }
    let sdk_name = headers
        .get(HEADER_SDK_NAME)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    let sdk_version = headers
        .get(HEADER_SDK_VERSION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    tracing::warn!(
        sdk_name,
        sdk_version,
        protocol = proto.unwrap_or("missing"),
        status = status.header_value(),
        "SDK con protocolo fuera de rango — se acepta por ahora",
    );
}
