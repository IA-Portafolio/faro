//! Endpoints de logs:
//!   GET /logs       → lista filtrable (servicio, severidad, texto/regex, trace_id)
//!   GET /logs/live  → stream SSE de logs nuevos en vivo
//!   GET /logs/stats → volumen por bucket y severidad para el histograma

use axum::extract::State;
use axum::response::sse::Sse;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{de_opt_num, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::storage::LogRow;
use crate::stream::{live_logs_sse, BodyMatcher, LogFilter};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/logs", get(list_logs))
        .route("/logs/live", get(stream_logs))
        .route("/logs/stats", get(log_stats))
}

#[derive(Debug, Deserialize)]
pub struct LogQuery {
    #[serde(flatten)]
    pub range: Range,
    pub service: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num")]
    pub min_severity: Option<u8>,
    pub query: Option<String>,
    pub trace_id: Option<String>,
    /// Cuando es `true`, `query` se interpreta como expresión regular
    /// (case-insensitive) en lugar de subcadena literal. Solo aplica al
    /// endpoint de live tail; las búsquedas históricas siguen usando
    /// `positionCaseInsensitive` en ClickHouse.
    #[serde(default)]
    pub regex: Option<bool>,
}

async fn list_logs(
    State(state): State<SharedState>,
    Query(q): Query<LogQuery>,
) -> ApiResult<Json<Vec<LogRow>>> {
    let (from, to) = q.range.resolve();
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);

    let mut sql = String::from(
        "SELECT timestamp, observed_timestamp, project_id, \
         service_name, severity_text, severity_number, body, trace_id, span_id, scope_name, \
         resource_attributes, attributes \
         FROM faro.logs \
         WHERE timestamp >= toDateTime64({from:DateTime64(9)}, 9) \
           AND timestamp <= toDateTime64({to:DateTime64(9)}, 9)",
    );
    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];

    if let Some(svc) = &q.range.project {
        if !svc.is_empty() {
            sql.push_str(" AND project_id = {project:String}");
            params.push(("project", svc));
        }
    }
    if let Some(svc) = &q.service {
        sql.push_str(" AND service_name = {service:String}");
        params.push(("service", svc));
    }
    let sev_str;
    if let Some(s) = q.min_severity {
        sev_str = s.to_string();
        sql.push_str(" AND severity_number >= {min_severity:UInt8}");
        params.push(("min_severity", &sev_str));
    }
    if let Some(tid) = &q.trace_id {
        sql.push_str(" AND trace_id = {trace_id:String}");
        params.push(("trace_id", tid));
    }
    if let Some(query) = &q.query {
        sql.push_str(" AND positionCaseInsensitive(body, {q:String}) > 0");
        params.push(("q", query));
    }

    // Paginación cursor-based: si el cliente manda `?cursor=<timestamp>` (el ts del
    // último log de la página anterior), filtramos antes del LIMIT. Evita el viejo
    // `OFFSET N`, que en ClickHouse escanea N+limit filas para descartar las primeras
    // N — con cursor el índice por timestamp resuelve en O(log n).
    let (cursor_clause, cursor_value) = q.range.cursor_clause("timestamp");
    sql.push_str(&cursor_clause);
    if let Some(c) = &cursor_value {
        params.push(("cursor", c.as_str()));
    }

    let tail = format!(" ORDER BY timestamp DESC LIMIT {}", q.range.limit());
    sql.push_str(&tail);

    let rows: Vec<LogRow> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

async fn stream_logs(
    State(state): State<SharedState>,
    Query(q): Query<LogQuery>,
) -> ApiResult<
    Sse<
        impl futures::stream::Stream<
            Item = Result<axum::response::sse::Event, std::convert::Infallible>,
        >,
    >,
> {
    let body = match q.query.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(s) if q.regex.unwrap_or(false) => {
            // size_limit acotado para evitar que un patrón pathológico (e.g. miles de
            // alternancias) ate memoria del proceso. 1 MiB cubre cualquier regex razonable.
            let re = regex::RegexBuilder::new(s)
                .case_insensitive(true)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| ApiError::BadRequest(format!("regex inválida: {e}")))?;
            Some(BodyMatcher::Regex(re))
        }
        Some(s) => Some(BodyMatcher::Substring(s.to_lowercase())),
    };
    let filter = LogFilter {
        project: q.range.project.filter(|p| !p.is_empty()),
        service: q.service.filter(|s| !s.is_empty()),
        min_severity: q.min_severity,
        body,
    };
    // Antes de subscribirnos, reservamos un slot. Si excede el cap por-proyecto
    // o el global, devolvemos 429 con Retry-After en vez de aceptar la conexión
    // y dejar que un cliente runaway ate recursos. El slot se libera por RAII
    // cuando el cliente desconecta. Ver `SseSubscriptions`.
    let project_key = filter.project.as_deref().unwrap_or("*").to_string();
    let slot = state
        .sse_subs
        .try_acquire(&project_key)
        .ok_or(ApiError::TooManyRequests {
            retry_after_secs: 5,
        })?;
    let rx = state.live_bus.logs.subscribe();
    Ok(live_logs_sse(rx, Some(filter), slot))
}

#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    #[serde(flatten)]
    pub range: Range,
    pub service: Option<String>,
    #[serde(default, deserialize_with = "de_opt_num")]
    pub bucket_seconds: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Bucket {
    pub ts: String,
    pub service: String,
    pub severity: String,
    pub count: u64,
}

async fn log_stats(
    State(state): State<SharedState>,
    Query(q): Query<StatsQuery>,
) -> ApiResult<Json<Vec<Bucket>>> {
    let (from, to) = q.range.resolve();
    let bucket = q.bucket_seconds.unwrap_or(60).max(1);
    let from_s = from.format("%Y-%m-%d %H:%M:%S").to_string();
    let to_s = to.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut sql = format!(
        "SELECT toString(toStartOfInterval(minute, INTERVAL {bucket} second)) AS ts, \
                service_name AS service, severity_text AS severity, \
                toUInt64(countMerge(count)) AS count \
         FROM faro.logs_stats \
         WHERE minute >= toDateTime({{from:DateTime}}) AND minute <= toDateTime({{to:DateTime}})"
    );
    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];
    if let Some(svc) = &q.service {
        sql.push_str(" AND service_name = {service:String}");
        params.push(("service", svc));
    }
    sql.push_str(" GROUP BY ts, service, severity ORDER BY ts");

    let rows: Vec<Bucket> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}
