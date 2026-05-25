//! Endpoints del usuario unificado multi-device (goal 10.E.1).
//!
//! Cuando un mismo humano interactúa desde web (anon-A → user_42 tras login)
//! y desde mobile (anon-B → user_42 tras login), Faro debe poder responder:
//!
//!   * "lista de usuarios identificados con su breakdown de devices" → `GET /product-users`.
//!   * "todo lo que sé del user_42" → `GET /product-users/:distinct_id`.
//!   * "todos los eventos del user_42 en CUALQUIER device" → `GET /product-users/:distinct_id/events`.
//!
//! La unificación la mantiene el worker `user_unifier`. Esta capa sólo lee:
//!  * `faro.product_users FINAL` para el row canónico de cada usuario,
//!  * `faro.product_user_aliases FINAL` para expandir los anon_ids ligados a
//!    un distinct_id (útil para incluir eventos pre-identify),
//!  * `faro.product_events` para los eventos efectivos del usuario,
//!    filtrando por `distinct_id = :id OR anonymous_id IN (...)`.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::api::params::{ch_dt, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::storage::ProductEventRow;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/product-users", get(list_users))
        .route("/product-users/{distinct_id}", get(get_user))
        .route("/product-users/{distinct_id}/events", get(user_events))
}

// ---------------------------------------------------------------------------
// GET /product-users
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ProductUserSummary {
    pub project_id: String,
    pub distinct_id: String,
    pub first_seen: String,
    pub last_seen: String,
    pub anonymous_ids: Vec<String>,
    pub sources: Vec<String>,
    pub event_count: u64,
    pub properties: String,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(flatten)]
    pub range: Range,
    /// Substring opcional: matchea contra `distinct_id` o el JSON crudo de
    /// `properties` (case-insensitive). Sirve para buscar "victor@..." sin
    /// saber el distinct_id exacto.
    pub query: Option<String>,
    /// Filtra usuarios vistos en al menos uno de estos sources. Para detectar
    /// usuarios que existen en web pero no en mobile (o viceversa).
    #[serde(default)]
    pub source: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UserRow {
    project_id: String,
    distinct_id: String,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    first_seen: DateTime<Utc>,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    last_seen: DateTime<Utc>,
    #[serde(default)]
    anonymous_ids: Vec<String>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    event_count: u64,
    #[serde(default)]
    properties: String,
}

async fn list_users(
    State(state): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<ProductUserSummary>>> {
    // El filtro temporal aplica a `last_seen` — "usuarios activos en el rango".
    // Pasarle `from = 0` (sin filtro) listaría TODO el padron histórico, lo
    // cual no es lo que se quiere en una vista de "users active this week".
    let (from, to) = q.range.resolve();
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);

    let mut sql = String::from(
        "SELECT project_id, distinct_id, first_seen, last_seen, \
                anonymous_ids, sources, event_count, properties \
         FROM faro.product_users FINAL \
         WHERE last_seen >= toDateTime64({from:DateTime64(9)}, 9) \
           AND last_seen <  toDateTime64({to:DateTime64(9)}, 9)",
    );
    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];

    let (proj_clause, proj_val) = q.range.project_clause("");
    sql.push_str(&proj_clause);
    if let Some(p) = proj_val {
        params.push(("project", p));
    }

    if let Some(qs) = q.query.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(
            " AND (positionCaseInsensitive(distinct_id, {q:String}) > 0 \
                  OR positionCaseInsensitive(properties, {q:String}) > 0)",
        );
        params.push(("q", qs.as_str()));
    }

    // Filtros por source. Cada uno es un `has(sources, {source_i:String})`.
    // Una sola query puede pedir varios sources — semántica AND ("vistos
    // tanto en web como en mobile") para distinguir usuarios "cross-device"
    // de los single-device.
    let source_keys: Vec<String> = (0..q.source.len()).map(|i| format!("source_{i}")).collect();
    for (i, src) in q.source.iter().enumerate() {
        if src.is_empty() {
            continue;
        }
        sql.push_str(&format!(
            " AND has(sources, {{{name}:String}})",
            name = source_keys[i]
        ));
        params.push((source_keys[i].as_str(), src.as_str()));
    }

    let limit = q.range.limit();
    let tail = format!(" ORDER BY last_seen DESC LIMIT {limit}");
    sql.push_str(&tail);

    let rows: Vec<UserRow> = state.ch.select_with_params(&sql, &params).await?;
    let out: Vec<ProductUserSummary> = rows.into_iter().map(into_summary).collect();
    Ok(Json(out))
}

fn into_summary(r: UserRow) -> ProductUserSummary {
    ProductUserSummary {
        project_id: r.project_id,
        distinct_id: r.distinct_id,
        first_seen: r
            .first_seen
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        last_seen: r
            .last_seen
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        anonymous_ids: r.anonymous_ids,
        sources: r.sources,
        event_count: r.event_count,
        properties: r.properties,
    }
}

// ---------------------------------------------------------------------------
// GET /product-users/:distinct_id
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct DeviceBreakdown {
    pub source: String,
    pub event_count: u64,
    pub last_seen: String,
    /// Número de anonymous_ids distintos que hemos visto bajo este source
    /// para este user. Sirve para detectar "el mismo user con 3 anon ids
    /// distintos en mobile" (típico: re-instalación, cache wipe).
    pub anonymous_id_count: u64,
}

#[derive(Debug, Serialize)]
pub struct ProductUserDetail {
    #[serde(flatten)]
    pub summary: ProductUserSummary,
    /// Breakdown por device/source en el rango pedido. Calculado en vivo
    /// contra `product_events` — más caro que la vista lista pero permite
    /// "user_42 en los últimos 7 días": cuántos eventos por device.
    pub devices: Vec<DeviceBreakdown>,
}

#[derive(Debug, Deserialize)]
pub struct DetailQuery {
    #[serde(flatten)]
    pub range: Range,
}

async fn get_user(
    State(state): State<SharedState>,
    Path(distinct_id): Path<String>,
    Query(q): Query<DetailQuery>,
) -> ApiResult<Json<ProductUserDetail>> {
    if distinct_id.is_empty() {
        return Err(ApiError::BadRequest("distinct_id requerido".into()));
    }

    // 1) Row canónico (con FINAL para que ReplacingMergeTree dedupe).
    let mut sql = String::from(
        "SELECT project_id, distinct_id, first_seen, last_seen, \
                anonymous_ids, sources, event_count, properties \
         FROM faro.product_users FINAL \
         WHERE distinct_id = {distinct_id:String}",
    );
    let mut params: Vec<(&str, &str)> = vec![("distinct_id", distinct_id.as_str())];
    let (proj_clause, proj_val) = q.range.project_clause("");
    sql.push_str(&proj_clause);
    if let Some(p) = proj_val {
        params.push(("project", p));
    }
    sql.push_str(" ORDER BY last_seen DESC LIMIT 1");

    let row: Option<UserRow> = state.ch.select_one_with_params(&sql, &params).await?;
    let user = row.ok_or(ApiError::NotFound)?;

    // 2) Breakdown por device en el rango. Joineamos contra los anon ids
    //    ligados a este user — eventos pre-login también cuentan.
    let (from, to) = q.range.resolve();
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);

    let all_ids =
        collect_user_ids(&state, &user.project_id, &distinct_id, &user.anonymous_ids).await?;
    let breakdown = compute_breakdown(&state, &user.project_id, &all_ids, &from_s, &to_s).await?;

    Ok(Json(ProductUserDetail {
        summary: into_summary(user),
        devices: breakdown,
    }))
}

#[derive(Debug, Deserialize)]
struct DeviceRow {
    source: String,
    event_count: u64,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    last_seen: DateTime<Utc>,
    anonymous_id_count: u64,
}

async fn compute_breakdown(
    state: &SharedState,
    project_id: &str,
    all_ids: &[String],
    from_s: &str,
    to_s: &str,
) -> ApiResult<Vec<DeviceBreakdown>> {
    // Construir IN-list. Si está vacío (no debería pasar — al menos
    // distinct_id está) devolvemos breakdown vacío.
    if all_ids.is_empty() {
        return Ok(Vec::new());
    }
    let names: Vec<String> = (0..all_ids.len()).map(|i| format!("id_{i}")).collect();
    let in_list = names
        .iter()
        .map(|n| format!("{{{n}:String}}"))
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "SELECT toString(source) AS source, \
                toUInt64(count()) AS event_count, \
                max(timestamp) AS last_seen, \
                toUInt64(uniqExact(anonymous_id)) AS anonymous_id_count \
         FROM faro.product_events \
         WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
           AND project_id = {{project:String}} \
           AND (distinct_id IN ({in_list}) OR anonymous_id IN ({in_list})) \
         GROUP BY source \
         ORDER BY event_count DESC"
    );

    let mut params: Vec<(&str, &str)> =
        vec![("from", from_s), ("to", to_s), ("project", project_id)];
    for (i, id) in all_ids.iter().enumerate() {
        params.push((names[i].as_str(), id.as_str()));
    }

    let rows: Vec<DeviceRow> = state.ch.select_with_params(&sql, &params).await?;
    Ok(rows
        .into_iter()
        .map(|r| DeviceBreakdown {
            source: r.source,
            event_count: r.event_count,
            last_seen: r
                .last_seen
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            anonymous_id_count: r.anonymous_id_count,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// GET /product-users/:distinct_id/events
// ---------------------------------------------------------------------------
//
// Este es EL endpoint del goal 10.E.1: "todos los events de user_42 en
// cualquier device". Resuelve los anon_ids ligados al distinct_id (vía
// product_user_aliases) y filtra product_events con `distinct_id = X OR
// anonymous_id IN (anons)`.

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    #[serde(flatten)]
    pub range: Range,
    /// Opcional: limitar a un source (web|mobile|backend) para el feed
    /// "actividad del user en web" sin tocar la vista global.
    pub source: Option<String>,
}

async fn user_events(
    State(state): State<SharedState>,
    Path(distinct_id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> ApiResult<Json<Vec<ProductEventRow>>> {
    if distinct_id.is_empty() {
        return Err(ApiError::BadRequest("distinct_id requerido".into()));
    }

    // Resolver project_id efectivo. Si el cliente lo pasa explícitamente lo
    // usamos; si no, recuperamos el primer match desde product_users.
    let project_id = match q.range.project.as_deref().filter(|s| !s.is_empty()) {
        Some(p) => p.to_string(),
        None => resolve_user_project(&state, &distinct_id)
            .await?
            .ok_or(ApiError::NotFound)?,
    };

    // Los anon_ids vienen del row canónico del usuario; si el worker aún no
    // procesó al user (caso primer login), caemos al lookup de aliases para
    // no quedarnos sin nada que mostrar.
    let anons = anonymous_ids_for(&state, &project_id, &distinct_id).await?;
    let all_ids = build_all_ids(&distinct_id, &anons);

    let (from, to) = q.range.resolve();
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);

    let names: Vec<String> = (0..all_ids.len()).map(|i| format!("id_{i}")).collect();
    let in_list = names
        .iter()
        .map(|n| format!("{{{n}:String}}"))
        .collect::<Vec<_>>()
        .join(", ");

    let mut sql = format!(
        "SELECT timestamp, project_id, event_name, distinct_id, anonymous_id, \
                session_id, properties, user_properties, context, source, \
                trace_id, span_id, toString(event_id) AS event_id \
         FROM faro.product_events \
         WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
           AND project_id = {{project:String}} \
           AND (distinct_id IN ({in_list}) OR anonymous_id IN ({in_list}))"
    );
    let mut params: Vec<(&str, &str)> =
        vec![("from", &from_s), ("to", &to_s), ("project", &project_id)];
    for (i, id) in all_ids.iter().enumerate() {
        params.push((names[i].as_str(), id.as_str()));
    }

    if let Some(s) = q.source.as_deref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND source = {source:String}");
        params.push(("source", s));
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

// ---------------------------------------------------------------------------
// Helpers compartidos
// ---------------------------------------------------------------------------

async fn resolve_user_project(state: &SharedState, distinct_id: &str) -> ApiResult<Option<String>> {
    #[derive(Deserialize)]
    struct R {
        project_id: String,
    }
    let row: Option<R> = state
        .ch
        .select_one_with_params(
            "SELECT project_id FROM faro.product_users FINAL \
             WHERE distinct_id = {distinct_id:String} \
             ORDER BY last_seen DESC LIMIT 1",
            &[("distinct_id", distinct_id)],
        )
        .await?;
    Ok(row.map(|r| r.project_id))
}

async fn anonymous_ids_for(
    state: &SharedState,
    project_id: &str,
    distinct_id: &str,
) -> ApiResult<Vec<String>> {
    #[derive(Deserialize)]
    struct R {
        #[serde(default)]
        anonymous_ids: Vec<String>,
    }
    let row: Option<R> = state
        .ch
        .select_one_with_params(
            "SELECT anonymous_ids FROM faro.product_users FINAL \
             WHERE project_id = {project:String} AND distinct_id = {distinct_id:String}",
            &[("project", project_id), ("distinct_id", distinct_id)],
        )
        .await?;
    if let Some(r) = row {
        if !r.anonymous_ids.is_empty() {
            return Ok(r.anonymous_ids);
        }
    }
    // Fallback: si user_unifier todavía no procesó al user, tomar la lista
    // de aliases inversos. Esto cubre la ventana inicial post-identify
    // antes del primer tick del worker.
    #[derive(Deserialize)]
    struct A {
        anonymous_id: String,
    }
    let aliases: Vec<A> = state
        .ch
        .select_with_params(
            "SELECT anonymous_id FROM faro.product_user_aliases FINAL \
             WHERE project_id = {project:String} AND distinct_id = {distinct_id:String} \
             LIMIT 1000",
            &[("project", project_id), ("distinct_id", distinct_id)],
        )
        .await?;
    Ok(aliases.into_iter().map(|a| a.anonymous_id).collect())
}

/// Unión del distinct_id con sus anon_ids, sin duplicados y sin vacíos.
fn build_all_ids(distinct_id: &str, anons: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(1 + anons.len());
    out.push(distinct_id.to_string());
    for a in anons {
        if !a.is_empty() && a != distinct_id {
            out.push(a.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

async fn collect_user_ids(
    state: &SharedState,
    project_id: &str,
    distinct_id: &str,
    anons_from_row: &[String],
) -> ApiResult<Vec<String>> {
    if !anons_from_row.is_empty() {
        return Ok(build_all_ids(distinct_id, anons_from_row));
    }
    let anons = anonymous_ids_for(state, project_id, distinct_id).await?;
    Ok(build_all_ids(distinct_id, &anons))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_all_ids_includes_distinct() {
        let v = build_all_ids("user_42", &["anon-A".into(), "anon-B".into()]);
        assert_eq!(v, vec!["anon-A", "anon-B", "user_42"]);
    }

    #[test]
    fn build_all_ids_dedupes_and_drops_empty() {
        let v = build_all_ids(
            "user_42",
            &[
                "".into(),
                "user_42".into(),
                "anon-A".into(),
                "anon-A".into(),
            ],
        );
        assert_eq!(v, vec!["anon-A", "user_42"]);
    }

    #[test]
    fn build_all_ids_lone_distinct() {
        let v = build_all_ids("user_42", &[]);
        assert_eq!(v, vec!["user_42"]);
    }
}
