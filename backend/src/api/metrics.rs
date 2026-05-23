use axum::extract::State; use axum_extra::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::api::params::{de_opt_num, escape_sql, Range};
use crate::error::ApiResult;
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/metrics/series", get(query_series))
        .route("/metrics/names", get(list_names))
}

#[derive(Debug, Deserialize)]
pub struct SeriesQuery {
    #[serde(flatten)]
    pub range: Range,
    pub name: String,
    pub service: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num")]
    pub bucket_seconds: Option<u32>,
    pub agg: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Point {
    pub ts: String,
    pub value: f64,
}

async fn query_series(
    State(state): State<SharedState>,
    Query(q): Query<SeriesQuery>,
) -> ApiResult<Json<Vec<Point>>> {
    let (from, to) = q.range.resolve();
    let bucket = q.bucket_seconds.unwrap_or(60).max(1);
    let agg = q.agg.as_deref().unwrap_or("avg");
    let agg_expr = match agg {
        "sum" => "sum(value)",
        "max" => "max(value)",
        "min" => "min(value)",
        "count" => "toFloat64(count())",
        _ => "avg(value)",
    };
    let svc = q
        .service
        .as_ref()
        .map(|s| format!(" AND service_name = '{}'", escape_sql(s)))
        .unwrap_or_default();
    let proj = q.range.project_clause("");
    let sql = format!(
        "SELECT toString(toStartOfInterval(timestamp, INTERVAL {bucket} second)) AS ts, \
                toFloat64({agg_expr}) AS value \
         FROM faro.metrics WHERE metric_name = '{name}' \
           AND timestamp >= toDateTime64('{from}', 9) \
           AND timestamp <= toDateTime64('{to}', 9){svc}{proj} \
         GROUP BY ts ORDER BY ts",
        name = escape_sql(&q.name),
        from = crate::api::params::ch_dt(from),
        to = crate::api::params::ch_dt(to),
    );
    let rows: Vec<Point> = state.ch.select(&sql).await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetricName {
    pub metric_name: String,
    pub metric_type: String,
    pub metric_unit: String,
    pub service_name: String,
}

async fn list_names(
    State(state): State<SharedState>,
    Query(range): Query<Range>,
) -> ApiResult<Json<Vec<MetricName>>> {
    let (from, to) = range.resolve();
    let proj = range.project_clause("");
    let sql = format!(
        "SELECT DISTINCT metric_name, metric_type, metric_unit, service_name \
         FROM faro.metrics WHERE timestamp >= toDateTime64('{from}', 9) AND timestamp <= toDateTime64('{to}', 9){proj} \
         ORDER BY metric_name LIMIT 1000",
        from = crate::api::params::ch_dt(from),
        to = crate::api::params::ch_dt(to),
    );
    let rows: Vec<MetricName> = state.ch.select(&sql).await?;
    Ok(Json(rows))
}
