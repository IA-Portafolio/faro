use axum::extract::State; use axum_extra::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::api::params::Range;
use crate::error::ApiResult;
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new().route("/dashboard", get(dashboard))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DashboardSummary {
    pub log_count: u64,
    pub error_count: u64,
    pub service_count: u64,
    pub trace_count: u64,
    pub open_issue_count: u64,
    pub firing_incident_count: u64,
    pub monitors_total: u64,
    pub monitors_down: u64,
}

async fn dashboard(
    State(state): State<SharedState>,
    Query(range): Query<Range>,
) -> ApiResult<Json<DashboardSummary>> {
    let (from, to) = range.resolve();
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);
    let proj = range.project_clause("");

    let sql = format!(
        "SELECT \
            (SELECT toUInt64(count()) FROM faro.logs WHERE timestamp >= toDateTime64('{from_s}', 9) AND timestamp <= toDateTime64('{to_s}', 9){proj}) AS log_count, \
            (SELECT toUInt64(countIf(severity_number >= 17)) FROM faro.logs WHERE timestamp >= toDateTime64('{from_s}', 9) AND timestamp <= toDateTime64('{to_s}', 9){proj}) AS error_count, \
            (SELECT toUInt64(uniqExact(service_name)) FROM faro.logs WHERE timestamp >= toDateTime64('{from_s}', 9) AND timestamp <= toDateTime64('{to_s}', 9){proj}) AS service_count, \
            (SELECT toUInt64(uniqExact(trace_id)) FROM faro.spans WHERE timestamp >= toDateTime64('{from_s}', 9) AND timestamp <= toDateTime64('{to_s}', 9){proj}) AS trace_count, \
            (SELECT toUInt64(uniqExact(fingerprint)) FROM faro.error_events WHERE timestamp >= toDateTime64('{from_s}', 9) AND timestamp <= toDateTime64('{to_s}', 9){proj}) AS open_issue_count, \
            (SELECT toUInt64(countIf(status = 'firing')) FROM faro.alert_incidents FINAL WHERE 1=1{proj}) AS firing_incident_count, \
            (SELECT toUInt64(count()) FROM faro.api_monitors FINAL WHERE deleted = 0 AND enabled = 1{proj}) AS monitors_total, \
            (SELECT toUInt64(uniqExact(monitor_id)) FROM faro.monitor_results \
                WHERE timestamp >= now() - INTERVAL 5 MINUTE AND success = 0{proj}) AS monitors_down"
    );

    let row: Option<DashboardSummary> = state.ch.select_one(&sql).await?;
    Ok(Json(row.unwrap_or(DashboardSummary {
        log_count: 0, error_count: 0, service_count: 0, trace_count: 0,
        open_issue_count: 0, firing_incident_count: 0, monitors_total: 0, monitors_down: 0,
    })))
}
