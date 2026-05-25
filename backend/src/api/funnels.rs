//! Funnels exploratorios sobre `faro.product_events` (6º pilar).
//!
//! `GET  /funnels/events`   → catálogo de event_name distintos (autocomplete del builder).
//! `POST /funnels/compute`  → cómputo ad-hoc de un funnel usando `windowFunnel` de ClickHouse.
//!
//! Diseño para hit <500ms en 7 días (objetivo del goal 10.D.1):
//!  * El SELECT interno sólo materializa `timestamp`, `distinct_id` y `event_name`, evitando
//!    descomprimir las columnas `properties`/`user_properties`/`context` (ZSTD(3), pesadas).
//!  * `event_name IN (...)` + bloom filter `idx_event_name` poda granules antes de leer.
//!  * Partition pruning por día sobre `timestamp` cubre el rango temporal.
//!  * No tocamos `product_events_per_day` para esto: la MV agrega por evento/día y pierde
//!    la dimensión `distinct_id` que windowFunnel necesita por usuario.

use std::time::Instant;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::api::params::{ch_dt, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/funnels/events", get(list_events))
        .route("/funnels/compute", post(compute))
        .route("/funnels/drop-off", post(drop_off))
        .route("/funnels/time-to-convert", post(time_to_convert))
}

// ---------------------------------------------------------------------------
// GET /funnels/events
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventCandidate {
    pub name: String,
    /// Veces que se vio en el rango (suma a través de días).
    pub count: u64,
}

/// Lee el catálogo de eventos desde la MV `product_events_per_day` para no escanear
/// la tabla cruda. La MV ya tiene `(day, project_id, event_name)` ordenado, así que un
/// `GROUP BY event_name` con `countMerge` se resuelve en milisegundos para meses de datos.
async fn list_events(
    State(state): State<SharedState>,
    axum_extra::extract::Query(range): axum_extra::extract::Query<Range>,
) -> ApiResult<Json<Vec<EventCandidate>>> {
    let (from, to) = range.resolve();
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);
    let (proj_clause, proj_value) = range.project_clause("");

    let sql = format!(
        "SELECT event_name AS name, toUInt64(sum(countMerge(count))) AS count \
         FROM faro.product_events_per_day \
         WHERE day >= toDate(toDateTime64({{from:DateTime64(9)}}, 9)) \
           AND day <= toDate(toDateTime64({{to:DateTime64(9)}}, 9)){proj_clause} \
         GROUP BY event_name \
         ORDER BY count DESC \
         LIMIT 200"
    );

    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];
    if let Some(p) = proj_value {
        params.push(("project", p));
    }

    let rows: Vec<EventCandidate> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

// ---------------------------------------------------------------------------
// POST /funnels/compute
// ---------------------------------------------------------------------------

const MAX_STEPS: usize = 8;
const MAX_WINDOW_SECS: u32 = 30 * 86_400;
const DEFAULT_WINDOW_SECS: u32 = 86_400;

#[derive(Debug, Deserialize, ToSchema)]
pub struct FunnelRequest {
    /// Lista ordenada de event_name del funnel. Mínimo 2, máximo 8 pasos.
    pub steps: Vec<String>,
    /// Ventana de conversión en segundos. Default 86_400 (1 día); máximo 30 días.
    #[serde(default)]
    pub window_seconds: Option<u32>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    /// Alternativa a `from`. Se ignora si `from` viene seteado.
    pub last_minutes: Option<i64>,
    /// Slug del proyecto. Si está vacío/ausente, considera todos los proyectos del tenant.
    pub project: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FunnelStep {
    pub event: String,
    pub users: u64,
    /// `users(step_i) / users(step_0)` ∈ [0, 1]. El primer paso es siempre 1.0.
    pub conversion_from_start: f32,
    /// `users(step_i) / users(step_{i-1})` ∈ [0, 1]. El primer paso es siempre 1.0.
    pub conversion_from_prev: f32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FunnelResult {
    pub steps: Vec<FunnelStep>,
    /// Total de usuarios distintos que llegaron al menos al primer paso.
    pub total_entered: u64,
    /// Ventana efectivamente usada (segundos), tras clamping del input.
    pub window_seconds: u32,
    pub from: String,
    pub to: String,
    /// Tiempo de cómputo del lado backend (incluye query a ClickHouse).
    pub took_ms: u64,
}

async fn compute(
    State(state): State<SharedState>,
    Json(req): Json<FunnelRequest>,
) -> ApiResult<Json<FunnelResult>> {
    let started = Instant::now();

    // -- Validación
    if req.steps.len() < 2 {
        return Err(ApiError::BadRequest(
            "un funnel necesita al menos 2 pasos".into(),
        ));
    }
    if req.steps.len() > MAX_STEPS {
        return Err(ApiError::BadRequest(format!(
            "máximo {MAX_STEPS} pasos por funnel"
        )));
    }
    if req.steps.iter().any(|e| e.trim().is_empty()) {
        return Err(ApiError::BadRequest(
            "los nombres de evento no pueden ser vacíos".into(),
        ));
    }

    let window = req
        .window_seconds
        .unwrap_or(DEFAULT_WINDOW_SECS)
        .clamp(60, MAX_WINDOW_SECS);

    // -- Rango temporal: default 7 días (alineado con el goal D.1).
    let to = req.to.unwrap_or_else(Utc::now);
    let from = req.from.unwrap_or_else(|| match req.last_minutes {
        Some(m) if m > 0 => to - Duration::minutes(m),
        _ => to - Duration::days(7),
    });
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }

    let from_s = ch_dt(from);
    let to_s = ch_dt(to);
    let window_s = window.to_string();

    // -- Construir SQL parametrizado.
    // event_<i> se bindea desde el lado servidor (ClickHouse param), así que los
    // event names del usuario nunca tocan el query crudo.
    let mut conds = String::new();
    let mut in_list = String::new();
    for i in 0..req.steps.len() {
        if i > 0 {
            conds.push_str(", ");
            in_list.push_str(", ");
        }
        conds.push_str(&format!("event_name = {{event_{i}:String}}"));
        in_list.push_str(&format!("{{event_{i}:String}}"));
    }

    let proj_clause = match &req.project {
        Some(p) if !p.is_empty() => " AND project_id = {project:String}",
        _ => "",
    };

    let sql = format!(
        "SELECT toUInt32(level) AS level, toUInt64(count()) AS users \
         FROM ( \
           SELECT windowFunnel({{window:UInt32}})(timestamp, {conds}) AS level \
           FROM faro.product_events \
           WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
             AND event_name IN ({in_list}){proj_clause} \
           GROUP BY distinct_id \
         ) \
         WHERE level > 0 \
         GROUP BY level \
         ORDER BY level",
    );

    // -- Parámetros. Los Strings deben vivir mientras dure el await — mantenemos
    //    `event_keys` y `steps` (ref a req) en stack.
    let event_keys: Vec<String> = (0..req.steps.len())
        .map(|i| format!("event_{i}"))
        .collect();
    let mut params: Vec<(&str, &str)> = Vec::with_capacity(req.steps.len() + 4);
    params.push(("window", &window_s));
    params.push(("from", &from_s));
    params.push(("to", &to_s));
    for (i, ev) in req.steps.iter().enumerate() {
        params.push((event_keys[i].as_str(), ev.as_str()));
    }
    if let Some(p) = &req.project {
        if !p.is_empty() {
            params.push(("project", p.as_str()));
        }
    }

    #[derive(Debug, Deserialize)]
    struct LevelRow {
        level: u32,
        users: u64,
    }
    let rows: Vec<LevelRow> = state.ch.select_with_params(&sql, &params).await?;

    // -- Convertir (level, users) → counts por paso (cumulative-from-top).
    //    level=k significa "alcanzó k pasos", así que step_i = Σ users donde level ≥ i+1.
    let n = req.steps.len();
    let mut step_users = vec![0u64; n];
    for r in &rows {
        let reached = (r.level as usize).min(n);
        for i in 0..reached {
            step_users[i] = step_users[i].saturating_add(r.users);
        }
    }
    let total_entered = step_users.first().copied().unwrap_or(0);

    let steps: Vec<FunnelStep> = req
        .steps
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let users = step_users[i];
            let from_start = if total_entered == 0 {
                0.0
            } else {
                users as f32 / total_entered as f32
            };
            let from_prev = if i == 0 {
                1.0
            } else if step_users[i - 1] == 0 {
                0.0
            } else {
                users as f32 / step_users[i - 1] as f32
            };
            FunnelStep {
                event: name.clone(),
                users,
                conversion_from_start: from_start,
                conversion_from_prev: from_prev,
            }
        })
        .collect();

    Ok(Json(FunnelResult {
        steps,
        total_entered,
        window_seconds: window,
        from: from.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        to: to.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

// ---------------------------------------------------------------------------
// POST /funnels/drop-off
// ---------------------------------------------------------------------------
//
// "Para los usuarios que llegaron al paso N pero no al N+1, ¿qué eventos
// dispararon en los siguientes `lookahead_seconds`?". El insight accionable
// del goal D.2: ver el ruido post-fricción ("vieron pricing y nunca clickearon
// signup, pero el 40% volvió a help docs").

const DEFAULT_LOOKAHEAD_SECS: u32 = 300; // 5 min, el default del goal D.2.
const MAX_LOOKAHEAD_SECS: u32 = 60 * 60; // tope 1h — más que eso se diluye.
const DEFAULT_DROPOFF_LIMIT: u32 = 20;
const MAX_DROPOFF_LIMIT: u32 = 100;

#[derive(Debug, Deserialize, ToSchema)]
pub struct DropOffRequest {
    /// Definición del funnel (mismos pasos que `POST /funnels/compute`).
    pub steps: Vec<String>,
    /// Paso a analizar (0-indexado). Debe ser < `steps.len() - 1`:
    /// el último paso no tiene "siguiente" del que se pueda caer.
    pub step_index: usize,
    /// Ventana de conversión del funnel en segundos. Define qué cuenta como
    /// "no llegó al siguiente paso". Default 86_400 (igual que `/preview`).
    #[serde(default)]
    pub window_seconds: Option<u32>,
    /// Segundos a mirar después del evento del paso N. Default 300 (5 min).
    #[serde(default)]
    pub lookahead_seconds: Option<u32>,
    /// Cuántos eventos top devolver. Default 20, máx 100.
    #[serde(default)]
    pub limit: Option<u32>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub last_minutes: Option<i64>,
    pub project: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DropOffEvent {
    pub event_name: String,
    /// Usuarios distintos del cohort que dispararon este evento dentro del lookahead.
    pub users: u64,
    /// Total de ocurrencias (un mismo usuario puede dispararlo varias veces).
    pub occurrences: u64,
    /// `users / dropped_users` ∈ [0, 1]. Listo para pintar como "el N% volvió a X".
    pub share: f32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DropOffResult {
    pub step_index: usize,
    pub step_event: String,
    pub next_event: String,
    /// Total de usuarios distintos que llegaron al paso N pero NO al paso N+1
    /// dentro de la ventana de conversión.
    pub dropped_users: u64,
    pub lookahead_seconds: u32,
    pub window_seconds: u32,
    pub from: String,
    pub to: String,
    pub top_events: Vec<DropOffEvent>,
    pub took_ms: u64,
}

async fn drop_off(
    State(state): State<SharedState>,
    Json(req): Json<DropOffRequest>,
) -> ApiResult<Json<DropOffResult>> {
    let started = Instant::now();

    // -- Validación
    if req.steps.len() < 2 {
        return Err(ApiError::BadRequest(
            "un funnel necesita al menos 2 eventos".into(),
        ));
    }
    if req.steps.len() > MAX_STEPS {
        return Err(ApiError::BadRequest(format!(
            "máximo {MAX_STEPS} pasos por funnel"
        )));
    }
    if req.steps.iter().any(|e| e.trim().is_empty()) {
        return Err(ApiError::BadRequest(
            "los nombres de evento no pueden ser vacíos".into(),
        ));
    }
    if req.step_index >= req.steps.len() - 1 {
        return Err(ApiError::BadRequest(
            "el último paso no tiene drop-off (no hay paso siguiente)".into(),
        ));
    }

    let window = req
        .window_seconds
        .unwrap_or(DEFAULT_WINDOW_SECS)
        .clamp(60, MAX_WINDOW_SECS);
    let lookahead = req
        .lookahead_seconds
        .unwrap_or(DEFAULT_LOOKAHEAD_SECS)
        .clamp(30, MAX_LOOKAHEAD_SECS);
    let limit = req
        .limit
        .unwrap_or(DEFAULT_DROPOFF_LIMIT)
        .clamp(1, MAX_DROPOFF_LIMIT);

    let to = req.to.unwrap_or_else(Utc::now);
    let from = req.from.unwrap_or_else(|| match req.last_minutes {
        Some(m) if m > 0 => to - Duration::minutes(m),
        _ => to - Duration::days(7),
    });
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }

    // -- Construir parámetros y fragmentos compartidos.
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);
    let window_s = window.to_string();
    let lookahead_s = lookahead.to_string();
    let limit_s = limit.to_string();
    // En windowFunnel los niveles son 1-indexados: nivel k = "completó los
    // primeros k eventos". `step_index` es 0-indexado: "reached step N pero no
    // step N+1" significa nivel exacto = N+1.
    let target_level_s = (req.step_index + 1).to_string();

    let mut conds = String::new();
    let mut in_list = String::new();
    for i in 0..req.steps.len() {
        if i > 0 {
            conds.push_str(", ");
            in_list.push_str(", ");
        }
        conds.push_str(&format!("event_name = {{event_{i}:String}}"));
        in_list.push_str(&format!("{{event_{i}:String}}"));
    }

    let proj_clause_pe = match &req.project {
        Some(p) if !p.is_empty() => " AND pe.project_id = {project:String}",
        _ => "",
    };
    let proj_clause_plain = match &req.project {
        Some(p) if !p.is_empty() => " AND project_id = {project:String}",
        _ => "",
    };

    // -- Query 1: tamaño del cohort drop-off.
    //
    // Lo separamos del JOIN para devolver `dropped_users` aunque no haya eventos
    // en el lookahead (e.g. usuarios que cierran la pestaña y se van).
    let cohort_sql = format!(
        "SELECT toUInt64(count()) AS users \
         FROM ( \
           SELECT distinct_id, windowFunnel({{window:UInt32}})(timestamp, {conds}) AS level \
           FROM faro.product_events \
           WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
             AND event_name IN ({in_list}){proj_clause_plain} \
           GROUP BY distinct_id \
         ) \
         WHERE level = {{target_level:UInt32}}"
    );

    // -- Parámetros compartidos (todos los Strings viven en el scope de la fn).
    let event_keys: Vec<String> = (0..req.steps.len())
        .map(|i| format!("event_{i}"))
        .collect();
    let mut base_params: Vec<(&str, &str)> = Vec::with_capacity(req.steps.len() + 8);
    base_params.push(("window", &window_s));
    base_params.push(("from", &from_s));
    base_params.push(("to", &to_s));
    base_params.push(("target_level", &target_level_s));
    for (i, ev) in req.steps.iter().enumerate() {
        base_params.push((event_keys[i].as_str(), ev.as_str()));
    }
    if let Some(p) = &req.project {
        if !p.is_empty() {
            base_params.push(("project", p.as_str()));
        }
    }

    #[derive(Debug, Deserialize)]
    struct CountRow {
        users: u64,
    }
    let cohort: Option<CountRow> = state
        .ch
        .select_one_with_params(&cohort_sql, &base_params)
        .await?;
    let dropped_users = cohort.map(|c| c.users).unwrap_or(0);

    // -- Si nadie cae en el cohort, no hay nada que mirar después.
    let top_events: Vec<DropOffEvent> = if dropped_users == 0 {
        Vec::new()
    } else {
        // -- Query 2: para el cohort, anclar al primer `event_name = events[step]`
        //    dentro del rango, y agregar eventos posteriores en `(anchor, anchor + lookahead]`.
        //
        // Notas de SQL:
        //  * `INNER JOIN ... USING` con un sub-select pequeño (el cohort/anchors)
        //    es el patrón rápido en ClickHouse: el lado derecho se hash-broadcasta.
        //  * Filtramos `pe.event_name != step_event` para no contar la propia
        //    re-ejecución del paso N como insight.
        //  * El `step_event` en el JSON viene de la lista controlada del frontend;
        //    igualmente lo bindeamos como parámetro `step_event:String` para
        //    mantener la regla "input del usuario no toca el SQL crudo".
        let drop_sql = format!(
            "WITH \
               cohort AS ( \
                 SELECT distinct_id \
                 FROM ( \
                   SELECT distinct_id, windowFunnel({{window:UInt32}})(timestamp, {conds}) AS level \
                   FROM faro.product_events \
                   WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
                     AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
                     AND event_name IN ({in_list}){proj_clause_plain} \
                   GROUP BY distinct_id \
                 ) \
                 WHERE level = {{target_level:UInt32}} \
               ), \
               anchors AS ( \
                 SELECT distinct_id, min(timestamp) AS anchor_ts \
                 FROM faro.product_events \
                 WHERE distinct_id IN (SELECT distinct_id FROM cohort) \
                   AND timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
                   AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
                   AND event_name = {{step_event:String}}{proj_clause_plain} \
                 GROUP BY distinct_id \
               ) \
             SELECT pe.event_name AS event_name, \
                    toUInt64(uniqExact(pe.distinct_id)) AS users, \
                    toUInt64(count()) AS occurrences \
             FROM faro.product_events AS pe \
             INNER JOIN anchors AS a USING (distinct_id) \
             WHERE pe.timestamp >  a.anchor_ts \
               AND pe.timestamp <= a.anchor_ts + toIntervalSecond({{lookahead:UInt32}}) \
               AND pe.event_name != {{step_event:String}}{proj_clause_pe} \
             GROUP BY event_name \
             ORDER BY users DESC, occurrences DESC \
             LIMIT {{drop_limit:UInt32}}"
        );

        let step_event = req.steps[req.step_index].as_str();
        let mut params = base_params.clone();
        params.push(("lookahead", &lookahead_s));
        params.push(("drop_limit", &limit_s));
        params.push(("step_event", step_event));

        #[derive(Debug, Deserialize)]
        struct DropRow {
            event_name: String,
            users: u64,
            occurrences: u64,
        }
        let rows: Vec<DropRow> = state.ch.select_with_params(&drop_sql, &params).await?;
        rows.into_iter()
            .map(|r| {
                let share = if dropped_users == 0 {
                    0.0
                } else {
                    r.users as f32 / dropped_users as f32
                };
                DropOffEvent {
                    event_name: r.event_name,
                    users: r.users,
                    occurrences: r.occurrences,
                    share,
                }
            })
            .collect()
    };

    let step_event = req.steps[req.step_index].clone();
    let next_event = req.steps[req.step_index + 1].clone();

    Ok(Json(DropOffResult {
        step_index: req.step_index,
        step_event,
        next_event,
        dropped_users,
        lookahead_seconds: lookahead,
        window_seconds: window,
        from: from.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        to: to.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        top_events,
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

// ---------------------------------------------------------------------------
// POST /funnels/time-to-convert
// ---------------------------------------------------------------------------
//
// Para usuarios que dispararon `event_from` y después `event_to`, distribución
// del delta en segundos. Buckets log-scale fijos (≤10s ... >30d) porque "5 seg"
// y "5 horas" y "5 días" son comportamientos cualitativamente distintos y un
// histograma lineal los aplasta. Devuelve también p50/p90/p99 + total_with_from
// para que la UI muestre conversion rate vs sólo gente que llegó al primer paso.

/// Bordes en segundos. El i-ésimo bin cubre `[BUCKET_EDGES[i], BUCKET_EDGES[i+1])`,
/// y el último bin (índice `BUCKET_EDGES.len()-1`) es el catch-all `[> última]`.
/// Si cambian, mantener sincronizado con el render del frontend (sólo labels).
const BUCKET_EDGES: &[u64] = &[
    0, 10,        // 10 s
    60,        // 1 min
    300,       // 5 min
    1_800,     // 30 min
    7_200,     // 2 h
    43_200,    // 12 h
    86_400,    // 1 d
    604_800,   // 7 d
    2_592_000, // 30 d
];
const MAX_TIME_TO_CONVERT_SECS: u32 = 90 * 86_400; // 90 días — más allá no es "conversión".

#[derive(Debug, Deserialize, ToSchema)]
pub struct TimeToConvertRequest {
    pub event_from: String,
    pub event_to: String,
    /// Tope de la ventana de conversión por usuario en segundos. Default 30 días.
    /// Acotamos para que la query no explore deltas absurdos.
    #[serde(default)]
    pub max_seconds: Option<u32>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub last_minutes: Option<i64>,
    pub project: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TimeBin {
    /// Borde inferior en segundos (incluido).
    pub lower_seconds: u64,
    /// Borde superior en segundos (excluido). `None` para el último catch-all.
    pub upper_seconds: Option<u64>,
    pub users: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TimeToConvertResult {
    pub event_from: String,
    pub event_to: String,
    /// Total de distinct_ids que dispararon `event_from` en el rango.
    pub total_with_from: u64,
    /// Usuarios que tras `event_from` dispararon `event_to` dentro de `max_seconds`.
    pub total_converted: u64,
    /// Percentiles del delta en segundos. 0 si no hubo conversiones.
    pub p50_seconds: u64,
    pub p90_seconds: u64,
    pub p99_seconds: u64,
    pub min_seconds: u64,
    pub max_seconds_observed: u64,
    pub bins: Vec<TimeBin>,
    pub max_seconds: u32,
    pub from: String,
    pub to: String,
    pub took_ms: u64,
}

async fn time_to_convert(
    State(state): State<SharedState>,
    Json(req): Json<TimeToConvertRequest>,
) -> ApiResult<Json<TimeToConvertResult>> {
    let started = Instant::now();

    if req.event_from.trim().is_empty() || req.event_to.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "event_from y event_to no pueden ser vacíos".into(),
        ));
    }
    if req.event_from == req.event_to {
        return Err(ApiError::BadRequest(
            "event_from y event_to deben ser distintos".into(),
        ));
    }

    let max_secs = req
        .max_seconds
        .unwrap_or(30 * 86_400)
        .clamp(60, MAX_TIME_TO_CONVERT_SECS);

    let to = req.to.unwrap_or_else(Utc::now);
    let from = req.from.unwrap_or_else(|| match req.last_minutes {
        Some(m) if m > 0 => to - Duration::minutes(m),
        _ => to - Duration::days(7),
    });
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }

    let from_s = ch_dt(from);
    let to_s = ch_dt(to);
    let max_secs_s = max_secs.to_string();

    let proj_clause_pe = match &req.project {
        Some(p) if !p.is_empty() => " AND pe.project_id = {project:String}",
        _ => "",
    };
    let proj_clause_plain = match &req.project {
        Some(p) if !p.is_empty() => " AND project_id = {project:String}",
        _ => "",
    };

    let params: Vec<(&str, &str)> = {
        let mut p: Vec<(&str, &str)> = vec![
            ("from", from_s.as_str()),
            ("to", to_s.as_str()),
            ("event_from", req.event_from.as_str()),
            ("event_to", req.event_to.as_str()),
            ("max_secs", max_secs_s.as_str()),
        ];
        if let Some(proj) = &req.project {
            if !proj.is_empty() {
                p.push(("project", proj.as_str()));
            }
        }
        p
    };

    // Las dos queries comparten `conversions`. ClickHouse no cachea CTEs entre
    // statements, así que el JOIN se ejecuta dos veces — el coste real es bajo
    // porque el cohort es pequeño y el bloom-filter de event_name corta granules.
    // Si esto se vuelve cuello de botella, se puede materializar en un solo
    // query devolviendo bins + stats en una sola row con arrays.
    let conversions_cte = format!(
        "conversions AS ( \
           SELECT pu.distinct_id AS distinct_id, \
                  toUInt64(dateDiff('second', pu.ts_from, min(pe.timestamp))) AS delta_s \
           FROM ( \
             SELECT distinct_id, min(timestamp) AS ts_from \
             FROM faro.product_events \
             WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
               AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
               AND event_name =  {{event_from:String}}{proj_clause_plain} \
             GROUP BY distinct_id \
           ) AS pu \
           INNER JOIN faro.product_events AS pe USING (distinct_id) \
           WHERE pe.event_name = {{event_to:String}} \
             AND pe.timestamp > pu.ts_from \
             AND pe.timestamp <= pu.ts_from + toIntervalSecond({{max_secs:UInt32}}){proj_clause_pe} \
           GROUP BY pu.distinct_id, pu.ts_from \
         )"
    );

    // -- Query 1: stats + total_with_from (escalar via subselect).
    let stats_sql = format!(
        "WITH {conversions_cte} \
         SELECT \
           (SELECT toUInt64(uniqExact(distinct_id)) \
            FROM faro.product_events \
            WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
              AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
              AND event_name =  {{event_from:String}}{proj_clause_plain}) AS total_with_from, \
           toUInt64(count()) AS total_converted, \
           toUInt64(if(count() > 0, quantileExact(0.5)(delta_s), 0)) AS p50_seconds, \
           toUInt64(if(count() > 0, quantileExact(0.9)(delta_s), 0)) AS p90_seconds, \
           toUInt64(if(count() > 0, quantileExact(0.99)(delta_s), 0)) AS p99_seconds, \
           toUInt64(if(count() > 0, min(delta_s), 0)) AS min_seconds, \
           toUInt64(if(count() > 0, max(delta_s), 0)) AS max_seconds_observed \
         FROM conversions"
    );

    #[derive(Debug, Deserialize)]
    struct StatsRow {
        total_with_from: u64,
        total_converted: u64,
        p50_seconds: u64,
        p90_seconds: u64,
        p99_seconds: u64,
        min_seconds: u64,
        max_seconds_observed: u64,
    }
    let stats: StatsRow = state
        .ch
        .select_one_with_params::<StatsRow>(&stats_sql, &params)
        .await?
        .unwrap_or(StatsRow {
            total_with_from: 0,
            total_converted: 0,
            p50_seconds: 0,
            p90_seconds: 0,
            p99_seconds: 0,
            min_seconds: 0,
            max_seconds_observed: 0,
        });

    // -- Query 2: histograma log-bucket. Los bordes están hardcodeados arriba
    //    en `BUCKET_EDGES`, así que generamos el `multiIf` dinámicamente para
    //    que ambos lados se mantengan sincronizados.
    let bin_count = BUCKET_EDGES.len(); // último índice = catch-all
    let mut multi_if = String::from("multiIf(");
    // Los primeros len-1 son los rangos [edges[i], edges[i+1]).
    for i in 1..bin_count {
        multi_if.push_str(&format!("delta_s < {}, {}, ", BUCKET_EDGES[i], i - 1));
    }
    // catch-all: índice = bin_count - 1.
    multi_if.push_str(&format!("{})", bin_count - 1));

    let bins_sql = format!(
        "WITH {conversions_cte} \
         SELECT toUInt32(bucket_idx) AS bucket_idx, toUInt64(count()) AS users \
         FROM ( \
           SELECT {multi_if} AS bucket_idx \
           FROM conversions \
         ) \
         GROUP BY bucket_idx \
         ORDER BY bucket_idx"
    );

    #[derive(Debug, Deserialize)]
    struct BinRow {
        bucket_idx: u32,
        users: u64,
    }
    let raw_bins: Vec<BinRow> = state.ch.select_with_params(&bins_sql, &params).await?;

    // Densificar: devolver TODOS los bins aunque users=0, para que el frontend
    // dibuje el histograma con todas las barras alineadas independientemente del
    // dataset. Si rebasamos `bin_count` es bug (multiIf garantiza < bin_count).
    let mut users_per_bin = vec![0u64; bin_count];
    for r in raw_bins {
        let idx = (r.bucket_idx as usize).min(bin_count - 1);
        users_per_bin[idx] = r.users;
    }

    let bins: Vec<TimeBin> = users_per_bin
        .into_iter()
        .enumerate()
        .map(|(i, users)| {
            // Rango [edges[i], edges[i+1]) salvo el último, que es catch-all sin tope.
            let lower = BUCKET_EDGES[i];
            let upper = if i + 1 < BUCKET_EDGES.len() {
                Some(BUCKET_EDGES[i + 1])
            } else {
                None
            };
            TimeBin {
                lower_seconds: lower,
                upper_seconds: upper,
                users,
            }
        })
        .collect();

    Ok(Json(TimeToConvertResult {
        event_from: req.event_from,
        event_to: req.event_to,
        total_with_from: stats.total_with_from,
        total_converted: stats.total_converted,
        p50_seconds: stats.p50_seconds,
        p90_seconds: stats.p90_seconds,
        p99_seconds: stats.p99_seconds,
        min_seconds: stats.min_seconds,
        max_seconds_observed: stats.max_seconds_observed,
        bins,
        max_seconds: max_secs,
        from: from.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        to: to.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_users_cumulative_from_levels() {
        // 3 pasos. windowFunnel devuelve por usuario el nivel máximo alcanzado.
        // 10 usuarios llegaron a paso 1, 4 a paso 2, 2 a paso 3.
        let rows = vec![(1u32, 6u64), (2, 2), (3, 2)];
        let n = 3;
        let mut step_users = vec![0u64; n];
        for (level, users) in rows {
            let reached = (level as usize).min(n);
            for i in 0..reached {
                step_users[i] += users;
            }
        }
        assert_eq!(step_users, vec![10, 4, 2]);
    }
}
