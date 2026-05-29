use std::time::Instant;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{ch_dt, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new().route("/retention", get(retention))
}

#[derive(Debug, Deserialize)]
pub struct RetentionQuery {
    #[serde(flatten)]
    pub range: Range,
    pub event_name: Option<String>,
    pub interval: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RetentionCohort {
    pub cohort_date: String,
    pub cohort_size: u64,
    pub d1_users: u64,
    pub d7_users: u64,
    pub d30_users: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RetentionResult {
    pub from: String,
    pub to: String,
    pub event_name: String,
    pub interval: String,
    pub columns: Vec<u8>,
    pub cohorts: Vec<RetentionCohort>,
    pub took_ms: u64,
}

fn parse_interval(raw: Option<&str>) -> ApiResult<&'static str> {
    match raw {
        None | Some("day") => Ok("day"),
        Some(_) => Err(ApiError::BadRequest(
            "interval soportado por ahora: day".into(),
        )),
    }
}

fn retention_rate(users: u64, cohort_size: u64) -> f32 {
    if cohort_size == 0 {
        0.0
    } else {
        users as f32 / cohort_size as f32
    }
}

async fn retention(
    State(state): State<SharedState>,
    Query(q): Query<RetentionQuery>,
) -> ApiResult<Json<RetentionResult>> {
    let started = Instant::now();
    let interval = parse_interval(q.interval.as_deref())?;
    let (from, to) = q.range.resolve();
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }

    let from_s = ch_dt(from);
    let to_s = ch_dt(to);
    // Techo del rango de eventos relevantes: el cohort más reciente posible es `to`,
    // y necesitamos hasta D+30 sobre él, dejamos un día extra para que la desigualdad
    // estricta `<` cubra cualquier borde.
    let to_plus_31d_s = ch_dt(to + chrono::Duration::days(31));
    let event_name = q.event_name.unwrap_or_default();
    let (project_clause, project_value) = q.range.project_clause("");
    // En el subquery de eventos de retorno, la columna se referencia sin prefijo de alias.
    let return_event_clause = if event_name.trim().is_empty() {
        ""
    } else {
        " AND event_name = {event_name:String}"
    };

    // Cohort clásico: usuarios cuya primera actividad histórica cae dentro del rango.
    // La retención se mide por actividad en el día calendario D+n.
    //
    // El subquery de pe acota el rango de timestamps a [from, to + 31d) y aplica
    // event_name/project filters; el JOIN queda como equijoin puro por
    // (project_id, distinct_id) — ClickHouse 24.8 con new analyzer rechaza
    // condiciones ON que mezclen columnas de left y right en desigualdades
    // (INVALID_JOIN_ON_EXPRESSION, 403). Los `uniqExactIf` ya filtran al día
    // exacto, así que el resultado es semánticamente idéntico.
    let sql = format!(
        "WITH \
           first_touch AS ( \
             SELECT project_id, distinct_id, toDate(first_ts) AS cohort_date \
             FROM ( \
               SELECT project_id, distinct_id, min(timestamp) AS first_ts \
               FROM faro.product_events \
               WHERE timestamp < toDateTime64({{to:DateTime64(9)}}, 9){project_clause} \
               GROUP BY project_id, distinct_id \
             ) \
             WHERE first_ts >= toDateTime64({{from:DateTime64(9)}}, 9) \
               AND first_ts <  toDateTime64({{to:DateTime64(9)}}, 9) \
           ) \
         SELECT toString(ft.cohort_date) AS cohort_date, \
                toUInt64(uniqExact(ft.distinct_id)) AS cohort_size, \
                toUInt64(uniqExactIf(ft.distinct_id, toDate(pe.timestamp) = ft.cohort_date + 1)) AS d1_users, \
                toUInt64(uniqExactIf(ft.distinct_id, toDate(pe.timestamp) = ft.cohort_date + 7)) AS d7_users, \
                toUInt64(uniqExactIf(ft.distinct_id, toDate(pe.timestamp) = ft.cohort_date + 30)) AS d30_users \
         FROM first_touch AS ft \
         LEFT JOIN ( \
           SELECT project_id, distinct_id, timestamp \
           FROM faro.product_events \
           WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND timestamp <  toDateTime64({{to_plus_31d:DateTime64(9)}}, 9){project_clause}{return_event_clause} \
         ) AS pe \
           ON pe.project_id = ft.project_id \
          AND pe.distinct_id = ft.distinct_id \
         GROUP BY ft.cohort_date \
         ORDER BY ft.cohort_date DESC"
    );

    let mut params: Vec<(&str, &str)> = vec![
        ("from", &from_s),
        ("to", &to_s),
        ("to_plus_31d", &to_plus_31d_s),
    ];
    if let Some(project) = project_value {
        params.push(("project", project));
    }
    if !event_name.trim().is_empty() {
        params.push(("event_name", event_name.as_str()));
    }

    let cohorts: Vec<RetentionCohort> = state.ch.select_with_params(&sql, &params).await?;

    Ok(Json(RetentionResult {
        from: from.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        to: to.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        event_name,
        interval: interval.to_string(),
        columns: vec![1, 7, 30],
        cohorts,
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interval_accepts_only_day() {
        assert_eq!(parse_interval(None).unwrap(), "day");
        assert_eq!(parse_interval(Some("day")).unwrap(), "day");
        assert!(parse_interval(Some("week")).is_err());
        assert!(parse_interval(Some("")).is_err());
    }

    #[test]
    fn retention_rate_handles_empty_cohort() {
        assert_eq!(retention_rate(25, 100), 0.25);
        assert_eq!(retention_rate(25, 0), 0.0);
    }
}
