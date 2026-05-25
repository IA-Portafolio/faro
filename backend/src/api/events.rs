use axum::extract::State;
use axum::response::sse::Sse;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::Range;
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::storage::ProductEventRow;
use crate::stream::{live_events_sse, EventFilter};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/events", get(list_events))
        .route("/events/live", get(stream_events))
        .route("/events/stats", get(event_stats))
}

/// Tope al número de filtros `properties.X = Y` aceptados en un solo request.
/// Cada uno añade un `AND JSONExtractString(...)` al WHERE; varios docenas saturan
/// el parser de ClickHouse y permiten que un cliente patológico convierta la query
/// en un DoS barato. 5 cubre cualquier caso real de exploración manual.
const MAX_PROP_FILTERS: usize = 5;
const MAX_PROP_KEY_LEN: usize = 128;
const MAX_PROP_VAL_LEN: usize = 256;

#[derive(Debug, Deserialize)]
pub struct EventQuery {
    #[serde(flatten)]
    pub range: Range,
    pub event_name: Option<String>,
    pub distinct_id: Option<String>,
    pub anonymous_id: Option<String>,
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub source: Option<String>,
    /// Búsqueda de substring case-insensitive sobre el JSON crudo de `properties`.
    pub query: Option<String>,
    /// Pares `key:value` para filtrar `JSONExtractString(properties, key) = value`.
    /// Acepta repeticiones del mismo parámetro: `?prop=a:b&prop=c:d`.
    #[serde(default)]
    pub prop: Vec<String>,
}

/// Convierte `["k1:v1", "k2:v2"]` a pares (k, v). Ignora silenciosamente entradas
/// sin `:`, vacías o que excedan los límites — preferible a 400 ruidoso en un
/// endpoint exploratorio donde el cliente puede tipear cualquier cosa.
fn parse_props(raw: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in raw.iter().take(MAX_PROP_FILTERS) {
        let Some((k, v)) = entry.split_once(':') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        if k.is_empty() || v.is_empty() {
            continue;
        }
        if k.len() > MAX_PROP_KEY_LEN || v.len() > MAX_PROP_VAL_LEN {
            continue;
        }
        out.push((k.to_string(), v.to_string()));
    }
    out
}

async fn list_events(
    State(state): State<SharedState>,
    Query(q): Query<EventQuery>,
) -> ApiResult<Json<Vec<ProductEventRow>>> {
    let (from, to) = q.range.resolve();
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);

    let mut sql = String::from(
        "SELECT timestamp, project_id, event_name, distinct_id, anonymous_id, \
                session_id, properties, user_properties, context, source, \
                trace_id, span_id, toString(event_id) AS event_id \
         FROM faro.product_events \
         WHERE timestamp >= toDateTime64({from:DateTime64(9)}, 9) \
           AND timestamp <= toDateTime64({to:DateTime64(9)}, 9)",
    );
    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];

    if let Some(p) = &q.range.project {
        if !p.is_empty() {
            sql.push_str(" AND project_id = {project:String}");
            params.push(("project", p));
        }
    }
    if let Some(name) = &q.event_name {
        if !name.is_empty() {
            sql.push_str(" AND event_name = {event_name:String}");
            params.push(("event_name", name));
        }
    }
    if let Some(d) = &q.distinct_id {
        if !d.is_empty() {
            sql.push_str(" AND distinct_id = {distinct_id:String}");
            params.push(("distinct_id", d));
        }
    }
    if let Some(a) = &q.anonymous_id {
        if !a.is_empty() {
            sql.push_str(" AND anonymous_id = {anonymous_id:String}");
            params.push(("anonymous_id", a));
        }
    }
    if let Some(s) = &q.session_id {
        if !s.is_empty() {
            sql.push_str(" AND session_id = {session_id:String}");
            params.push(("session_id", s));
        }
    }
    if let Some(t) = &q.trace_id {
        if !t.is_empty() {
            sql.push_str(" AND trace_id = {trace_id:String}");
            params.push(("trace_id", t));
        }
    }
    if let Some(s) = &q.source {
        if !s.is_empty() {
            sql.push_str(" AND source = {source:String}");
            params.push(("source", s));
        }
    }
    if let Some(qs) = &q.query {
        if !qs.is_empty() {
            sql.push_str(" AND positionCaseInsensitive(properties, {q:String}) > 0");
            params.push(("q", qs));
        }
    }

    // Filtros estructurados sobre properties. Las llaves de parámetro deben ser
    // distintas, así que numeramos: prop_k_0, prop_v_0, prop_k_1, prop_v_1…
    // Los `String` viven en `prop_pairs` hasta el final del scope, así que
    // tomar `&str` para `params` es seguro.
    let prop_pairs = parse_props(&q.prop);
    let prop_param_names: Vec<(String, String)> = (0..prop_pairs.len())
        .map(|i| (format!("prop_k_{i}"), format!("prop_v_{i}")))
        .collect();
    for (i, (k, v)) in prop_pairs.iter().enumerate() {
        let (kp, vp) = &prop_param_names[i];
        sql.push_str(&format!(
            " AND JSONExtractString(properties, {{{kp}:String}}) = {{{vp}:String}}"
        ));
        params.push((kp.as_str(), k.as_str()));
        params.push((vp.as_str(), v.as_str()));
    }

    let (cursor_clause, cursor_value) = q.range.cursor_clause("timestamp");
    sql.push_str(&cursor_clause);
    if let Some(c) = &cursor_value {
        params.push(("cursor", c.as_str()));
    }

    let tail = format!(" ORDER BY timestamp DESC LIMIT {}", q.range.limit());
    sql.push_str(&tail);

    let rows: Vec<ProductEventRow> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

async fn stream_events(
    State(state): State<SharedState>,
    Query(q): Query<EventQuery>,
) -> ApiResult<
    Sse<
        impl futures::stream::Stream<
            Item = Result<axum::response::sse::Event, std::convert::Infallible>,
        >,
    >,
> {
    let filter = EventFilter {
        project: q.range.project.filter(|p| !p.is_empty()),
        event_name: q.event_name.filter(|s| !s.is_empty()),
        distinct_id: q.distinct_id.filter(|s| !s.is_empty()),
        trace_id: q.trace_id.filter(|s| !s.is_empty()),
        source: q.source.filter(|s| !s.is_empty()),
    };
    // Reserva slot bajo los caps por-proyecto y global, igual que `/logs/live`.
    // El SseSlot se libera por RAII al desconectarse el cliente.
    let project_key = filter.project.as_deref().unwrap_or("*").to_string();
    let slot = state
        .sse_subs
        .try_acquire(&project_key)
        .ok_or(ApiError::TooManyRequests {
            retry_after_secs: 5,
        })?;
    let rx = state.live_bus.events.subscribe();
    Ok(live_events_sse(rx, Some(filter), slot))
}

#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    #[serde(flatten)]
    pub range: Range,
    pub event_name: Option<String>,
    #[serde(default, deserialize_with = "crate::api::params::de_opt_num")]
    pub bucket_seconds: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventBucket {
    pub ts: String,
    pub event_name: String,
    pub count: u64,
}

async fn event_stats(
    State(state): State<SharedState>,
    Query(q): Query<StatsQuery>,
) -> ApiResult<Json<Vec<EventBucket>>> {
    let (from, to) = q.range.resolve();
    let bucket = q.bucket_seconds.unwrap_or(60).max(1);
    let from_s = from.format("%Y-%m-%d %H:%M:%S").to_string();
    let to_s = to.format("%Y-%m-%d %H:%M:%S").to_string();

    // No tenemos una MV por segundo/minuto para events (solo `product_events_per_day`,
    // que solo sirve para cards diarias). El histograma del rango corto va directo
    // a `product_events`. Acotamos columnas y aplicamos filtros típicos para que
    // ClickHouse use el índice por timestamp + el bloom de event_name.
    let mut sql = format!(
        "SELECT toString(toStartOfInterval(timestamp, INTERVAL {bucket} second)) AS ts, \
                event_name, toUInt64(count()) AS count \
         FROM faro.product_events \
         WHERE timestamp >= toDateTime64({{from:DateTime64(0)}}, 0) \
           AND timestamp <= toDateTime64({{to:DateTime64(0)}}, 0)"
    );
    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];
    if let Some(p) = &q.range.project {
        if !p.is_empty() {
            sql.push_str(" AND project_id = {project:String}");
            params.push(("project", p));
        }
    }
    if let Some(name) = &q.event_name {
        if !name.is_empty() {
            sql.push_str(" AND event_name = {event_name:String}");
            params.push(("event_name", name));
        }
    }
    sql.push_str(" GROUP BY ts, event_name ORDER BY ts");

    let rows: Vec<EventBucket> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}
