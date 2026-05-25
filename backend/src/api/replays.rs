use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::params::Range;
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    // `{fingerprint}` debe coincidir con el nombre que usa `api::errors::router`
    // en `/errors/{fingerprint}/status` — matchit panica al arrancar si dos rutas
    // que comparten estructura usan nombres distintos para el mismo segmento.
    Router::new()
        .route("/replays", get(list_replays))
        .route("/replays/{session_id}", get(get_replay))
        .route("/errors/{fingerprint}/sessions", get(sessions_for_issue))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReplaySummary {
    pub session_id: String,
    pub service_name: String,
    pub start_ts: String,
    pub end_ts: String,
    pub event_count: u64,
    pub chunk_count: u32,
    pub user_id: String,
    pub page_url: String,
}

#[derive(Debug, Deserialize)]
pub struct ReplayListQuery {
    #[serde(flatten)]
    pub range: Range,
    pub service: Option<String>,
    pub session_id: Option<String>,
}

/// Lista sesiones disponibles dentro del rango. Útil para una vista futura de
/// "todas las sesiones del proyecto"; por ahora la entrada principal es el link
/// desde un error.
async fn list_replays(
    State(state): State<SharedState>,
    Query(q): Query<ReplayListQuery>,
) -> ApiResult<Json<Vec<ReplaySummary>>> {
    let (from, to) = q.range.resolve();
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);

    let mut where_clause = String::from(
        "timestamp >= toDateTime64({from:DateTime64(3)}, 3) \
         AND timestamp <= toDateTime64({to:DateTime64(3)}, 3)",
    );
    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];
    if let Some(p) = &q.range.project {
        if !p.is_empty() {
            where_clause.push_str(" AND project_id = {project:String}");
            params.push(("project", p));
        }
    }
    if let Some(svc) = &q.service {
        where_clause.push_str(" AND service_name = {service:String}");
        params.push(("service", svc));
    }
    if let Some(sid) = &q.session_id {
        where_clause.push_str(" AND session_id = {sid:String}");
        params.push(("sid", sid));
    }

    let sql = format!(
        "SELECT session_id, \
                any(service_name) AS service_name, \
                toString(min(start_ts)) AS start_ts, \
                toString(max(end_ts)) AS end_ts, \
                toUInt64(sum(event_count)) AS event_count, \
                toUInt32(count()) AS chunk_count, \
                any(user_id) AS user_id, \
                argMax(page_url, end_ts) AS page_url \
         FROM faro.session_replays \
         WHERE {where_clause} \
         GROUP BY session_id \
         ORDER BY end_ts DESC \
         LIMIT {limit}",
        where_clause = where_clause,
        limit = q.range.limit()
    );
    let rows: Vec<ReplaySummary> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReplayPayload {
    pub session_id: String,
    pub service_name: String,
    pub start_ts: String,
    pub end_ts: String,
    pub event_count: u64,
    pub page_url: String,
    pub user_id: String,
    pub user_agent: String,
    /// Eventos rrweb concatenados de todos los chunks de la sesión, en orden.
    pub events: Vec<Value>,
}

#[derive(Deserialize)]
struct ChunkRow {
    service_name: String,
    start_ts: String,
    end_ts: String,
    event_count: u64,
    events: String,
    user_id: String,
    page_url: String,
    user_agent: String,
}

/// Devuelve la sesión entera reconstruida: eventos rrweb concatenados en orden
/// de `seq`, además de metadata (service, ventana temporal, page_url, user).
async fn get_replay(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> ApiResult<Json<ReplayPayload>> {
    if session_id.is_empty() || session_id.len() > 128 {
        return Err(ApiError::BadRequest("session_id inválido".into()));
    }
    let sql = "SELECT service_name, \
                toString(start_ts) AS start_ts, \
                toString(end_ts) AS end_ts, \
                event_count, events, user_id, page_url, user_agent \
         FROM faro.session_replays \
         WHERE session_id = {sid:String} \
         ORDER BY seq ASC LIMIT 5000";
    let chunks: Vec<ChunkRow> = state
        .ch
        .select_with_params(sql, &[("sid", &session_id)])
        .await?;
    if chunks.is_empty() {
        return Err(ApiError::NotFound);
    }

    // Concatena los arrays JSON de cada chunk en uno solo. Se parsea por chunk
    // para evitar pegar strings (que no daría un JSON válido en el wire).
    let mut events: Vec<Value> = Vec::new();
    let mut event_total: u64 = 0;
    for c in &chunks {
        match serde_json::from_str::<Vec<Value>>(&c.events) {
            Ok(arr) => events.extend(arr),
            Err(e) => {
                tracing::warn!(session = %session_id, error = %e, "chunk con events JSON inválido — saltado");
            }
        }
        event_total += c.event_count;
    }

    let first = &chunks[0];
    let last = chunks.last().unwrap();
    // El page_url del último chunk suele ser el más representativo si hubo navegación;
    // user_agent y user_id se toman del primero (no cambian dentro de una sesión).
    let payload = ReplayPayload {
        session_id,
        service_name: first.service_name.clone(),
        start_ts: first.start_ts.clone(),
        end_ts: last.end_ts.clone(),
        event_count: event_total,
        page_url: last.page_url.clone(),
        user_id: first.user_id.clone(),
        user_agent: first.user_agent.clone(),
        events,
    };
    Ok(Json(payload))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IssueSession {
    pub session_id: String,
    pub timestamp: String,
    pub service_name: String,
    pub has_replay: u8,
}

/// Para un fingerprint dado, devuelve las sesiones presentes en los eventos
/// de error (extrayendo `session.id` desde el Map de attributes), y marca cuáles
/// tienen replay disponible. La join se hace en ClickHouse con un IN sobre la
/// tabla session_replays para evitar mandar la lista al servidor.
async fn sessions_for_issue(
    State(state): State<SharedState>,
    Path(fp): Path<String>,
) -> ApiResult<Json<Vec<IssueSession>>> {
    if fp.is_empty() || fp.len() > 64 {
        return Err(ApiError::BadRequest("fingerprint inválido".into()));
    }
    // Lee los últimos 200 eventos del error y queda con los que traen session.id.
    // Después un GLOBAL IN contra session_replays nos dice cuáles tienen replay
    // realmente persistido (puede que la sesión muriese antes del primer flush
    // o que ya cayese del TTL de 7d).
    let sql = "SELECT attributes['session.id'] AS session_id, \
                toString(max(timestamp)) AS timestamp, \
                any(service_name) AS service_name, \
                toUInt8(session_id IN ( \
                    SELECT DISTINCT session_id FROM faro.session_replays \
                )) AS has_replay \
         FROM faro.error_events \
         WHERE fingerprint = {fp:String} \
           AND attributes['session.id'] != '' \
         GROUP BY session_id \
         ORDER BY timestamp DESC \
         LIMIT 50";
    let rows: Vec<IssueSession> = state.ch.select_with_params(sql, &[("fp", &fp)]).await?;
    Ok(Json(rows))
}
