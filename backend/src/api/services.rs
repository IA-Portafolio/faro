use axum::extract::State; use axum_extra::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::api::params::Range;
use crate::error::ApiResult;
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new().route("/services", get(list_services))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Service {
    pub service_name: String,
    pub log_count: u64,
    pub error_count: u64,
    pub last_seen: String,
}

async fn list_services(
    State(state): State<SharedState>,
    Query(range): Query<Range>,
) -> ApiResult<Json<Vec<Service>>> {
    let (from, to) = range.resolve();
    let sql = format!(
        "SELECT service_name, \
                toUInt64(count()) AS log_count, \
                toUInt64(countIf(severity_number >= 17)) AS error_count, \
                toString(max(timestamp)) AS last_seen \
         FROM faro.logs WHERE timestamp >= toDateTime64('{from}', 9) AND timestamp <= toDateTime64('{to}', 9) \
         GROUP BY service_name ORDER BY last_seen DESC LIMIT 200",
        from = crate::api::params::ch_dt(from),
        to = crate::api::params::ch_dt(to),
    );
    let rows: Vec<Service> = state.ch.select(&sql).await?;
    Ok(Json(rows))
}
