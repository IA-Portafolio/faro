use axum::extract::{Path, State}; use axum_extra::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::api::params::{de_opt_num, escape_sql, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::storage::SpanRow;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/traces", get(list_traces))
        .route("/traces/:trace_id", get(get_trace))
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
    let mut filters = vec![
        format!("timestamp >= toDateTime64('{}', 3)", crate::api::params::ch_dt(from)),
        format!("timestamp <= toDateTime64('{}', 3)", crate::api::params::ch_dt(to)),
    ];
    if let Some(p) = &q.range.project {
        if !p.is_empty() {
            filters.push(format!("project_id = '{}'", escape_sql(p)));
        }
    }
    if let Some(svc) = &q.service {
        filters.push(format!("service_name = '{}'", escape_sql(svc)));
    }
    if let Some(st) = &q.status {
        filters.push(format!("status_code = '{}'", escape_sql(st)));
    }
    if let Some(min_ms) = q.min_duration_ms {
        filters.push(format!("duration_ns >= {}", min_ms * 1_000_000));
    }

    // Re-aggregate from spans on read so we always reflect the latest data.
    // Inner alias `ts` avoids shadowing the column `timestamp` under CH 24's analyzer
    // (otherwise toUnixTimestamp64Nano() receives the String alias and errors out).
    // Inner aliases use distinct names (ts, svc, root, dur, status, count) to avoid
    // shadowing actual columns under CH 24's new analyzer; the outer SELECT renames
    // them back to the API contract.
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
            FROM faro.spans WHERE {} GROUP BY trace_id \
         ) ORDER BY ts DESC LIMIT {}",
        filters.join(" AND "),
        q.range.limit()
    );
    let rows: Vec<TraceSummary> = state.ch.select(&sql).await?;
    Ok(Json(rows))
}

async fn get_trace(
    State(state): State<SharedState>,
    Path(trace_id): Path<String>,
) -> ApiResult<Json<Vec<SpanRow>>> {
    let id = escape_sql(&trace_id);
    let sql = format!(
        "SELECT timestamp, trace_id, span_id, parent_span_id, trace_state, \
                name, kind, service_name, duration_ns, status_code, status_message, \
                resource_attributes, span_attributes, \
                events_timestamps, \
                events_names, events_attributes, links_trace_ids, links_span_ids \
         FROM faro.spans WHERE trace_id = '{id}' ORDER BY timestamp ASC LIMIT 5000"
    );
    let rows: Vec<SpanRow> = state.ch.select(&sql).await?;
    if rows.is_empty() {
        return Err(ApiError::NotFound);
    }
    Ok(Json(rows))
}
