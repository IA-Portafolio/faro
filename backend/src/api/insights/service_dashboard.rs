//! `GET /insights/service-dashboard` — resumen agregado por servicio.
//!
//! Combina 4 CTEs contra `faro.product_events`, `faro.error_events` y
//! `faro.spans` para responder: ¿cuánto convierte este servicio?, ¿qué
//! porcentaje de sesiones fallidas tienen errores linkeados?, ¿cuál es la
//! p95 del span crítico?, ¿cuáles son los top errors de esas sesiones
//! fallidas?

use axum::extract::State;
use axum::Json;
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{ch_dt, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

use super::util::{
    funnel_conversion_rate, service_dashboard_summary, DEFAULT_CHECKOUT_EVENT,
    DEFAULT_FUNNEL_FROM_EVENT,
};

#[derive(Debug, Deserialize)]
pub struct ServiceDashboardQuery {
    #[serde(flatten)]
    pub range: Range,
    pub service: Option<String>,
    pub span_name: Option<String>,
    pub funnel_from: Option<String>,
    pub funnel_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceDashboardInsight {
    pub project: String,
    pub service_name: String,
    pub span_name: String,
    pub funnel_from: String,
    pub funnel_to: String,
    pub started_events: u64,
    pub completed_events: u64,
    pub conversion_rate: f64,
    pub started_sessions: u64,
    pub completed_sessions: u64,
    pub failed_sessions: u64,
    pub linked_error_count: u64,
    pub linked_error_sessions: u64,
    pub p95_latency_ms: f64,
    pub span_count: u64,
    pub summary: String,
    pub top_errors: Vec<ServiceDashboardIssue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceDashboardIssue {
    pub fingerprint: String,
    pub service_name: String,
    pub exception_type: String,
    pub message: String,
    pub error_count: u64,
    pub affected_failed_sessions: u64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Deserialize)]
struct DashboardEventCountsRow {
    started_events: u64,
    completed_events: u64,
}

#[derive(Debug, Deserialize)]
struct DashboardSessionCountsRow {
    started_sessions: u64,
    completed_sessions: u64,
    failed_sessions: u64,
}

#[derive(Debug, Deserialize)]
struct DashboardLinkedErrorsRow {
    linked_error_count: u64,
    linked_error_sessions: u64,
}

#[derive(Debug, Deserialize)]
struct DashboardLatencyRow {
    span_count: u64,
    p95_latency_ms: f64,
}

pub async fn service_dashboard(
    State(state): State<SharedState>,
    Query(q): Query<ServiceDashboardQuery>,
) -> ApiResult<Json<ServiceDashboardInsight>> {
    let service = q.service.as_deref().unwrap_or("checkout").trim();
    let span_name = q.span_name.as_deref().unwrap_or("/api/checkout").trim();
    let funnel_from = q
        .funnel_from
        .as_deref()
        .unwrap_or(DEFAULT_FUNNEL_FROM_EVENT)
        .trim();
    let funnel_to = q
        .funnel_to
        .as_deref()
        .unwrap_or(DEFAULT_CHECKOUT_EVENT)
        .trim();

    if service.is_empty() {
        return Err(ApiError::BadRequest("service no puede ser vacío".into()));
    }
    if span_name.is_empty() {
        return Err(ApiError::BadRequest("span_name no puede ser vacío".into()));
    }
    if funnel_from.is_empty() || funnel_to.is_empty() {
        return Err(ApiError::BadRequest(
            "funnel_from y funnel_to no pueden ser vacíos".into(),
        ));
    }
    if funnel_from == funnel_to {
        return Err(ApiError::BadRequest(
            "funnel_from y funnel_to deben ser distintos".into(),
        ));
    }

    let (from, to) = q.range.resolve();
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);
    let limit = q.range.limit();

    let project_clause_plain = match &q.range.project {
        Some(p) if !p.is_empty() => " AND project_id = {project:String}",
        _ => "",
    };
    let service_clause_plain = " AND service_name = {service:String}";

    let mut params: Vec<(&str, &str)> = vec![
        ("from", from_s.as_str()),
        ("to", to_s.as_str()),
        ("service", service),
        ("span_name", span_name),
        ("funnel_from", funnel_from),
        ("funnel_to", funnel_to),
    ];
    if let Some(project) = &q.range.project {
        if !project.is_empty() {
            params.push(("project", project.as_str()));
        }
    }

    let events_sql = format!(
        "SELECT toUInt64(countIf(event_name = {{funnel_from:String}})) AS started_events, \
                toUInt64(countIf(event_name = {{funnel_to:String}})) AS completed_events \
         FROM faro.product_events \
         WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
           AND event_name IN ({{funnel_from:String}}, {{funnel_to:String}}){project_clause_plain}"
    );
    let event_counts = state
        .ch
        .select_one_with_params::<DashboardEventCountsRow>(&events_sql, &params)
        .await?
        .unwrap_or(DashboardEventCountsRow {
            started_events: 0,
            completed_events: 0,
        });

    let sessions_sql = format!(
        "WITH funnel_sessions AS ( \
           SELECT project_id, session_id, \
                  countIf(event_name = {{funnel_from:String}}) AS started_count, \
                  countIf(event_name = {{funnel_to:String}}) AS completed_count \
           FROM faro.product_events \
           WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
             AND session_id != '' \
             AND event_name IN ({{funnel_from:String}}, {{funnel_to:String}}){project_clause_plain} \
           GROUP BY project_id, session_id \
         ) \
         SELECT toUInt64(countIf(started_count > 0)) AS started_sessions, \
                toUInt64(countIf(completed_count > 0)) AS completed_sessions, \
                toUInt64(countIf(started_count > 0 AND completed_count = 0)) AS failed_sessions \
         FROM funnel_sessions"
    );
    let session_counts = state
        .ch
        .select_one_with_params::<DashboardSessionCountsRow>(&sessions_sql, &params)
        .await?
        .unwrap_or(DashboardSessionCountsRow {
            started_sessions: 0,
            completed_sessions: 0,
            failed_sessions: 0,
        });

    let failed_sessions_cte = format!(
        "failed_sessions AS ( \
           SELECT project_id, session_id \
           FROM faro.product_events \
           WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
             AND session_id != '' \
             AND event_name IN ({{funnel_from:String}}, {{funnel_to:String}}){project_clause_plain} \
           GROUP BY project_id, session_id \
           HAVING countIf(event_name = {{funnel_from:String}}) > 0 \
              AND countIf(event_name = {{funnel_to:String}}) = 0 \
         )"
    );
    let error_rows_cte = format!(
        "error_rows AS ( \
           SELECT timestamp, project_id, fingerprint, service_name, exception_type, message, \
                  multiIf(attributes['session.id'] != '', attributes['session.id'], \
                          attributes['session_id'] != '', attributes['session_id'], '') AS session_id \
           FROM faro.error_events \
           WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9){project_clause_plain}{service_clause_plain} \
         )"
    );

    let linked_sql = format!(
        "WITH {failed_sessions_cte}, {error_rows_cte} \
         SELECT toUInt64(count()) AS linked_error_count, \
                toUInt64(uniqExact(e.session_id)) AS linked_error_sessions \
         FROM error_rows AS e \
         INNER JOIN failed_sessions AS fs \
           ON fs.project_id = e.project_id AND fs.session_id = e.session_id \
         WHERE e.session_id != ''"
    );
    let linked_errors = state
        .ch
        .select_one_with_params::<DashboardLinkedErrorsRow>(&linked_sql, &params)
        .await?
        .unwrap_or(DashboardLinkedErrorsRow {
            linked_error_count: 0,
            linked_error_sessions: 0,
        });

    let top_errors_sql = format!(
        "WITH {failed_sessions_cte}, {error_rows_cte} \
         SELECT fingerprint, \
                any(service_name) AS service_name, \
                any(exception_type) AS exception_type, \
                any(message) AS message, \
                toUInt64(count()) AS error_count, \
                toUInt64(uniqExact(e.session_id)) AS affected_failed_sessions, \
                toString(min(timestamp)) AS first_seen, \
                toString(max(timestamp)) AS last_seen \
         FROM error_rows AS e \
         INNER JOIN failed_sessions AS fs \
           ON fs.project_id = e.project_id AND fs.session_id = e.session_id \
         WHERE e.session_id != '' \
         GROUP BY fingerprint \
         ORDER BY affected_failed_sessions DESC, error_count DESC \
         LIMIT {limit}"
    );
    let top_errors: Vec<ServiceDashboardIssue> = state
        .ch
        .select_with_params(&top_errors_sql, &params)
        .await?;

    let latency_sql = format!(
        "SELECT toUInt64(count()) AS span_count, \
                if(count() = 0, 0.0, toFloat64(quantileExact(0.95)(duration_ns)) / 1000000.0) AS p95_latency_ms \
         FROM faro.spans \
         WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
           AND name = {{span_name:String}}{project_clause_plain}{service_clause_plain}"
    );
    let latency = state
        .ch
        .select_one_with_params::<DashboardLatencyRow>(&latency_sql, &params)
        .await?
        .unwrap_or(DashboardLatencyRow {
            span_count: 0,
            p95_latency_ms: 0.0,
        });

    let conversion_rate =
        funnel_conversion_rate(event_counts.started_events, event_counts.completed_events);
    let project = q.range.project.clone().unwrap_or_default();
    let summary = service_dashboard_summary(
        service,
        funnel_from,
        funnel_to,
        event_counts.started_events,
        event_counts.completed_events,
        linked_errors.linked_error_sessions,
        session_counts.failed_sessions,
        span_name,
        latency.p95_latency_ms,
    );

    Ok(Json(ServiceDashboardInsight {
        project,
        service_name: service.to_string(),
        span_name: span_name.to_string(),
        funnel_from: funnel_from.to_string(),
        funnel_to: funnel_to.to_string(),
        started_events: event_counts.started_events,
        completed_events: event_counts.completed_events,
        conversion_rate,
        started_sessions: session_counts.started_sessions,
        completed_sessions: session_counts.completed_sessions,
        failed_sessions: session_counts.failed_sessions,
        linked_error_count: linked_errors.linked_error_count,
        linked_error_sessions: linked_errors.linked_error_sessions,
        p95_latency_ms: latency.p95_latency_ms,
        span_count: latency.span_count,
        summary,
        top_errors,
    }))
}
