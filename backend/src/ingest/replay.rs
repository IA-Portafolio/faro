use axum::extract::{DefaultBodyLimit, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ApiError, ApiResult};
use crate::observability::names;
use crate::state::SharedState;

/// Chunks de rrweb llegan grandes — el snapshot inicial son DOM completos
/// serializados. 16 MiB cubre incluso páginas pesadas con muchas imágenes
/// inlined; suficientemente bajo para que un cliente abusivo no agote RAM.
const REPLAY_BODY_LIMIT: usize = 16 * 1024 * 1024;

pub fn router() -> Router<SharedState> {
    Router::new().route(
        "/replay",
        post(ingest_replay).layer(DefaultBodyLimit::max(REPLAY_BODY_LIMIT)),
    )
}

#[derive(Deserialize)]
struct ReplayPayload {
    session_id: String,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    seq: u32,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    page_url: Option<String>,
    #[serde(default)]
    user_agent: Option<String>,
    /// Array de eventos rrweb. Se almacena tal cual; ClickHouse comprime con ZSTD.
    events: Vec<Value>,
}

#[derive(Serialize)]
struct ReplayRow<'a> {
    #[serde(serialize_with = "rfc3339_millis")]
    timestamp: DateTime<Utc>,
    project_id: &'a str,
    session_id: &'a str,
    service_name: &'a str,
    seq: u32,
    #[serde(serialize_with = "rfc3339_millis")]
    start_ts: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339_millis")]
    end_ts: DateTime<Utc>,
    event_count: u32,
    events: String,
    user_id: &'a str,
    page_url: &'a str,
    user_agent: &'a str,
}

fn rfc3339_millis<S: serde::Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

async fn ingest_replay(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(payload): Json<ReplayPayload>,
) -> ApiResult<Json<serde_json::Value>> {
    let project = super::resolve_project(&state, &headers)?;
    // El replay SDK corre 100% en browser — la validación de Origin es lo más
    // relevante aquí. Server-side no debería postear a /replay nunca.
    super::check_origin(&state, &project, &headers)?;

    // Validaciones baratas antes de tocar el rate limiter.
    if payload.session_id.is_empty() || payload.session_id.len() > 128 {
        return Err(ApiError::BadRequest(
            "session_id requerido (≤128 chars)".into(),
        ));
    }
    if payload.events.is_empty() {
        return Err(ApiError::BadRequest("events vacío".into()));
    }
    if payload.events.len() > 5000 {
        return Err(ApiError::BadRequest(
            "demasiados eventos en un chunk (>5000)".into(),
        ));
    }

    // Comparte bucket con logs; un cliente no puede esquivar el límite cambiando
    // de signal. Contamos cada chunk como 1 record — el costo dominante es el
    // payload, no el número de filas.
    match state.limiter.check(&project, 1) {
        super::rate_limit::LimitOutcome::Allowed => {}
        other => {
            let secs = other.retry_after_secs();
            tracing::warn!(
                project,
                retry_after_secs = secs,
                "ingest /replay rate-limited"
            );
            metrics::counter!(
                names::RATE_LIMITED,
                "project" => project.clone(),
                "signal" => "replay",
            )
            .increment(1);
            return Err(ApiError::TooManyRequests {
                retry_after_secs: secs,
            });
        }
    }

    // Calcula los timestamps del chunk a partir de los eventos rrweb. Cada evento
    // tiene un `.timestamp` en ms desde epoch. Si falla la inferencia, cae a now.
    let (start_ts, end_ts) = chunk_bounds(&payload.events).unwrap_or_else(|| {
        let now = Utc::now();
        (now, now)
    });

    let events_json = serde_json::to_string(&payload.events)
        .map_err(|e| ApiError::BadRequest(format!("events no serializable: {e}")))?;

    let svc = payload.service.unwrap_or_else(|| "unknown".into());
    let user_id = payload.user_id.unwrap_or_default();
    let page_url = payload.page_url.unwrap_or_default();
    let user_agent = payload.user_agent.unwrap_or_default();

    let row = ReplayRow {
        timestamp: Utc::now(),
        project_id: &project,
        session_id: &payload.session_id,
        service_name: &svc,
        seq: payload.seq,
        start_ts,
        end_ts,
        event_count: payload.events.len() as u32,
        events: events_json,
        user_id: &user_id,
        page_url: &page_url,
        user_agent: &user_agent,
    };

    state.ch.insert("faro.session_replays", &[row]).await?;

    metrics::counter!(
        names::INGEST_RECORDS,
        "project" => project.clone(),
        "signal" => "replay",
        "outcome" => "accepted",
    )
    .increment(payload.events.len() as u64);

    Ok(Json(serde_json::json!({
        "accepted": payload.events.len(),
        "session_id": payload.session_id,
        "seq": payload.seq,
    })))
}

/// Extrae (min, max) de los timestamps de los eventos rrweb. rrweb usa ms desde
/// epoch en `event.timestamp`, así que parseamos como i64 y convertimos.
fn chunk_bounds(events: &[Value]) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let mut min_ms: Option<i64> = None;
    let mut max_ms: Option<i64> = None;
    for e in events {
        if let Some(ts) = e.get("timestamp").and_then(Value::as_i64) {
            min_ms = Some(min_ms.map_or(ts, |m| m.min(ts)));
            max_ms = Some(max_ms.map_or(ts, |m| m.max(ts)));
        }
    }
    let (a, b) = (min_ms?, max_ms?);
    Some((from_ms(a), from_ms(b)))
}

fn from_ms(ms: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_else(Utc::now)
}
