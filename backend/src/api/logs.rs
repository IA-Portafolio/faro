use axum::extract::State; use axum_extra::extract::Query;
use axum::response::sse::Sse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::api::params::{de_opt_num, escape_sql, Range};
use crate::error::ApiResult;
use crate::state::SharedState;
use crate::stream::{live_logs_sse, LogFilter};
use crate::storage::LogRow;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/logs", get(list_logs))
        .route("/logs/live", get(stream_logs))
        .route("/logs/stats", get(log_stats))
}

#[derive(Debug, Deserialize)]
pub struct LogQuery {
    #[serde(flatten)]
    pub range: Range,
    pub service: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num")]
    pub min_severity: Option<u8>,
    pub query: Option<String>,
    pub trace_id: Option<String>,
}

async fn list_logs(
    State(state): State<SharedState>,
    Query(q): Query<LogQuery>,
) -> ApiResult<Json<Vec<LogRow>>> {
    let (from, to) = q.range.resolve();
    let mut filters = vec![
        format!("timestamp >= toDateTime64('{}', 9)", crate::api::params::ch_dt(from)),
        format!("timestamp <= toDateTime64('{}', 9)", crate::api::params::ch_dt(to)),
    ];
    if let Some(svc) = &q.range.project {
        if !svc.is_empty() {
            filters.push(format!("project_id = '{}'", escape_sql(svc)));
        }
    }
    if let Some(svc) = &q.service {
        filters.push(format!("service_name = '{}'", escape_sql(svc)));
    }
    if let Some(s) = q.min_severity {
        filters.push(format!("severity_number >= {s}"));
    }
    if let Some(tid) = &q.trace_id {
        filters.push(format!("trace_id = '{}'", escape_sql(tid)));
    }
    if let Some(query) = &q.query {
        let qq = escape_sql(query);
        filters.push(format!("(positionCaseInsensitive(body, '{qq}') > 0)"));
    }

    let sql = format!(
        "SELECT timestamp, observed_timestamp, project_id, \
         service_name, severity_text, severity_number, body, trace_id, span_id, scope_name, \
         resource_attributes, attributes \
         FROM faro.logs WHERE {} \
         ORDER BY timestamp DESC LIMIT {} OFFSET {}",
        filters.join(" AND "),
        q.range.limit(),
        q.range.offset
    );

    let rows: Vec<LogRow> = state.ch.select(&sql).await?;
    Ok(Json(rows))
}

async fn stream_logs(
    State(state): State<SharedState>,
    Query(q): Query<LogQuery>,
) -> Sse<impl futures::stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    let filter = LogFilter {
        service: q.service,
        min_severity: q.min_severity,
        query: q.query,
    };
    let rx = state.live_bus.logs.subscribe();
    live_logs_sse(rx, Some(filter))
}

#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    #[serde(flatten)]
    pub range: Range,
    pub service: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num")]
    pub bucket_seconds: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Bucket {
    pub ts: String,
    pub service: String,
    pub severity: String,
    pub count: u64,
}

async fn log_stats(
    State(state): State<SharedState>,
    Query(q): Query<StatsQuery>,
) -> ApiResult<Json<Vec<Bucket>>> {
    let (from, to) = q.range.resolve();
    let bucket = q.bucket_seconds.unwrap_or(60).max(1);

    let svc_filter = match q.service {
        Some(s) => format!(" AND service_name = '{}'", escape_sql(&s)),
        None => String::new(),
    };

    let sql = format!(
        "SELECT toString(toStartOfInterval(minute, INTERVAL {bucket} second)) AS ts, \
                service_name AS service, severity_text AS severity, \
                toUInt64(countMerge(count)) AS count \
         FROM faro.logs_stats \
         WHERE minute >= toDateTime('{from}') AND minute <= toDateTime('{to}'){svc_filter} \
         GROUP BY ts, service, severity \
         ORDER BY ts",
        from = from.format("%Y-%m-%d %H:%M:%S"),
        to = to.format("%Y-%m-%d %H:%M:%S"),
    );
    let rows: Vec<Bucket> = state.ch.select(&sql).await?;
    Ok(Json(rows))
}
