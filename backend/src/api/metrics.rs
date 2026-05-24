use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{de_opt_num, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

const EVENT_METRIC_PREFIX: &str = "events.";
const EVENT_METRIC_SUFFIX: &str = ".count";

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/metrics/series", get(query_series))
        .route("/metrics/names", get(list_names))
}

#[derive(Debug, Deserialize)]
pub struct SeriesQuery {
    #[serde(flatten)]
    pub range: Range,
    pub name: String,
    pub service: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num")]
    pub bucket_seconds: Option<u32>,
    pub agg: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Point {
    pub ts: String,
    pub value: f64,
}

async fn query_series(
    State(state): State<SharedState>,
    Query(q): Query<SeriesQuery>,
) -> ApiResult<Json<Vec<Point>>> {
    if let Some(event_name) = event_name_from_metric(&q.name) {
        return query_event_series(state, q, event_name).await;
    }

    let (from, to) = q.range.resolve();
    let bucket = q.bucket_seconds.unwrap_or(60).max(1);
    let agg = q.agg.as_deref().unwrap_or("avg");
    // Whitelist explícita — `agg_expr` se concatena al SQL como fragmento, así que
    // sólo se permiten valores conocidos.
    let agg_expr = match agg {
        "sum" => "sum(value)",
        "max" => "max(value)",
        "min" => "min(value)",
        "count" => "toFloat64(count())",
        _ => "avg(value)",
    };

    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);
    let (proj_clause, proj_value) = q.range.project_clause("");

    let mut params: Vec<(&str, &str)> =
        vec![("name", q.name.as_str()), ("from", &from_s), ("to", &to_s)];
    let mut svc_clause = String::new();
    if let Some(s) = &q.service {
        svc_clause.push_str(" AND service_name = {service:String}");
        params.push(("service", s));
    }
    if let Some(p) = proj_value {
        params.push(("project", p));
    }

    let sql = format!(
        "SELECT toString(toStartOfInterval(timestamp, INTERVAL {bucket} second)) AS ts, \
                toFloat64({agg_expr}) AS value \
         FROM faro.metrics WHERE metric_name = {{name:String}} \
           AND timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9){svc_clause}{proj_clause} \
         GROUP BY ts ORDER BY ts",
    );
    let rows: Vec<Point> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

async fn query_event_series(
    state: SharedState,
    q: SeriesQuery,
    event_name: String,
) -> ApiResult<Json<Vec<Point>>> {
    let (from, to) = q.range.resolve();
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }

    let bucket = q.bucket_seconds.unwrap_or(60).max(1);
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);
    let (proj_clause, proj_value) = q.range.project_clause("");

    let mut params: Vec<(&str, &str)> = vec![
        ("event_name", event_name.as_str()),
        ("from", &from_s),
        ("to", &to_s),
    ];
    let mut source_clause = String::new();
    if let Some(s) = &q.service {
        if !s.is_empty() {
            source_clause.push_str(" AND source = {service:String}");
            params.push(("service", s));
        }
    }
    if let Some(p) = proj_value {
        params.push(("project", p));
    }

    let sql = format!(
        "SELECT toString(toStartOfInterval(timestamp, INTERVAL {bucket} second)) AS ts, \
                toFloat64(count()) AS value \
         FROM faro.product_events \
         WHERE event_name = {{event_name:String}} \
           AND timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9){source_clause}{proj_clause} \
         GROUP BY ts ORDER BY ts"
    );
    let rows: Vec<Point> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetricName {
    pub metric_name: String,
    pub metric_type: String,
    pub metric_unit: String,
    pub service_name: String,
}

async fn list_names(
    State(state): State<SharedState>,
    Query(range): Query<Range>,
) -> ApiResult<Json<Vec<MetricName>>> {
    let (from, to) = range.resolve();
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);
    let (proj_clause, proj_value) = range.project_clause("");

    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];
    if let Some(p) = proj_value {
        params.push(("project", p));
    }

    let metric_sql = format!(
        "SELECT DISTINCT metric_name, metric_type, metric_unit, service_name \
         FROM faro.metrics WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
            AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9){proj_clause} \
         ORDER BY metric_name LIMIT 1000",
    );
    let mut rows: Vec<MetricName> = state.ch.select_with_params(&metric_sql, &params).await?;

    let event_sql = format!(
        "SELECT concat('{EVENT_METRIC_PREFIX}', event_name, '{EVENT_METRIC_SUFFIX}') AS metric_name, \
                'counter' AS metric_type, \
                'events' AS metric_unit, \
                source AS service_name \
         FROM faro.product_events \
         WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9){proj_clause} \
         GROUP BY event_name, source \
         ORDER BY metric_name \
         LIMIT 1000"
    );
    rows.extend(
        state
            .ch
            .select_with_params::<MetricName>(&event_sql, &params)
            .await?,
    );
    rows.sort_by(|a, b| {
        a.metric_name
            .cmp(&b.metric_name)
            .then_with(|| a.service_name.cmp(&b.service_name))
    });
    Ok(Json(rows))
}

fn event_name_from_metric(metric_name: &str) -> Option<String> {
    let name = metric_name
        .strip_prefix(EVENT_METRIC_PREFIX)?
        .strip_suffix(EVENT_METRIC_SUFFIX)?;
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_name_from_metric_parses_virtual_event_metric() {
        assert_eq!(
            event_name_from_metric("events.checkout_completed.count").as_deref(),
            Some("checkout_completed")
        );
        assert_eq!(
            event_name_from_metric("events.$feature.exposure.count").as_deref(),
            Some("$feature.exposure")
        );
    }

    #[test]
    fn event_name_from_metric_ignores_native_metrics() {
        assert_eq!(event_name_from_metric("http.server.duration"), None);
        assert_eq!(event_name_from_metric("events..count"), None);
    }
}
