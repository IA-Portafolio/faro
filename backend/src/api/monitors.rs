use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::params::Range;
use crate::error::ApiResult;
use crate::state::SharedState;
use crate::storage::{AttrMap, MonitorResultRow, MonitorRow};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/monitors", get(list_monitors).post(create_monitor))
        .route(
            "/monitors/{id}",
            get(get_monitor).put(update_monitor).delete(delete_monitor),
        )
        .route("/monitors/{id}/results", get(monitor_results))
        .route("/monitors/{id}/uptime", get(monitor_uptime))
}

async fn list_monitors(
    State(state): State<SharedState>,
    Query(range): Query<Range>,
) -> ApiResult<Json<Vec<MonitorRow>>> {
    let (proj_clause, proj_value) = range.project_clause("");
    let mut params: Vec<(&str, &str)> = Vec::new();
    if let Some(p) = proj_value {
        params.push(("project", p));
    }
    let sql = format!(
        "SELECT id, project_id, name, method, url, headers, body, interval_seconds, timeout_seconds, \
         expected_status_min, expected_status_max, expected_body_regex, enabled, \
         created_at, updated_at, deleted, version \
         FROM faro.api_monitors FINAL WHERE deleted = 0{proj_clause} ORDER BY name"
    );
    let rows: Vec<MonitorRow> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct MonitorInput {
    pub name: String,
    #[serde(default = "default_project")]
    pub project: String,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: AttrMap,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_interval")]
    pub interval_seconds: u32,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
    #[serde(default = "default_status_min")]
    pub expected_status_min: u16,
    #[serde(default = "default_status_max")]
    pub expected_status_max: u16,
    #[serde(default)]
    pub expected_body_regex: String,
    #[serde(default = "default_true")]
    pub enabled: u8,
}

fn default_interval() -> u32 {
    60
}
fn default_timeout() -> u32 {
    30
}
fn default_status_min() -> u16 {
    200
}
fn default_status_max() -> u16 {
    299
}
fn default_true() -> u8 {
    1
}
fn default_project() -> String {
    "default".into()
}

async fn create_monitor(
    State(state): State<SharedState>,
    Json(input): Json<MonitorInput>,
) -> ApiResult<Json<MonitorRow>> {
    let now = Utc::now();
    let row = MonitorRow {
        id: Uuid::new_v4(),
        project_id: if input.project.is_empty() {
            "default".into()
        } else {
            input.project
        },
        name: input.name,
        method: input.method,
        url: input.url,
        headers: input.headers,
        body: input.body,
        interval_seconds: input.interval_seconds,
        timeout_seconds: input.timeout_seconds,
        expected_status_min: input.expected_status_min,
        expected_status_max: input.expected_status_max,
        expected_body_regex: input.expected_body_regex,
        enabled: input.enabled,
        created_at: now,
        updated_at: now,
        deleted: 0,
        version: now.timestamp_millis() as u64,
    };
    state.ch.insert("faro.api_monitors", &[row.clone()]).await?;
    Ok(Json(row))
}

async fn get_monitor(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<MonitorRow>> {
    let id_s = id.to_string();
    let sql = "SELECT id, project_id, name, method, url, headers, body, interval_seconds, timeout_seconds, \
         expected_status_min, expected_status_max, expected_body_regex, enabled, \
         created_at, updated_at, deleted, version \
         FROM faro.api_monitors FINAL WHERE id = {id:UUID} AND deleted = 0 LIMIT 1";
    let row: Option<MonitorRow> = state
        .ch
        .select_one_with_params(sql, &[("id", &id_s)])
        .await?;
    row.map(Json).ok_or(crate::error::ApiError::NotFound)
}

async fn update_monitor(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(input): Json<MonitorInput>,
) -> ApiResult<Json<MonitorRow>> {
    let now = Utc::now();
    let id_s = id.to_string();
    let existing_sql =
        "SELECT id, project_id, name, method, url, headers, body, interval_seconds, timeout_seconds, \
         expected_status_min, expected_status_max, expected_body_regex, enabled, \
         created_at, updated_at, deleted, version \
         FROM faro.api_monitors FINAL WHERE id = {id:UUID} LIMIT 1";
    let mut existing: MonitorRow = state
        .ch
        .select_one_with_params(existing_sql, &[("id", &id_s)])
        .await?
        .ok_or(crate::error::ApiError::NotFound)?;
    existing.name = input.name;
    existing.method = input.method;
    existing.url = input.url;
    existing.headers = input.headers;
    existing.body = input.body;
    existing.interval_seconds = input.interval_seconds;
    existing.timeout_seconds = input.timeout_seconds;
    existing.expected_status_min = input.expected_status_min;
    existing.expected_status_max = input.expected_status_max;
    existing.expected_body_regex = input.expected_body_regex;
    existing.enabled = input.enabled;
    existing.updated_at = now;
    existing.version = now.timestamp_millis() as u64;
    state
        .ch
        .insert("faro.api_monitors", &[existing.clone()])
        .await?;
    Ok(Json(existing))
}

async fn delete_monitor(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let id_s = id.to_string();
    let sql =
        "SELECT id, project_id, name, method, url, headers, body, interval_seconds, timeout_seconds, \
         expected_status_min, expected_status_max, expected_body_regex, enabled, \
         created_at, updated_at, deleted, version \
         FROM faro.api_monitors FINAL WHERE id = {id:UUID} LIMIT 1";
    let mut row: MonitorRow = state
        .ch
        .select_one_with_params(sql, &[("id", &id_s)])
        .await?
        .ok_or(crate::error::ApiError::NotFound)?;
    row.deleted = 1;
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.api_monitors", &[row]).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn monitor_results(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(range): Query<Range>,
) -> ApiResult<Json<Vec<MonitorResultRow>>> {
    let (from, to) = range.resolve();
    let id_s = id.to_string();
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);
    let sql = format!(
        "SELECT monitor_id, timestamp, success, status_code, duration_ms, error_message, response_size \
         FROM faro.monitor_results \
         WHERE monitor_id = {{id:UUID}} \
           AND timestamp >= toDateTime64({{from:DateTime64(3)}}, 3) \
           AND timestamp <= toDateTime64({{to:DateTime64(3)}}, 3) \
         ORDER BY timestamp DESC LIMIT {limit}",
        limit = range.limit(),
    );
    let rows: Vec<MonitorResultRow> = state
        .ch
        .select_with_params(&sql, &[("id", &id_s), ("from", &from_s), ("to", &to_s)])
        .await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UptimeStats {
    pub total: u64,
    pub success: u64,
    pub uptime_pct: f64,
    pub avg_duration_ms: f64,
    pub p95_duration_ms: f64,
}

async fn monitor_uptime(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(range): Query<Range>,
) -> ApiResult<Json<UptimeStats>> {
    let (from, to) = range.resolve();
    let id_s = id.to_string();
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);
    let sql = "SELECT toUInt64(count()) AS total, \
                toUInt64(sum(success)) AS success, \
                if(count() > 0, sum(success)/count()*100, 100) AS uptime_pct, \
                toFloat64(avg(duration_ms)) AS avg_duration_ms, \
                toFloat64(quantile(0.95)(duration_ms)) AS p95_duration_ms \
         FROM faro.monitor_results \
         WHERE monitor_id = {id:UUID} \
           AND timestamp >= toDateTime64({from:DateTime64(3)}, 3) \
           AND timestamp <= toDateTime64({to:DateTime64(3)}, 3)";
    let row: Option<UptimeStats> = state
        .ch
        .select_one_with_params(sql, &[("id", &id_s), ("from", &from_s), ("to", &to_s)])
        .await?;
    Ok(Json(row.unwrap_or(UptimeStats {
        total: 0,
        success: 0,
        uptime_pct: 100.0,
        avg_duration_ms: 0.0,
        p95_duration_ms: 0.0,
    })))
}
