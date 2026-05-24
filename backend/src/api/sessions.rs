use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{ch_dt, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new().route("/sessions", get(list_sessions))
}

#[derive(Debug, Deserialize)]
pub struct SessionListQuery {
    #[serde(flatten)]
    pub range: Range,
    pub distinct_id: Option<String>,
    pub has_replay: Option<String>,
    pub has_error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductSessionSummary {
    pub project_id: String,
    pub session_id: String,
    pub distinct_id: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration_seconds: u32,
    pub pageview_count: u32,
    pub event_count: u32,
    pub error_count: u64,
    pub has_error: u8,
    pub has_replay: u8,
    pub replay_event_count: u64,
    pub replay_chunk_count: u32,
    pub source: String,
}

fn truthy(raw: Option<&str>) -> bool {
    matches!(
        raw.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("1" | "true" | "yes" | "y" | "on")
    )
}

async fn list_sessions(
    State(state): State<SharedState>,
    Query(q): Query<SessionListQuery>,
) -> ApiResult<Json<Vec<ProductSessionSummary>>> {
    let (from, to) = q.range.resolve();
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);

    let project_clause_ps = match &q.range.project {
        Some(p) if !p.is_empty() => " AND project_id = {project:String}",
        _ => "",
    };
    let project_clause_plain = project_clause_ps;
    let distinct_clause = match &q.distinct_id {
        Some(d) if !d.is_empty() => " AND distinct_id = {distinct_id:String}",
        _ => "",
    };

    let mut outer_filters = Vec::new();
    if truthy(q.has_replay.as_deref()) {
        outer_filters.push("has_replay = 1");
    }
    if truthy(q.has_error.as_deref()) {
        outer_filters.push("has_error = 1");
    }
    let outer_where = if outer_filters.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", outer_filters.join(" AND "))
    };

    let sql = format!(
        "WITH \
           replay_sessions AS ( \
             SELECT project_id, \
                    session_id, \
                    toUInt8(1) AS has_replay, \
                    toUInt64(sum(event_count)) AS replay_event_count, \
                    toUInt32(count()) AS replay_chunk_count \
             FROM faro.session_replays \
             WHERE timestamp >= toDateTime64({{from:DateTime64(3)}}, 3) \
               AND timestamp <= toDateTime64({{to:DateTime64(3)}}, 3){project_clause_plain} \
             GROUP BY project_id, session_id \
           ), \
           error_sessions AS ( \
             SELECT project_id, \
                    session_id, \
                    toUInt64(count()) AS error_count \
             FROM ( \
               SELECT project_id, \
                      multiIf( \
                        attributes['session.id'] != '', attributes['session.id'], \
                        attributes['session_id'] != '', attributes['session_id'], \
                        '' \
                      ) AS session_id \
               FROM faro.error_events \
               WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
                 AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9){project_clause_plain} \
             ) \
             WHERE session_id != '' \
             GROUP BY project_id, session_id \
           ) \
         SELECT * \
         FROM ( \
           SELECT ps.project_id AS project_id, \
                  ps.session_id AS session_id, \
                  ps.distinct_id AS distinct_id, \
                  toString(ps.started_at) AS started_at, \
                  toString(ps.ended_at) AS ended_at, \
                  ps.duration_seconds AS duration_seconds, \
                  ps.pageview_count AS pageview_count, \
                  ps.event_count AS event_count, \
                  ifNull(es.error_count, 0) AS error_count, \
                  toUInt8(ifNull(es.error_count, 0) > 0) AS has_error, \
                  ifNull(rs.has_replay, 0) AS has_replay, \
                  ifNull(rs.replay_event_count, 0) AS replay_event_count, \
                  ifNull(rs.replay_chunk_count, 0) AS replay_chunk_count, \
                  ps.source AS source \
           FROM faro.product_sessions FINAL AS ps \
           LEFT JOIN replay_sessions AS rs \
             ON rs.project_id = ps.project_id AND rs.session_id = ps.session_id \
           LEFT JOIN error_sessions AS es \
             ON es.project_id = ps.project_id AND es.session_id = ps.session_id \
           WHERE ps.ended_at >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND ps.started_at <= toDateTime64({{to:DateTime64(9)}}, 9) \
             AND ps.session_id != ''{project_clause_ps}{distinct_clause} \
         ){outer_where} \
         ORDER BY ended_at DESC \
         LIMIT {limit}",
        limit = q.range.limit()
    );

    let mut params: Vec<(&str, &str)> = vec![("from", from_s.as_str()), ("to", to_s.as_str())];
    if let Some(project) = &q.range.project {
        if !project.is_empty() {
            params.push(("project", project.as_str()));
        }
    }
    if let Some(distinct_id) = &q.distinct_id {
        if !distinct_id.is_empty() {
            params.push(("distinct_id", distinct_id.as_str()));
        }
    }

    let rows: Vec<ProductSessionSummary> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthy_accepts_common_true_values() {
        assert!(truthy(Some("1")));
        assert!(truthy(Some("true")));
        assert!(truthy(Some("YES")));
        assert!(truthy(Some("on")));
    }

    #[test]
    fn truthy_rejects_absent_or_false_values() {
        assert!(!truthy(None));
        assert!(!truthy(Some("")));
        assert!(!truthy(Some("0")));
        assert!(!truthy(Some("false")));
    }
}
