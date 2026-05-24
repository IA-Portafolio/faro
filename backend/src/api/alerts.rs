use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::params::Range;
use crate::error::ApiResult;
use crate::state::SharedState;
use crate::storage::{AlertIncidentRow, AlertRuleRow};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/alerts/rules", get(list_rules).post(create_rule))
        .route(
            "/alerts/rules/:id",
            get(get_rule).put(update_rule).delete(delete_rule),
        )
        .route("/alerts/incidents", get(list_incidents))
}

async fn list_rules(
    State(state): State<SharedState>,
    Query(range): Query<Range>,
) -> ApiResult<Json<Vec<AlertRuleRow>>> {
    let (proj_clause, proj_value) = range.project_clause("");
    let mut params: Vec<(&str, &str)> = Vec::new();
    if let Some(p) = proj_value {
        params.push(("project", p));
    }
    let sql = format!(
        "SELECT id, project_id, name, description, source, query, condition, threshold, \
         window_seconds, interval_seconds, severity, notification_targets, enabled, \
         created_at, updated_at, deleted, version \
         FROM faro.alert_rules FINAL WHERE deleted = 0{proj_clause} ORDER BY name"
    );
    let rows: Vec<AlertRuleRow> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct RuleInput {
    pub name: String,
    #[serde(default = "default_project_rule")]
    pub project: String,
    #[serde(default)]
    pub description: String,
    pub source: String,
    pub query: String,
    pub condition: String,
    pub threshold: f64,
    #[serde(default = "default_window")]
    pub window_seconds: u32,
    #[serde(default = "default_interval")]
    pub interval_seconds: u32,
    #[serde(default = "default_sev")]
    pub severity: String,
    #[serde(default)]
    pub notification_targets: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: u8,
}

fn default_window() -> u32 {
    300
}
fn default_interval() -> u32 {
    60
}
fn default_sev() -> String {
    "warn".into()
}
fn default_enabled() -> u8 {
    1
}
fn default_project_rule() -> String {
    "default".into()
}

async fn create_rule(
    State(state): State<SharedState>,
    Json(input): Json<RuleInput>,
) -> ApiResult<Json<AlertRuleRow>> {
    let now = Utc::now();
    let row = AlertRuleRow {
        id: Uuid::new_v4(),
        project_id: if input.project.is_empty() {
            "default".into()
        } else {
            input.project
        },
        name: input.name,
        description: input.description,
        source: input.source,
        query: input.query,
        condition: input.condition,
        threshold: input.threshold,
        window_seconds: input.window_seconds,
        interval_seconds: input.interval_seconds,
        severity: input.severity,
        notification_targets: input.notification_targets,
        enabled: input.enabled,
        created_at: now,
        updated_at: now,
        deleted: 0,
        version: now.timestamp_millis() as u64,
    };
    state.ch.insert("faro.alert_rules", &[row.clone()]).await?;
    Ok(Json(row))
}

const RULE_COLS: &str = "id, project_id, name, description, source, query, condition, threshold, \
     window_seconds, interval_seconds, severity, notification_targets, enabled, \
     created_at, updated_at, deleted, version";

async fn get_rule(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AlertRuleRow>> {
    let id_s = id.to_string();
    let sql = format!(
        "SELECT {RULE_COLS} FROM faro.alert_rules FINAL WHERE id = {{id:UUID}} AND deleted = 0 LIMIT 1"
    );
    state
        .ch
        .select_one_with_params::<AlertRuleRow>(&sql, &[("id", &id_s)])
        .await?
        .map(Json)
        .ok_or(crate::error::ApiError::NotFound)
}

async fn update_rule(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(input): Json<RuleInput>,
) -> ApiResult<Json<AlertRuleRow>> {
    let now = Utc::now();
    let id_s = id.to_string();
    let sql =
        format!("SELECT {RULE_COLS} FROM faro.alert_rules FINAL WHERE id = {{id:UUID}} LIMIT 1");
    let mut row: AlertRuleRow = state
        .ch
        .select_one_with_params(&sql, &[("id", &id_s)])
        .await?
        .ok_or(crate::error::ApiError::NotFound)?;
    row.name = input.name;
    row.description = input.description;
    row.source = input.source;
    row.query = input.query;
    row.condition = input.condition;
    row.threshold = input.threshold;
    row.window_seconds = input.window_seconds;
    row.interval_seconds = input.interval_seconds;
    row.severity = input.severity;
    row.notification_targets = input.notification_targets;
    row.enabled = input.enabled;
    row.updated_at = now;
    row.version = now.timestamp_millis() as u64;
    state.ch.insert("faro.alert_rules", &[row.clone()]).await?;
    Ok(Json(row))
}

async fn delete_rule(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let id_s = id.to_string();
    let sql =
        format!("SELECT {RULE_COLS} FROM faro.alert_rules FINAL WHERE id = {{id:UUID}} LIMIT 1");
    let mut row: AlertRuleRow = state
        .ch
        .select_one_with_params(&sql, &[("id", &id_s)])
        .await?
        .ok_or(crate::error::ApiError::NotFound)?;
    row.deleted = 1;
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.alert_rules", &[row]).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn list_incidents(
    State(state): State<SharedState>,
    Query(range): Query<Range>,
) -> ApiResult<Json<Vec<AlertIncidentRow>>> {
    let (from, to) = range.resolve();
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);
    let (proj_clause, proj_value) = range.project_clause("");

    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];
    if let Some(p) = proj_value {
        params.push(("project", p));
    }
    let sql = format!(
        "SELECT id, project_id, rule_id, rule_name, started_at, \
                if(isNull(resolved_at), NULL, toString(resolved_at)) AS resolved_at, \
                value, threshold, severity, status, note, version \
         FROM faro.alert_incidents FINAL \
         WHERE started_at >= toDateTime64({{from:DateTime64(3)}}, 3) \
           AND started_at <= toDateTime64({{to:DateTime64(3)}}, 3){proj_clause} \
         ORDER BY started_at DESC LIMIT {limit}",
        limit = range.limit(),
    );
    let rows: Vec<AlertIncidentRow> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}
