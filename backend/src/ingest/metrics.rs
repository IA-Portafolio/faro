//! Ingest nativo para métricas (sin OTLP).
//!
//! Los SDKs `@iaportafolio/*` envían contadores / gauges / histogramas con un
//! payload JSON simple. Conceptualmente equivalente a `ingest::logs` pero
//! escribiendo en `faro.metrics`. La ruta OTLP (`/v1/metrics`) sigue ahí intacta
//! para clientes OTel — este endpoint es el camino corto para el SDK propio,
//! sin tener que generar protobuf ni entender `aggregationTemporality`.

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
use crate::storage::{AttrMap, MetricRow};

pub fn router() -> Router<SharedState> {
    Router::new().route("/metrics", post(ingest_metrics))
}

#[derive(Deserialize)]
struct IngestPayload {
    service: Option<String>,
    metrics: Vec<RawMetric>,
}

/// `kind` aceptados: `counter` (monotónico), `sum` (no monotónico),
/// `gauge`, `histogram`. Cualquier otro valor se normaliza a `gauge` —
/// preferimos guardar el data point con la etiqueta menos significativa
/// antes que rechazar el batch entero por una metadata equivocada.
#[derive(Deserialize)]
struct RawMetric {
    name: String,
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    value: Option<f64>,
    #[serde(default)]
    attributes: Option<Value>,
    // Campos opcionales que SOLO aplican a histogramas. Se ignoran para
    // counter/gauge/sum (que usan `value`).
    #[serde(default)]
    count: Option<u64>,
    #[serde(default)]
    sum: Option<f64>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    bucket_bounds: Option<Vec<f64>>,
    #[serde(default)]
    bucket_counts: Option<Vec<u64>>,
}

fn default_kind() -> String {
    "gauge".into()
}

async fn ingest_metrics(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(payload): Json<IngestPayload>,
) -> ApiResult<Json<serde_json::Value>> {
    let project = super::resolve_project(&state, &headers)?;
    super::check_origin(&state, &project, &headers)?;

    let n: u32 = payload.metrics.len().try_into().unwrap_or(u32::MAX);
    super::otlp::enforce_limit(&state, &project, "metrics", n)?;

    let now = Utc::now();
    let svc_default = payload.service.unwrap_or_else(|| "unknown".into());
    let mut accepted = 0u64;

    for raw in payload.metrics {
        let svc = raw.service.unwrap_or_else(|| svc_default.clone());
        let ts = raw.timestamp.unwrap_or(now);
        let attrs = json_to_attr_map(raw.attributes.as_ref());
        let kind = normalize_kind(&raw.kind);
        let is_hist = kind == "histogram";

        let row = MetricRow {
            timestamp: ts,
            project_id: project.clone(),
            metric_name: raw.name,
            metric_type: kind.into(),
            metric_unit: raw.unit,
            service_name: svc,
            // Para histogramas la columna `value` guarda la suma (consistente con la
            // ruta OTLP: ver `ingest_metrics` en otlp.rs). Para el resto, el value
            // del data point directo. Sin value => 0.0.
            value: if is_hist {
                raw.sum.unwrap_or(0.0)
            } else {
                raw.value.unwrap_or(0.0)
            },
            resource_attributes: AttrMap::new(),
            attributes: attrs,
            hist_count: raw.count.unwrap_or(0),
            hist_sum: raw.sum.unwrap_or(0.0),
            hist_min: raw.min.unwrap_or(0.0),
            hist_max: raw.max.unwrap_or(0.0),
            hist_bucket_bounds: raw.bucket_bounds.unwrap_or_default(),
            hist_bucket_counts: raw.bucket_counts.unwrap_or_default(),
        };

        if state.ingest.metrics_tx.try_send(row).is_ok() {
            accepted += 1;
        } else {
            crate::observability::record_ingest_drop("metrics");
            tracing::warn!("metrics ingest channel full, dropping data point");
        }
    }

    super::otlp::record_accepted(&project, "metrics", accepted);

    Ok(Json(
        serde_json::json!({ "accepted": accepted, "project": project }),
    ))
}

fn normalize_kind(k: &str) -> &'static str {
    match k.to_ascii_lowercase().as_str() {
        "counter" => "counter",
        "sum" | "updowncounter" | "up_down_counter" => "sum",
        "histogram" => "histogram",
        "summary" => "summary",
        _ => "gauge",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_kind_maps_common_aliases() {
        assert_eq!(normalize_kind("counter"), "counter");
        assert_eq!(normalize_kind("Counter"), "counter");
        assert_eq!(normalize_kind("gauge"), "gauge");
        assert_eq!(normalize_kind("histogram"), "histogram");
        assert_eq!(normalize_kind("updowncounter"), "sum");
        assert_eq!(normalize_kind("up_down_counter"), "sum");
        assert_eq!(normalize_kind("sum"), "sum");
        assert_eq!(normalize_kind("summary"), "summary");
        // Desconocido → gauge para no perder el data point.
        assert_eq!(normalize_kind("widget"), "gauge");
        assert_eq!(normalize_kind(""), "gauge");
    }
}
