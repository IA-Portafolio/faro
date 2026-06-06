//! Endpoints de trazas (tracing distribuido):
//!   GET /traces            → lista de trazas (filtro por servicio/estado/duración)
//!   GET /traces/{trace_id} → todos los spans que componen una traza

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{de_opt_num, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::storage::SpanRow;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/traces", get(list_traces))
        .route("/traces/{trace_id}", get(get_trace))
}

#[derive(Debug, Deserialize)]
pub struct TracesQuery {
    #[serde(flatten)]
    pub range: Range,
    pub service: Option<String>,
    pub status: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num")]
    pub min_duration_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TraceSummary {
    pub trace_id: String,
    pub timestamp: String,
    pub service_name: String,
    pub root_name: String,
    pub duration_ns: u64,
    pub status_code: String,
    pub span_count: u32,
}

async fn list_traces(
    State(state): State<SharedState>,
    Query(q): Query<TracesQuery>,
) -> ApiResult<Json<Vec<TraceSummary>>> {
    let (from, to) = q.range.resolve();
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);

    let mut inner_where = String::from(
        "timestamp >= toDateTime64({from:DateTime64(3)}, 3) \
         AND timestamp <= toDateTime64({to:DateTime64(3)}, 3)",
    );
    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];

    if let Some(p) = &q.range.project {
        if !p.is_empty() {
            inner_where.push_str(" AND project_id = {project:String}");
            params.push(("project", p));
        }
    }
    if let Some(svc) = &q.service {
        inner_where.push_str(" AND service_name = {service:String}");
        params.push(("service", svc));
    }
    if let Some(st) = &q.status {
        inner_where.push_str(" AND status_code = {status:String}");
        params.push(("status", st));
    }
    let min_ns_s;
    if let Some(min_ms) = q.min_duration_ms {
        min_ns_s = (min_ms * 1_000_000).to_string();
        inner_where.push_str(" AND duration_ns >= {min_dur_ns:UInt64}");
        params.push(("min_dur_ns", &min_ns_s));
    }

    // Re-agrega desde spans en cada lectura para reflejar siempre los datos más recientes.
    // El alias interno `ts` evita sombrear la columna `timestamp` bajo el analizador de CH 24
    // (de lo contrario toUnixTimestamp64Nano() recibe el alias String y falla).
    // Los alias internos usan nombres distintos (ts, svc, root, dur, status, count) para
    // evitar sombrear columnas reales con el nuevo analizador de CH 24; el SELECT externo
    // los renombra al contrato de la API.
    let sql = format!(
        "SELECT trace_id, toString(ts) AS timestamp, svc AS service_name, root AS root_name, \
                dur AS duration_ns, status AS status_code, span_count \
         FROM ( \
            SELECT trace_id, \
                   min(timestamp) AS ts, \
                   any(service_name) AS svc, \
                   argMin(name, timestamp) AS root, \
                   toUInt64(max(toUnixTimestamp64Nano(timestamp) + duration_ns) - min(toUnixTimestamp64Nano(timestamp))) AS dur, \
                   argMax(status_code, duration_ns) AS status, \
                   toUInt32(count()) AS span_count \
            FROM faro.spans WHERE {inner_where} GROUP BY trace_id \
         ) ORDER BY ts DESC LIMIT {limit}",
        inner_where = inner_where,
        limit = q.range.limit()
    );
    let rows: Vec<TraceSummary> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

async fn get_trace(
    State(state): State<SharedState>,
    Path(trace_id): Path<String>,
) -> ApiResult<Json<Vec<SpanRow>>> {
    let sql = "SELECT timestamp, trace_id, span_id, parent_span_id, trace_state, \
                name, kind, service_name, duration_ns, status_code, status_message, \
                resource_attributes, span_attributes, \
                events_timestamps, \
                events_names, events_attributes, links_trace_ids, links_span_ids \
         FROM faro.spans WHERE trace_id = {trace_id:String} ORDER BY timestamp ASC LIMIT 5000";
    let rows: Vec<SpanRow> = state
        .ch
        .select_with_params(sql, &[("trace_id", &trace_id)])
        .await?;
    if rows.is_empty() {
        return Err(ApiError::NotFound);
    }
    Ok(Json(rows))
}
