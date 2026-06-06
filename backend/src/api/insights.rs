//! Endpoints de "insights" (hallazgos de negocio por servicio):
//!   GET /insights/service-dashboard            → resumen combinado por servicio
//!   GET /insights/revenue-impact               → impacto estimado en ingresos
//!   GET /insights/latency-funnel-impact        → latencia vs conversión del funnel
//!   GET /insights/web-vitals-conversion-impact → web vitals vs conversión
//!
//! Cruzan product events, funnels, errores y latencia para estimar impacto.

use std::collections::BTreeMap;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{ch_dt, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

const DEFAULT_CHECKOUT_EVENT: &str = "checkout_completed";
const DEFAULT_AVERAGE_ORDER_VALUE: f64 = 100.0;
const DEFAULT_FUNNEL_FROM_EVENT: &str = "checkout_started";
const DEFAULT_LATENCY_THRESHOLD_MS: u32 = 2_000;
const DEFAULT_WEB_VITAL_METRIC: &str = "LCP";
const DEFAULT_WEB_VITAL_THRESHOLD_MS: f64 = 4_000.0;
const DEFAULT_PAGEVIEW_EVENT: &str = "$pageview";
const DEFAULT_BUCKET_MINUTES: u32 = 60;
const MAX_BUCKET_MINUTES: u32 = 24 * 60;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/insights/service-dashboard", get(service_dashboard))
        .route("/insights/revenue-impact", get(revenue_impact))
        .route(
            "/insights/latency-funnel-impact",
            get(latency_funnel_impact),
        )
        .route(
            "/insights/web-vitals-conversion-impact",
            get(web_vitals_conversion_impact),
        )
}

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

#[derive(Debug, Deserialize)]
pub struct LatencyFunnelImpactQuery {
    #[serde(flatten)]
    pub range: Range,
    pub span_name: String,
    pub service: Option<String>,
    pub funnel_from: Option<String>,
    pub funnel_to: Option<String>,
    pub latency_threshold_ms: Option<u32>,
    pub bucket_minutes: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LatencyFunnelImpactResult {
    pub span_name: String,
    pub service_name: String,
    pub funnel_from: String,
    pub funnel_to: String,
    pub bucket_minutes: u32,
    pub p95_threshold_ms: u32,
    pub slow_bucket_count: u32,
    pub baseline_bucket_count: u32,
    pub baseline_conversion_rate: f64,
    pub slow_conversion_rate: f64,
    pub conversion_drop_points: f64,
    pub summary: String,
    pub buckets: Vec<LatencyFunnelBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyFunnelBucket {
    pub bucket_start: String,
    pub p95_latency_ms: f64,
    pub funnel_started: u64,
    pub funnel_completed: u64,
    pub conversion_rate: f64,
    pub slow: bool,
}

#[derive(Debug, Deserialize)]
struct SpanLatencyBucketRow {
    bucket_start: String,
    p95_latency_ms: f64,
}

#[derive(Debug, Deserialize)]
struct FunnelBucketRow {
    bucket_start: String,
    funnel_started: u64,
    funnel_completed: u64,
}

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

async fn revenue_impact(
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

async fn latency_funnel_impact(
    State(state): State<SharedState>,
    Query(q): Query<LatencyFunnelImpactQuery>,
) -> ApiResult<Json<LatencyFunnelImpactResult>> {
    let span_name = q.span_name.trim();
    if span_name.is_empty() {
        return Err(ApiError::BadRequest("span_name no puede ser vacío".into()));
    }

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

    let threshold_ms = q
        .latency_threshold_ms
        .unwrap_or(DEFAULT_LATENCY_THRESHOLD_MS);
    if threshold_ms == 0 {
        return Err(ApiError::BadRequest(
            "latency_threshold_ms debe ser positivo".into(),
        ));
    }
    let bucket_minutes = q
        .bucket_minutes
        .unwrap_or(DEFAULT_BUCKET_MINUTES)
        .clamp(1, MAX_BUCKET_MINUTES);
    let bucket_seconds = (bucket_minutes * 60).to_string();

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
    let service_clause = match &q.service {
        Some(s) if !s.is_empty() => " AND service_name = {service:String}",
        _ => "",
    };

    let mut params: Vec<(&str, &str)> = vec![
        ("from", from_s.as_str()),
        ("to", to_s.as_str()),
        ("span_name", span_name),
        ("funnel_from", funnel_from),
        ("funnel_to", funnel_to),
        ("bucket_seconds", bucket_seconds.as_str()),
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

    let span_sql = format!(
        "SELECT toString(toStartOfInterval(timestamp, toIntervalSecond({{bucket_seconds:UInt32}}))) AS bucket_start, \
                toFloat64(quantileExact(0.95)(duration_ns)) / 1000000.0 AS p95_latency_ms \
         FROM faro.spans \
         WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
           AND name = {{span_name:String}}{project_clause_plain}{service_clause} \
         GROUP BY bucket_start \
         ORDER BY bucket_start"
    );
    let span_rows: Vec<SpanLatencyBucketRow> =
        state.ch.select_with_params(&span_sql, &params).await?;

    let funnel_sql = format!(
        "SELECT toString(toStartOfInterval(timestamp, toIntervalSecond({{bucket_seconds:UInt32}}))) AS bucket_start, \
                toUInt64(uniqExactIf(distinct_id, event_name = {{funnel_from:String}})) AS funnel_started, \
                toUInt64(uniqExactIf(distinct_id, event_name = {{funnel_to:String}})) AS funnel_completed \
         FROM faro.product_events \
         WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
           AND event_name IN ({{funnel_from:String}}, {{funnel_to:String}}){project_clause_plain} \
         GROUP BY bucket_start \
         ORDER BY bucket_start"
    );
    let funnel_rows: Vec<FunnelBucketRow> =
        state.ch.select_with_params(&funnel_sql, &params).await?;

    let mut buckets_by_start: BTreeMap<String, LatencyFunnelBucket> = BTreeMap::new();
    for row in span_rows {
        buckets_by_start.insert(
            row.bucket_start.clone(),
            LatencyFunnelBucket {
                bucket_start: row.bucket_start,
                p95_latency_ms: row.p95_latency_ms,
                funnel_started: 0,
                funnel_completed: 0,
                conversion_rate: 0.0,
                slow: row.p95_latency_ms >= threshold_ms as f64,
            },
        );
    }
    for row in funnel_rows {
        let bucket = buckets_by_start
            .entry(row.bucket_start.clone())
            .or_insert_with(|| LatencyFunnelBucket {
                bucket_start: row.bucket_start,
                p95_latency_ms: 0.0,
                funnel_started: 0,
                funnel_completed: 0,
                conversion_rate: 0.0,
                slow: false,
            });
        bucket.funnel_started = row.funnel_started;
        bucket.funnel_completed = row.funnel_completed;
    }

    let mut buckets: Vec<LatencyFunnelBucket> = buckets_by_start
        .into_values()
        .map(|mut bucket| {
            bucket.conversion_rate =
                funnel_conversion_rate(bucket.funnel_started, bucket.funnel_completed);
            bucket.slow = bucket.p95_latency_ms >= threshold_ms as f64;
            bucket
        })
        .collect();

    let mut slow_started = 0u64;
    let mut slow_completed = 0u64;
    let mut baseline_started = 0u64;
    let mut baseline_completed = 0u64;
    let mut slow_bucket_count = 0u32;
    let mut baseline_bucket_count = 0u32;
    for bucket in &buckets {
        if bucket.funnel_started == 0 {
            continue;
        }
        if bucket.slow {
            slow_bucket_count += 1;
            slow_started += bucket.funnel_started;
            slow_completed += bucket.funnel_completed;
        } else {
            baseline_bucket_count += 1;
            baseline_started += bucket.funnel_started;
            baseline_completed += bucket.funnel_completed;
        }
    }

    let baseline_conversion_rate = funnel_conversion_rate(baseline_started, baseline_completed);
    let slow_conversion_rate = funnel_conversion_rate(slow_started, slow_completed);
    let drop_points = conversion_drop_points(baseline_conversion_rate, slow_conversion_rate);

    buckets.truncate(q.range.limit() as usize);
    let service_name = q.service.clone().unwrap_or_default();

    Ok(Json(LatencyFunnelImpactResult {
        span_name: span_name.to_string(),
        service_name,
        funnel_from: funnel_from.to_string(),
        funnel_to: funnel_to.to_string(),
        bucket_minutes,
        p95_threshold_ms: threshold_ms,
        slow_bucket_count,
        baseline_bucket_count,
        baseline_conversion_rate,
        slow_conversion_rate,
        conversion_drop_points: drop_points,
        summary: latency_funnel_summary(span_name, threshold_ms, drop_points),
        buckets,
    }))
}

async fn web_vitals_conversion_impact(
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

async fn service_dashboard(
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

fn conversion_rate(affected_sessions: u64, sessions_without_checkout: u64) -> f64 {
    if affected_sessions == 0 {
        return 0.0;
    }
    let completed = affected_sessions.saturating_sub(sessions_without_checkout);
    completed as f64 / affected_sessions as f64
}

fn estimated_lost_revenue(
    baseline_conversion_rate: f64,
    issue_conversion_rate: f64,
    affected_sessions: u64,
    average_order_value: f64,
) -> f64 {
    let gap = (baseline_conversion_rate - issue_conversion_rate).max(0.0);
    gap * affected_sessions as f64 * average_order_value
}

fn funnel_conversion_rate(started: u64, completed: u64) -> f64 {
    if started == 0 {
        return 0.0;
    }
    completed as f64 / started as f64
}

fn conversion_drop_points(baseline_conversion_rate: f64, slow_conversion_rate: f64) -> f64 {
    (baseline_conversion_rate - slow_conversion_rate).max(0.0) * 100.0
}

fn latency_funnel_summary(span_name: &str, latency_threshold_ms: u32, drop_points: f64) -> String {
    let threshold = if latency_threshold_ms % 1_000 == 0 {
        format!("{}s", latency_threshold_ms / 1_000)
    } else {
        format!("{:.1}s", latency_threshold_ms as f64 / 1_000.0)
    };
    format!(
        "Cuando {span_name} p95 supera {threshold}, el funnel checkout cae {:.0} puntos.",
        drop_points
    )
}

fn web_vitals_conversion_summary(metric: &str, threshold_ms: f64, drop_points: f64) -> String {
    format!(
        "Los usuarios con {metric} > {} convierten {:.0} puntos menos.",
        threshold_label(threshold_ms),
        drop_points
    )
}

fn threshold_label(threshold_ms: f64) -> String {
    let seconds = threshold_ms / 1_000.0;
    if seconds.fract().abs() < f64::EPSILON {
        format!("{seconds:.0}s")
    } else {
        format!("{seconds:.1}s")
    }
}

fn service_dashboard_summary(
    service: &str,
    funnel_from: &str,
    funnel_to: &str,
    started_events: u64,
    completed_events: u64,
    linked_error_sessions: u64,
    failed_sessions: u64,
    span_name: &str,
    p95_latency_ms: f64,
) -> String {
    let conversion = funnel_conversion_rate(started_events, completed_events) * 100.0;
    format!(
        "{service}: {completed_events}/{started_events} {funnel_to} desde {funnel_from} ({conversion:.1}%). {linked_error_sessions} de {failed_sessions} sesiones fallidas tienen errores linkeados; p95 {span_name}: {p95_latency_ms:.0}ms."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_rate_counts_completed_sessions_over_affected_sessions() {
        let rate = conversion_rate(12, 5);

        assert!((rate - 7.0 / 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn conversion_rate_is_zero_when_there_are_no_affected_sessions() {
        assert_eq!(conversion_rate(0, 0), 0.0);
    }

    #[test]
    fn estimated_lost_revenue_uses_positive_conversion_gap() {
        let lost = estimated_lost_revenue(0.71, 0.60, 1_247, 100.0);

        assert!((lost - 13_717.0).abs() < 0.0001);
    }

    #[test]
    fn estimated_lost_revenue_clamps_when_issue_outperforms_baseline() {
        let lost = estimated_lost_revenue(0.40, 0.60, 1_247, 100.0);

        assert_eq!(lost, 0.0);
    }

    #[test]
    fn funnel_conversion_rate_counts_completed_over_started() {
        let rate = funnel_conversion_rate(10, 7);

        assert!((rate - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn funnel_conversion_rate_is_zero_when_no_one_started() {
        assert_eq!(funnel_conversion_rate(0, 7), 0.0);
    }

    #[test]
    fn conversion_drop_points_clamps_negative_drop() {
        assert_eq!(conversion_drop_points(0.50, 0.75), 0.0);
    }

    #[test]
    fn conversion_drop_points_returns_percentage_points() {
        let points = conversion_drop_points(0.71, 0.59);

        assert!((points - 12.0).abs() < 0.0001);
    }

    #[test]
    fn latency_funnel_summary_formats_threshold_and_drop() {
        let text = latency_funnel_summary("/api/checkout", 2_000, 12.0);

        assert_eq!(
            text,
            "Cuando /api/checkout p95 supera 2s, el funnel checkout cae 12 puntos."
        );
    }

    #[test]
    fn web_vitals_conversion_summary_formats_threshold_and_drop() {
        let text = web_vitals_conversion_summary("LCP", 4_000.0, 13.0);

        assert_eq!(
            text,
            "Los usuarios con LCP > 4s convierten 13 puntos menos."
        );
    }

    #[test]
    fn service_dashboard_summary_links_events_errors_and_latency() {
        let text = service_dashboard_summary(
            "checkout",
            "checkout_started",
            "checkout_completed",
            12_453,
            8_901,
            18,
            3_552,
            "/api/checkout",
            230.0,
        );

        assert_eq!(
            text,
            "checkout: 8901/12453 checkout_completed desde checkout_started (71.5%). 18 de 3552 sesiones fallidas tienen errores linkeados; p95 /api/checkout: 230ms."
        );
    }
}
