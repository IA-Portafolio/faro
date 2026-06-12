//! `GET /insights/latency-funnel-impact` — efecto de la latencia en la conversión.
//!
//! Para cada bucket temporal, calcula p95 de un span y la conversión del
//! funnel `funnel_from → funnel_to`. Compara los buckets "lentos" (p95 ≥
//! threshold) contra los buckets baseline y reporta la caída de conversión
//! en puntos porcentuales.

use std::collections::BTreeMap;

use axum::extract::State;
use axum::Json;
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{ch_dt, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

use super::util::{
    conversion_drop_points, funnel_conversion_rate, latency_funnel_summary, DEFAULT_BUCKET_MINUTES,
    DEFAULT_CHECKOUT_EVENT, DEFAULT_FUNNEL_FROM_EVENT, DEFAULT_LATENCY_THRESHOLD_MS,
    MAX_BUCKET_MINUTES,
};

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

pub async fn latency_funnel_impact(
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
