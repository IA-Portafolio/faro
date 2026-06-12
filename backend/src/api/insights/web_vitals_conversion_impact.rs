//! `GET /insights/web-vitals-conversion-impact` — efecto de web vitals
//! (LCP / FID / CLS / INP) en la conversión.
//!
//! Clasifica cada session en "slow" (métrica > threshold) o "baseline", y
//! compara las tasas de conversión al evento de checkout entre ambos
//! cohorts.

use axum::extract::State;
use axum::Json;
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{ch_dt, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

use super::util::{
    conversion_drop_points, funnel_conversion_rate, web_vitals_conversion_summary,
    DEFAULT_CHECKOUT_EVENT, DEFAULT_PAGEVIEW_EVENT, DEFAULT_WEB_VITAL_METRIC,
    DEFAULT_WEB_VITAL_THRESHOLD_MS,
};

#[derive(Debug, Deserialize)]
pub struct WebVitalsConversionImpactQuery {
    #[serde(flatten)]
    pub range: Range,
    pub metric: Option<String>,
    pub threshold_ms: Option<f64>,
    pub conversion_event: Option<String>,
    pub pageview_event: Option<String>,
    pub service: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebVitalsConversionImpactResult {
    pub metric: String,
    pub threshold_ms: f64,
    pub pageview_event: String,
    pub conversion_event: String,
    pub service_name: String,
    pub slow_sessions: u64,
    pub baseline_sessions: u64,
    pub slow_users: u64,
    pub baseline_users: u64,
    pub slow_pageviews: u64,
    pub baseline_pageviews: u64,
    pub slow_conversions: u64,
    pub baseline_conversions: u64,
    pub slow_conversion_rate: f64,
    pub baseline_conversion_rate: f64,
    pub conversion_drop_points: f64,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
struct WebVitalsConversionImpactRow {
    slow_sessions: u64,
    baseline_sessions: u64,
    slow_users: u64,
    baseline_users: u64,
    slow_pageviews: u64,
    baseline_pageviews: u64,
    slow_conversions: u64,
    baseline_conversions: u64,
}

pub async fn web_vitals_conversion_impact(
    State(state): State<SharedState>,
    Query(q): Query<WebVitalsConversionImpactQuery>,
) -> ApiResult<Json<WebVitalsConversionImpactResult>> {
    let metric = q
        .metric
        .as_deref()
        .unwrap_or(DEFAULT_WEB_VITAL_METRIC)
        .trim()
        .to_ascii_uppercase();
    if metric.is_empty() {
        return Err(ApiError::BadRequest("metric no puede ser vacío".into()));
    }

    let threshold_ms = q.threshold_ms.unwrap_or(DEFAULT_WEB_VITAL_THRESHOLD_MS);
    if !threshold_ms.is_finite() || threshold_ms <= 0.0 {
        return Err(ApiError::BadRequest(
            "threshold_ms debe ser un número positivo".into(),
        ));
    }

    let conversion_event = q
        .conversion_event
        .as_deref()
        .unwrap_or(DEFAULT_CHECKOUT_EVENT)
        .trim();
    let pageview_event = q
        .pageview_event
        .as_deref()
        .unwrap_or(DEFAULT_PAGEVIEW_EVENT)
        .trim();
    if conversion_event.is_empty() || pageview_event.is_empty() {
        return Err(ApiError::BadRequest(
            "conversion_event y pageview_event no pueden ser vacíos".into(),
        ));
    }

    let (from, to) = q.range.resolve();
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);
    let threshold_ms_s = threshold_ms.to_string();

    let project_clause_plain = match &q.range.project {
        Some(p) if !p.is_empty() => " AND project_id = {project:String}",
        _ => "",
    };
    let service_clause = match &q.service {
        Some(s) if !s.is_empty() => " AND service_name = {service:String}",
        _ => "",
    };

    let mut params: Vec<(&str, &str)> = vec![
        ("from", from_s.as_str()),
        ("to", to_s.as_str()),
        ("metric", metric.as_str()),
        ("threshold_ms", threshold_ms_s.as_str()),
        ("conversion_event", conversion_event),
        ("pageview_event", pageview_event),
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

    let sql = format!(
        "WITH vital_sessions AS ( \
           SELECT project_id, \
                  session_id, \
                  max(toFloat64OrZero(metric_value_raw)) AS metric_value \
           FROM ( \
             SELECT project_id, \
                    if(attributes['session.id'] != '', attributes['session.id'], attributes['session_id']) AS session_id, \
                    attributes['metric.value'] AS metric_value_raw \
             FROM faro.logs \
             WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
               AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
               AND attributes['metric.name'] = {{metric:String}}{project_clause_plain}{service_clause} \
           ) \
           WHERE session_id != '' AND metric_value_raw != '' \
           GROUP BY project_id, session_id \
         ), \
         product_sessions AS ( \
           SELECT project_id, \
                  session_id, \
                  anyIf(distinct_id, distinct_id != '') AS distinct_id, \
                  toUInt64(countIf(event_name = {{pageview_event:String}})) AS pageviews, \
                  max(event_name = {{pageview_event:String}}) AS has_pageview, \
                  max(event_name = {{conversion_event:String}}) AS has_conversion \
           FROM faro.product_events \
           WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
             AND session_id != '' \
             AND event_name IN ({{pageview_event:String}}, {{conversion_event:String}}){project_clause_plain} \
           GROUP BY project_id, session_id \
         ), \
         joined AS ( \
           SELECT v.project_id AS project_id, \
                  v.session_id AS session_id, \
                  p.distinct_id AS distinct_id, \
                  p.pageviews AS pageviews, \
                  p.has_conversion AS has_conversion, \
                  if(v.metric_value > {{threshold_ms:Float64}}, 1, 0) AS slow \
           FROM vital_sessions AS v \
           INNER JOIN product_sessions AS p \
              ON p.project_id = v.project_id \
             AND p.session_id = v.session_id \
           WHERE p.has_pageview = 1 \
         ) \
         SELECT toUInt64(countIf(slow = 1)) AS slow_sessions, \
                toUInt64(countIf(slow = 0)) AS baseline_sessions, \
                toUInt64(uniqExactIf(distinct_id, slow = 1 AND distinct_id != '')) AS slow_users, \
                toUInt64(uniqExactIf(distinct_id, slow = 0 AND distinct_id != '')) AS baseline_users, \
                toUInt64(sumIf(pageviews, slow = 1)) AS slow_pageviews, \
                toUInt64(sumIf(pageviews, slow = 0)) AS baseline_pageviews, \
                toUInt64(sumIf(has_conversion, slow = 1)) AS slow_conversions, \
                toUInt64(sumIf(has_conversion, slow = 0)) AS baseline_conversions \
         FROM joined"
    );

    let row = state
        .ch
        .select_one_with_params::<WebVitalsConversionImpactRow>(&sql, &params)
        .await?
        .unwrap_or(WebVitalsConversionImpactRow {
            slow_sessions: 0,
            baseline_sessions: 0,
            slow_users: 0,
            baseline_users: 0,
            slow_pageviews: 0,
            baseline_pageviews: 0,
            slow_conversions: 0,
            baseline_conversions: 0,
        });

    let slow_conversion_rate = funnel_conversion_rate(row.slow_sessions, row.slow_conversions);
    let baseline_conversion_rate =
        funnel_conversion_rate(row.baseline_sessions, row.baseline_conversions);
    let drop_points = conversion_drop_points(baseline_conversion_rate, slow_conversion_rate);
    let service_name = q.service.clone().unwrap_or_default();

    Ok(Json(WebVitalsConversionImpactResult {
        metric: metric.clone(),
        threshold_ms,
        pageview_event: pageview_event.to_string(),
        conversion_event: conversion_event.to_string(),
        service_name,
        slow_sessions: row.slow_sessions,
        baseline_sessions: row.baseline_sessions,
        slow_users: row.slow_users,
        baseline_users: row.baseline_users,
        slow_pageviews: row.slow_pageviews,
        baseline_pageviews: row.baseline_pageviews,
        slow_conversions: row.slow_conversions,
        baseline_conversions: row.baseline_conversions,
        slow_conversion_rate,
        baseline_conversion_rate,
        conversion_drop_points: drop_points,
        summary: web_vitals_conversion_summary(&metric, threshold_ms, drop_points),
    }))
}
