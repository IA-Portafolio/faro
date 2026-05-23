use axum::extract::{Path, State}; use axum_extra::extract::Query;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::api::params::{escape_sql, Range};
use crate::error::ApiResult;
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/errors", get(list_issues))
        .route("/errors/:fingerprint", get(issue_detail))
        .route("/errors/:fingerprint/status", post(update_status))
}

#[derive(Debug, Deserialize)]
pub struct IssuesQuery {
    #[serde(flatten)]
    pub range: Range,
    pub service: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Issue {
    pub fingerprint: String,
    pub service_name: String,
    pub exception_type: String,
    pub message: String,
    pub event_count: u64,
    pub first_seen: String,
    pub last_seen: String,
    pub status: String,
}

async fn list_issues(
    State(state): State<SharedState>,
    Query(q): Query<IssuesQuery>,
) -> ApiResult<Json<Vec<Issue>>> {
    let (from, to) = q.range.resolve();
    let mut filters = vec![
        format!("e.timestamp >= toDateTime64('{}', 9)", crate::api::params::ch_dt(from)),
        format!("e.timestamp <= toDateTime64('{}', 9)", crate::api::params::ch_dt(to)),
    ];
    if let Some(p) = &q.range.project {
        if !p.is_empty() {
            filters.push(format!("e.project_id = '{}'", escape_sql(p)));
        }
    }
    if let Some(svc) = &q.service {
        filters.push(format!("e.service_name = '{}'", escape_sql(svc)));
    }

    let having = match q.status.as_deref() {
        Some(s) => format!(" HAVING coalesce(status, 'unresolved') = '{}'", escape_sql(s)),
        None => String::new(),
    };

    let sql = format!(
        "SELECT e.fingerprint AS fingerprint, e.service_name AS service_name, \
                argMax(e.exception_type, e.timestamp) AS exception_type, \
                argMax(e.message, e.timestamp) AS message, \
                toUInt64(count()) AS event_count, \
                toString(min(e.timestamp)) AS first_seen, \
                toString(max(e.timestamp)) AS last_seen, \
                coalesce(any(s.status), 'unresolved') AS status \
         FROM faro.error_events e \
         LEFT JOIN (SELECT service_name, fingerprint, argMax(status, version) AS status \
                    FROM faro.error_issue_status GROUP BY service_name, fingerprint) s \
                ON s.service_name = e.service_name AND s.fingerprint = e.fingerprint \
         WHERE {filters} \
         GROUP BY e.fingerprint, e.service_name{having} \
         ORDER BY last_seen DESC LIMIT {limit}",
        filters = filters.join(" AND "),
        limit = q.range.limit()
    );
    let rows: Vec<Issue> = state.ch.select(&sql).await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IssueDetail {
    pub issue: Issue,
    pub events: Vec<crate::storage::ErrorEventRow>,
}

async fn issue_detail(
    State(state): State<SharedState>,
    Path(fp): Path<String>,
) -> ApiResult<Json<IssueDetail>> {
    let fp = escape_sql(&fp);

    let issue_sql = format!(
        "SELECT e.fingerprint AS fingerprint, e.service_name AS service_name, \
                argMax(e.exception_type, e.timestamp) AS exception_type, \
                argMax(e.message, e.timestamp) AS message, \
                toUInt64(count()) AS event_count, \
                toString(min(e.timestamp)) AS first_seen, \
                toString(max(e.timestamp)) AS last_seen, \
                coalesce(any(s.status), 'unresolved') AS status \
         FROM faro.error_events e \
         LEFT JOIN (SELECT service_name, fingerprint, argMax(status, version) AS status \
                    FROM faro.error_issue_status GROUP BY service_name, fingerprint) s \
                ON s.service_name = e.service_name AND s.fingerprint = e.fingerprint \
         WHERE e.fingerprint = '{fp}' GROUP BY e.fingerprint, e.service_name LIMIT 1"
    );
    let issue: Option<Issue> = state.ch.select_one(&issue_sql).await?;
    let issue = issue.ok_or(crate::error::ApiError::NotFound)?;

    let events_sql = format!(
        "SELECT timestamp, fingerprint, service_name, severity_text, \
                message, exception_type, exception_message, stack_trace, trace_id, span_id, attributes \
         FROM faro.error_events WHERE fingerprint = '{fp}' ORDER BY timestamp DESC LIMIT 100"
    );
    let events = state.ch.select(&events_sql).await?;

    Ok(Json(IssueDetail { issue, events }))
}

#[derive(Debug, Deserialize)]
pub struct StatusUpdate {
    pub status: String,
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub note: String,
    pub service_name: String,
}

async fn update_status(
    State(state): State<SharedState>,
    Path(fp): Path<String>,
    Json(req): Json<StatusUpdate>,
) -> ApiResult<Json<serde_json::Value>> {
    if !matches!(req.status.as_str(), "unresolved" | "resolved" | "ignored") {
        return Err(crate::error::ApiError::BadRequest("estado inválido".into()));
    }
    let row = serde_json::json!({
        "service_name": req.service_name,
        "fingerprint": fp,
        "status": req.status,
        "assignee": req.assignee,
        "note": req.note,
        "updated_at": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "version": Utc::now().timestamp_millis() as u64,
    });
    state.ch.insert("faro.error_issue_status", &[row]).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}
