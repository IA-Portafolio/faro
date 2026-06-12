//! `GET /insights/revenue-impact` — impacto estimado en revenue por issue.
//!
//! Calcula, para cada fingerprint de error, cuántas sesiones "afectadas" (que
//! vieron el error) NO convirtieron al evento de checkout, y traduce eso a
//! revenue perdido comparándolo contra la baseline de conversion rate del
//! período.

use axum::extract::State;
use axum::Json;
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{ch_dt, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

use super::util::{
    conversion_rate, estimated_lost_revenue, DEFAULT_AVERAGE_ORDER_VALUE, DEFAULT_CHECKOUT_EVENT,
};

#[derive(Debug, Deserialize)]
pub struct RevenueImpactQuery {
    #[serde(flatten)]
    pub range: Range,
    pub checkout_event: Option<String>,
    pub average_order_value: Option<f64>,
    pub service: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevenueImpactIssue {
    pub fingerprint: String,
    pub service_name: String,
    pub exception_type: String,
    pub message: String,
    pub affected_sessions: u64,
    pub sessions_without_checkout: u64,
    pub issue_conversion_rate: f64,
    pub baseline_conversion_rate: f64,
    pub conversion_gap: f64,
    pub estimated_lost_revenue: f64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Deserialize)]
struct BaselineRow {
    total_sessions: u64,
    sessions_with_checkout: u64,
}

#[derive(Debug, Deserialize)]
struct ImpactRow {
    fingerprint: String,
    service_name: String,
    exception_type: String,
    message: String,
    affected_sessions: u64,
    sessions_without_checkout: u64,
    first_seen: String,
    last_seen: String,
}

pub async fn revenue_impact(
    State(state): State<SharedState>,
    Query(q): Query<RevenueImpactQuery>,
) -> ApiResult<Json<Vec<RevenueImpactIssue>>> {
    let checkout_event = q
        .checkout_event
        .as_deref()
        .unwrap_or(DEFAULT_CHECKOUT_EVENT)
        .trim();
    if checkout_event.is_empty() {
        return Err(ApiError::BadRequest(
            "checkout_event no puede ser vacío".into(),
        ));
    }

    let average_order_value = q.average_order_value.unwrap_or(DEFAULT_AVERAGE_ORDER_VALUE);
    if !average_order_value.is_finite() || average_order_value <= 0.0 {
        return Err(ApiError::BadRequest(
            "average_order_value debe ser un número positivo".into(),
        ));
    }

    let (from, to) = q.range.resolve();
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);

    let project_clause_plain = match &q.range.project {
        Some(p) if !p.is_empty() => " AND project_id = {project:String}",
        _ => "",
    };
    let project_clause_e = match &q.range.project {
        Some(p) if !p.is_empty() => " AND e.project_id = {project:String}",
        _ => "",
    };
    let service_clause = match &q.service {
        Some(s) if !s.is_empty() => " AND e.service_name = {service:String}",
        _ => "",
    };

    let mut params: Vec<(&str, &str)> = vec![
        ("from", from_s.as_str()),
        ("to", to_s.as_str()),
        ("checkout_event", checkout_event),
    ];
    if let Some(project) = &q.range.project {
        if !project.is_empty() {
            params.push(("project", project.as_str()));
        }
    }
    if let Some(service) = &q.service {
        if !service.is_empty() {
            params.push(("service", service.as_str()));
        }
    }

    let baseline_sql = format!(
        "WITH sessions AS ( \
           SELECT project_id, session_id, max(event_name = {{checkout_event:String}}) AS has_checkout \
           FROM faro.product_events \
           WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
             AND session_id != ''{project_clause_plain} \
           GROUP BY project_id, session_id \
         ) \
         SELECT toUInt64(count()) AS total_sessions, \
                toUInt64(sum(has_checkout)) AS sessions_with_checkout \
         FROM sessions"
    );

    let baseline = state
        .ch
        .select_one_with_params::<BaselineRow>(&baseline_sql, &params)
        .await?
        .unwrap_or(BaselineRow {
            total_sessions: 0,
            sessions_with_checkout: 0,
        });
    let baseline_conversion_rate = if baseline.total_sessions == 0 {
        0.0
    } else {
        baseline.sessions_with_checkout as f64 / baseline.total_sessions as f64
    };

    let impact_sql = format!(
        "WITH error_sessions AS ( \
           SELECT e.project_id AS project_id, \
                  e.session_id AS session_id, \
                  e.fingerprint AS fingerprint, \
                  e.service_name AS service_name, \
                  argMax(e.exception_type, e.timestamp) AS exception_type, \
                  argMax(e.message, e.timestamp) AS message, \
                  min(e.timestamp) AS first_error_at, \
                  min(e.timestamp) AS first_seen_ts, \
                  max(e.timestamp) AS last_seen_ts \
           FROM ( \
             SELECT timestamp, project_id, fingerprint, service_name, exception_type, message, \
                    attributes['session_id'] AS session_id \
             FROM faro.error_events \
             WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
               AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
           ) AS e \
           WHERE e.session_id != ''{project_clause_e}{service_clause} \
           GROUP BY e.project_id, e.session_id, e.fingerprint, e.service_name \
         ), \
         checkout_events AS ( \
           SELECT project_id, session_id, timestamp \
           FROM faro.product_events \
           WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
             AND event_name = {{checkout_event:String}} \
             AND session_id != ''{project_clause_plain} \
         ), \
         issue_sessions AS ( \
           SELECT es.fingerprint AS fingerprint, \
                  es.service_name AS service_name, \
                  es.session_id AS session_id, \
                  any(es.exception_type) AS exception_type, \
                  any(es.message) AS message, \
                  min(es.first_seen_ts) AS first_seen_ts, \
                  max(es.last_seen_ts) AS last_seen_ts, \
                  max(pe.timestamp > es.first_error_at) AS has_checkout_after_error \
           FROM error_sessions AS es \
           LEFT JOIN checkout_events AS pe \
             ON pe.project_id = es.project_id \
            AND pe.session_id = es.session_id \
           GROUP BY es.fingerprint, es.service_name, es.session_id \
         ) \
         SELECT fingerprint, \
                service_name, \
                any(exception_type) AS exception_type, \
                any(message) AS message, \
                toUInt64(count()) AS affected_sessions, \
                toUInt64(sum(if(has_checkout_after_error = 0, 1, 0))) AS sessions_without_checkout, \
                toString(min(first_seen_ts)) AS first_seen, \
                toString(max(last_seen_ts)) AS last_seen \
         FROM issue_sessions \
         GROUP BY fingerprint, service_name"
    );

    let rows: Vec<ImpactRow> = state.ch.select_with_params(&impact_sql, &params).await?;
    let mut issues: Vec<RevenueImpactIssue> = rows
        .into_iter()
        .filter(|row| row.affected_sessions > 0)
        .map(|row| {
            let issue_conversion_rate =
                conversion_rate(row.affected_sessions, row.sessions_without_checkout);
            let conversion_gap = (baseline_conversion_rate - issue_conversion_rate).max(0.0);
            let estimated_lost_revenue = estimated_lost_revenue(
                baseline_conversion_rate,
                issue_conversion_rate,
                row.affected_sessions,
                average_order_value,
            );

            RevenueImpactIssue {
                fingerprint: row.fingerprint,
                service_name: row.service_name,
                exception_type: row.exception_type,
                message: row.message,
                affected_sessions: row.affected_sessions,
                sessions_without_checkout: row.sessions_without_checkout,
                issue_conversion_rate,
                baseline_conversion_rate,
                conversion_gap,
                estimated_lost_revenue,
                first_seen: row.first_seen,
                last_seen: row.last_seen,
            }
        })
        .collect();

    issues.sort_by(|a, b| {
        b.estimated_lost_revenue
            .partial_cmp(&a.estimated_lost_revenue)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.sessions_without_checkout
                    .cmp(&a.sessions_without_checkout)
            })
    });
    issues.truncate(q.range.limit() as usize);

    Ok(Json(issues))
}
