//! Cohorts: segmentación de usuarios sobre `faro.product_events`.
//!
//! Endpoints:
//!   * `GET    /cohorts`                    → lista (FINAL, soft-delete filtrado).
//!   * `POST   /cohorts`                    → crear (valida + persiste).
//!   * `GET    /cohorts/:id`                → leer uno.
//!   * `PUT    /cohorts/:id`                → editar (bumpea version).
//!   * `DELETE /cohorts/:id`                → soft-delete.
//!   * `POST   /cohorts/preview`            → evaluar una definition sin guardar
//!                                            (devuelve size + sample de distinct_id).
//!   * `GET    /cohorts/:id/users`          → sample paginable de miembros del cohort.
//!   * `GET    /cohorts/:id/retention`      → fracción del cohort activa por día
//!                                            (mirando hacia atrás desde hoy).
//!   * `GET    /cohorts/:id/overlap`        → intersección con `?other=<uuid>`.
//!
//! Reglas:
//!   * El cohort se evalúa AL VUELO contra `faro.product_events` con un SELECT
//!     parametrizado por ClickHouse (todos los valores del usuario van como
//!     `param_<name>`, nunca como interpolación). El input del usuario solo
//!     decide el shape del WHERE (qué claves de filtros añadir), nunca el SQL.
//!   * `op` se valida en una whitelist (`==/>=/>/<=/<`); cualquier otra cosa
//!     responde 400.
//!   * Tope práctico: 3 filtros sobre properties (más allá pierde el bloom y
//!     ataca columnas ZSTD(3) pesadas).

use std::time::Instant;

use axum::extract::{Path, Query as AxumQuery, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::storage::{CohortDefinition, CohortRow};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/cohorts", get(list_cohorts).post(create_cohort))
        .route("/cohorts/preview", post(preview_cohort))
        .route(
            "/cohorts/:id",
            get(get_cohort).put(update_cohort).delete(delete_cohort),
        )
        .route("/cohorts/:id/users", get(cohort_users))
        .route("/cohorts/:id/retention", get(cohort_retention))
        .route("/cohorts/:id/overlap", get(cohort_overlap))
}

// ---------------------------------------------------------------------------
// Validación & helpers
// ---------------------------------------------------------------------------

const MAX_FILTERS: usize = 3;
const MAX_LAST_DAYS: u32 = 365;
const MIN_LAST_DAYS: u32 = 1;
const MAX_COUNT: u32 = 1_000_000;
const MAX_RETENTION_HORIZON: u32 = 90;
const DEFAULT_RETENTION_HORIZON: u32 = 30;
const MAX_USERS_LIMIT: u32 = 500;
const DEFAULT_USERS_LIMIT: u32 = 100;
const MAX_NAME_LEN: usize = 200;
const MAX_KEY_LEN: usize = 128;
const MAX_VAL_LEN: usize = 256;

const COHORT_COLS: &str = "id, project_id, name, description, definition, created_at, updated_at, \
     created_by, deleted, version";

/// Whitelist de operadores comparativos. Cualquier valor fuera de aquí se
/// rechaza con 400 — esto es lo que protege contra inyección de SQL en `op`,
/// que se interpola directamente en el HAVING (no se puede parametrizar
/// el operador en una query parametrizada).
fn validated_op(op: &str) -> Result<&'static str, ApiError> {
    match op {
        "==" => Ok("="),
        "=" => Ok("="),
        ">=" => Ok(">="),
        ">" => Ok(">"),
        "<=" => Ok("<="),
        "<" => Ok("<"),
        _ => Err(ApiError::BadRequest(format!(
            "operador no soportado: '{op}' (válidos: ==, >=, >, <=, <)"
        ))),
    }
}

fn validate_def(def: &CohortDefinition) -> ApiResult<()> {
    if def.event.trim().is_empty() {
        return Err(ApiError::BadRequest("event no puede ser vacío".into()));
    }
    if def.event.len() > MAX_KEY_LEN {
        return Err(ApiError::BadRequest("event demasiado largo".into()));
    }
    validated_op(&def.op)?;
    if def.count == 0 || def.count > MAX_COUNT {
        return Err(ApiError::BadRequest(format!(
            "count fuera de rango [1, {MAX_COUNT}]"
        )));
    }
    if def.last_days < MIN_LAST_DAYS || def.last_days > MAX_LAST_DAYS {
        return Err(ApiError::BadRequest(format!(
            "last_days fuera de rango [{MIN_LAST_DAYS}, {MAX_LAST_DAYS}]"
        )));
    }
    let total_filters = def.filters.len() + def.user_filters.len();
    if total_filters > MAX_FILTERS {
        return Err(ApiError::BadRequest(format!(
            "máximo {MAX_FILTERS} filtros combinados sobre properties de evento y usuario"
        )));
    }
    for f in def.filters.iter().chain(def.user_filters.iter()) {
        if f.key.trim().is_empty() || f.value.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "filtros sobre properties: key y value no pueden ser vacíos".into(),
            ));
        }
        if f.key.len() > MAX_KEY_LEN || f.value.len() > MAX_VAL_LEN {
            return Err(ApiError::BadRequest("filtro demasiado largo".into()));
        }
    }
    Ok(())
}

fn parse_def(raw: &str) -> ApiResult<CohortDefinition> {
    serde_json::from_str(raw).map_err(|e| ApiError::BadRequest(format!("definition inválida: {e}")))
}

/// Construye el sub-SELECT que devuelve los `distinct_id` del cohort. Las
/// claves de parámetros se generan con `prefix` para evitar colisiones cuando
/// dos sub-queries del mismo statement comparten parámetros (e.g. overlap).
///
/// Devuelve:
///   * `sql` — el sub-SELECT (sin alias; el caller decide cómo envolverlo).
///   * `params` — pares `(name, value)` listos para `ch.select_with_params`.
///     Los `String` viven en el contenedor que devuelve la función para que el
///     borrow del caller siga siendo válido durante el await.
struct CohortQuery {
    sql: String,
    /// Storage propio de los `String` que después se pasan como `&str` en
    /// `params`. Evita que las keys/valores se dropeen antes del await.
    owned: Vec<(String, String)>,
}

impl CohortQuery {
    fn params(&self) -> Vec<(&str, &str)> {
        self.owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

fn build_cohort_query(
    def: &CohortDefinition,
    project_id: &str,
    prefix: &str,
) -> ApiResult<CohortQuery> {
    let op = validated_op(&def.op)?;
    let last_days = def.last_days.clamp(MIN_LAST_DAYS, MAX_LAST_DAYS);
    let has_user_filters = !def.user_filters.is_empty();
    let mut owned: Vec<(String, String)> =
        Vec::with_capacity(4 + (def.filters.len() + def.user_filters.len()) * 2);
    owned.push((format!("{prefix}event"), def.event.clone()));
    owned.push((format!("{prefix}count"), def.count.to_string()));
    owned.push((format!("{prefix}last_days"), last_days.to_string()));
    owned.push((format!("{prefix}project"), project_id.to_string()));

    let mut filter_clauses = String::new();
    let event_properties = if has_user_filters {
        "e.properties"
    } else {
        "properties"
    };
    for (i, f) in def.filters.iter().enumerate() {
        let kp = format!("{prefix}fk_{i}");
        let vp = format!("{prefix}fv_{i}");
        filter_clauses.push_str(&format!(
            " AND JSONExtractString({event_properties}, {{{kp}:String}}) = {{{vp}:String}}"
        ));
        owned.push((kp, f.key.clone()));
        owned.push((vp, f.value.clone()));
    }
    for (i, f) in def.user_filters.iter().enumerate() {
        let kp = format!("{prefix}ufk_{i}");
        let vp = format!("{prefix}ufv_{i}");
        filter_clauses.push_str(&format!(
            " AND JSONExtractString(u.properties, {{{kp}:String}}) = {{{vp}:String}}"
        ));
        owned.push((kp, f.key.clone()));
        owned.push((vp, f.value.clone()));
    }

    // `op` ya está sanitizado por validated_op (string literal '=' / '>=' / etc.),
    // así que es seguro interpolarlo en el HAVING. Todo lo demás va vía
    // parámetros server-side.
    let event_p = format!("{prefix}event");
    let count_p = format!("{prefix}count");
    let last_p = format!("{prefix}last_days");
    let proj_p = format!("{prefix}project");
    let sql = if has_user_filters {
        format!(
            "SELECT e.distinct_id \
             FROM faro.product_events AS e \
             INNER JOIN (SELECT project_id, distinct_id, properties FROM faro.product_users FINAL) AS u \
               ON u.project_id = e.project_id \
              AND u.distinct_id = e.distinct_id \
             WHERE e.event_name = {{{event_p}:String}} \
               AND e.timestamp >= now() - toIntervalDay({{{last_p}:UInt32}}) \
               AND e.project_id = {{{proj_p}:String}}{filter_clauses} \
             GROUP BY e.distinct_id \
             HAVING count() {op} {{{count_p}:UInt32}}"
        )
    } else {
        format!(
            "SELECT distinct_id \
             FROM faro.product_events \
             WHERE event_name = {{{event_p}:String}} \
               AND timestamp >= now() - toIntervalDay({{{last_p}:UInt32}}) \
               AND project_id = {{{proj_p}:String}}{filter_clauses} \
             GROUP BY distinct_id \
             HAVING count() {op} {{{count_p}:UInt32}}"
        )
    };

    Ok(CohortQuery { sql, owned })
}

// ---------------------------------------------------------------------------
// Listado / CRUD
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    project: Option<String>,
}

async fn list_cohorts(
    State(state): State<SharedState>,
    AxumQuery(q): AxumQuery<ListQuery>,
) -> ApiResult<Json<Vec<CohortRow>>> {
    let mut params: Vec<(&str, &str)> = Vec::new();
    let proj_clause = match q.project.as_deref() {
        Some(p) if !p.is_empty() => {
            params.push(("project", p));
            " AND project_id = {project:String}"
        }
        _ => "",
    };
    let sql = format!(
        "SELECT {COHORT_COLS} \
         FROM faro.cohorts FINAL \
         WHERE deleted = 0{proj_clause} \
         ORDER BY name"
    );
    let rows: Vec<CohortRow> = state.ch.select_with_params(&sql, &params).await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct CohortInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_project_in")]
    pub project: String,
    pub definition: CohortDefinition,
}

fn default_project_in() -> String {
    "default".into()
}

fn validate_input(input: &CohortInput) -> ApiResult<()> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name no puede ser vacío".into()));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(ApiError::BadRequest("name demasiado largo".into()));
    }
    validate_def(&input.definition)?;
    Ok(())
}

async fn create_cohort(
    State(state): State<SharedState>,
    Json(input): Json<CohortInput>,
) -> ApiResult<Json<CohortRow>> {
    validate_input(&input)?;
    let now = Utc::now();
    let definition_json = serde_json::to_string(&input.definition)
        .map_err(|e| ApiError::BadRequest(format!("no pude serializar definition: {e}")))?;
    let row = CohortRow {
        id: Uuid::new_v4(),
        project_id: if input.project.is_empty() {
            "default".into()
        } else {
            input.project
        },
        name: input.name.trim().to_string(),
        description: input.description,
        definition: definition_json,
        created_at: now,
        updated_at: now,
        created_by: String::new(),
        deleted: 0,
        version: now.timestamp_millis() as u64,
    };
    state.ch.insert("faro.cohorts", &[row.clone()]).await?;
    Ok(Json(row))
}

async fn get_cohort(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CohortRow>> {
    let id_s = id.to_string();
    let sql = format!(
        "SELECT {COHORT_COLS} FROM faro.cohorts FINAL \
         WHERE id = {{id:UUID}} AND deleted = 0 LIMIT 1"
    );
    state
        .ch
        .select_one_with_params::<CohortRow>(&sql, &[("id", &id_s)])
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn update_cohort(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(input): Json<CohortInput>,
) -> ApiResult<Json<CohortRow>> {
    validate_input(&input)?;
    let now = Utc::now();
    let id_s = id.to_string();
    let sql =
        format!("SELECT {COHORT_COLS} FROM faro.cohorts FINAL WHERE id = {{id:UUID}} LIMIT 1");
    let mut row: CohortRow = state
        .ch
        .select_one_with_params(&sql, &[("id", &id_s)])
        .await?
        .ok_or(ApiError::NotFound)?;
    row.name = input.name.trim().to_string();
    row.description = input.description;
    row.definition = serde_json::to_string(&input.definition)
        .map_err(|e| ApiError::BadRequest(format!("no pude serializar definition: {e}")))?;
    row.updated_at = now;
    row.version = now.timestamp_millis() as u64;
    state.ch.insert("faro.cohorts", &[row.clone()]).await?;
    Ok(Json(row))
}

async fn delete_cohort(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let id_s = id.to_string();
    let sql =
        format!("SELECT {COHORT_COLS} FROM faro.cohorts FINAL WHERE id = {{id:UUID}} LIMIT 1");
    let mut row: CohortRow = state
        .ch
        .select_one_with_params(&sql, &[("id", &id_s)])
        .await?
        .ok_or(ApiError::NotFound)?;
    row.deleted = 1;
    let now = Utc::now();
    row.updated_at = now;
    row.version = now.timestamp_millis() as u64;
    state.ch.insert("faro.cohorts", &[row]).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Preview (evaluar sin guardar)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PreviewInput {
    #[serde(default = "default_project_in")]
    pub project: String,
    pub definition: CohortDefinition,
    /// Cuántos distinct_id ejemplo devolver (para que el usuario vea quién cae
    /// en el cohort). Default 20, máx 500.
    #[serde(default)]
    pub sample_limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct PreviewResult {
    pub size: u64,
    pub sample: Vec<String>,
    pub took_ms: u64,
}

async fn preview_cohort(
    State(state): State<SharedState>,
    Json(input): Json<PreviewInput>,
) -> ApiResult<Json<PreviewResult>> {
    validate_def(&input.definition)?;
    let started = Instant::now();
    let proj = if input.project.is_empty() {
        "default"
    } else {
        input.project.as_str()
    };
    let q = build_cohort_query(&input.definition, proj, "")?;
    let (size, sample) = run_size_and_sample(&state, &q, input.sample_limit).await?;
    Ok(Json(PreviewResult {
        size,
        sample,
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

async fn run_size_and_sample(
    state: &SharedState,
    q: &CohortQuery,
    sample_limit: Option<u32>,
) -> ApiResult<(u64, Vec<String>)> {
    // 1) Tamaño exacto.
    #[derive(Debug, Deserialize)]
    struct CountRow {
        users: u64,
    }
    let size_sql = format!(
        "SELECT toUInt64(count()) AS users FROM ({sub})",
        sub = q.sql
    );
    let size: u64 = state
        .ch
        .select_one_with_params::<CountRow>(&size_sql, &q.params())
        .await?
        .map(|r| r.users)
        .unwrap_or(0);

    // 2) Sample para mostrar quién cae.
    let limit = sample_limit
        .unwrap_or(DEFAULT_USERS_LIMIT)
        .clamp(1, MAX_USERS_LIMIT);
    let limit_s = limit.to_string();
    let mut params = q.params();
    params.push(("sample_limit", limit_s.as_str()));
    let sample_sql = format!(
        "SELECT distinct_id FROM ({sub}) ORDER BY distinct_id LIMIT {{sample_limit:UInt32}}",
        sub = q.sql
    );
    #[derive(Debug, Deserialize)]
    struct UserRow {
        distinct_id: String,
    }
    let sample: Vec<String> = state
        .ch
        .select_with_params::<UserRow>(&sample_sql, &params)
        .await?
        .into_iter()
        .map(|r| r.distinct_id)
        .collect();
    Ok((size, sample))
}

// ---------------------------------------------------------------------------
// Users / Retention / Overlap (sobre cohort ya guardado)
// ---------------------------------------------------------------------------

async fn load_cohort(state: &SharedState, id: Uuid) -> ApiResult<CohortRow> {
    let id_s = id.to_string();
    let sql = format!(
        "SELECT {COHORT_COLS} FROM faro.cohorts FINAL \
         WHERE id = {{id:UUID}} AND deleted = 0 LIMIT 1"
    );
    state
        .ch
        .select_one_with_params(&sql, &[("id", &id_s)])
        .await?
        .ok_or(ApiError::NotFound)
}

#[derive(Debug, Deserialize)]
struct UsersQuery {
    #[serde(default)]
    limit: Option<u32>,
}

async fn cohort_users(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    AxumQuery(q): AxumQuery<UsersQuery>,
) -> ApiResult<Json<PreviewResult>> {
    let started = Instant::now();
    let cohort = load_cohort(&state, id).await?;
    let def = parse_def(&cohort.definition)?;
    let cq = build_cohort_query(&def, &cohort.project_id, "")?;
    let (size, sample) = run_size_and_sample(&state, &cq, q.limit).await?;
    Ok(Json(PreviewResult {
        size,
        sample,
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

#[derive(Debug, Deserialize)]
struct RetentionQuery {
    /// Cuántos días hacia atrás mirar la actividad. Default 30, máx 90.
    #[serde(default)]
    horizon_days: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct RetentionPoint {
    /// 0 = hoy, 1 = ayer, … hasta `horizon_days`.
    pub day_back: u32,
    pub active_users: u64,
}

#[derive(Debug, Serialize)]
pub struct RetentionResult {
    pub cohort_size: u64,
    pub horizon_days: u32,
    pub points: Vec<RetentionPoint>,
    pub took_ms: u64,
}

async fn cohort_retention(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    AxumQuery(q): AxumQuery<RetentionQuery>,
) -> ApiResult<Json<RetentionResult>> {
    let started = Instant::now();
    let cohort = load_cohort(&state, id).await?;
    let def = parse_def(&cohort.definition)?;
    let horizon = q
        .horizon_days
        .unwrap_or(DEFAULT_RETENTION_HORIZON)
        .clamp(1, MAX_RETENTION_HORIZON);

    // El cohort se evalúa una vez para obtener `size`; la actividad se mide
    // intersectando contra el sub-SELECT del cohort, que ClickHouse hash-joinea.
    let cq = build_cohort_query(&def, &cohort.project_id, "")?;

    // 1) Tamaño del cohort.
    #[derive(Debug, Deserialize)]
    struct CountRow {
        users: u64,
    }
    let size_sql = format!(
        "SELECT toUInt64(count()) AS users FROM ({sub})",
        sub = cq.sql
    );
    let cohort_size: u64 = state
        .ch
        .select_one_with_params::<CountRow>(&size_sql, &cq.params())
        .await?
        .map(|r| r.users)
        .unwrap_or(0);

    // 2) Actividad diaria de los miembros del cohort dentro del horizon.
    //    `day_back` = days_diff(today(), bucket). Bucket = toDate(timestamp).
    let horizon_s = horizon.to_string();
    let mut params = cq.params();
    params.push(("horizon", horizon_s.as_str()));
    let retention_sql = format!(
        "SELECT toUInt32(dateDiff('day', toDate(timestamp), today())) AS day_back, \
                toUInt64(uniqExact(distinct_id)) AS active_users \
         FROM faro.product_events \
         WHERE distinct_id IN ({sub}) \
           AND project_id = {{{proj}:String}} \
           AND timestamp >= today() - toIntervalDay({{horizon:UInt32}}) \
           AND timestamp <  today() + toIntervalDay(1) \
         GROUP BY day_back \
         ORDER BY day_back",
        sub = cq.sql,
        proj = "project"
    );

    #[derive(Debug, Deserialize)]
    struct RowOut {
        day_back: u32,
        active_users: u64,
    }
    let rows: Vec<RowOut> = state.ch.select_with_params(&retention_sql, &params).await?;
    let points = rows
        .into_iter()
        .map(|r| RetentionPoint {
            day_back: r.day_back,
            active_users: r.active_users,
        })
        .collect();

    Ok(Json(RetentionResult {
        cohort_size,
        horizon_days: horizon,
        points,
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

#[derive(Debug, Deserialize)]
struct OverlapQuery {
    /// Id del otro cohort a intersectar.
    other: Uuid,
}

#[derive(Debug, Serialize)]
pub struct OverlapResult {
    pub size_a: u64,
    pub size_b: u64,
    pub intersection: u64,
    /// Jaccard = |A ∩ B| / |A ∪ B|. 0.0 si ambos están vacíos.
    pub jaccard: f64,
    pub took_ms: u64,
}

async fn cohort_overlap(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    AxumQuery(q): AxumQuery<OverlapQuery>,
) -> ApiResult<Json<OverlapResult>> {
    if id == q.other {
        return Err(ApiError::BadRequest(
            "no podés calcular overlap de un cohort con sí mismo".into(),
        ));
    }
    let started = Instant::now();
    let a = load_cohort(&state, id).await?;
    let b = load_cohort(&state, q.other).await?;

    let def_a = parse_def(&a.definition)?;
    let def_b = parse_def(&b.definition)?;

    let qa = build_cohort_query(&def_a, &a.project_id, "a_")?;
    let qb = build_cohort_query(&def_b, &b.project_id, "b_")?;

    // Tres queries simples ganan a una mega-CTE: ClickHouse cachea granules y
    // las dos sub-queries de tamaño se resuelven en milisegundos cada una.
    // El JOIN del intersect se queda en memoria con el lado más pequeño.
    let mut params = qa.params();
    params.extend(qb.params());

    #[derive(Debug, Deserialize)]
    struct CountRow {
        n: u64,
    }
    let size_a_sql = format!("SELECT toUInt64(count()) AS n FROM ({sub})", sub = qa.sql);
    let size_b_sql = format!("SELECT toUInt64(count()) AS n FROM ({sub})", sub = qb.sql);
    let inter_sql = format!(
        "SELECT toUInt64(count()) AS n FROM ( \
            SELECT distinct_id FROM ({a}) \
            INTERSECT \
            SELECT distinct_id FROM ({b}) \
         )",
        a = qa.sql,
        b = qb.sql
    );

    let size_a = state
        .ch
        .select_one_with_params::<CountRow>(&size_a_sql, &qa.params())
        .await?
        .map(|r| r.n)
        .unwrap_or(0);
    let size_b = state
        .ch
        .select_one_with_params::<CountRow>(&size_b_sql, &qb.params())
        .await?
        .map(|r| r.n)
        .unwrap_or(0);
    let intersection = state
        .ch
        .select_one_with_params::<CountRow>(&inter_sql, &params)
        .await?
        .map(|r| r.n)
        .unwrap_or(0);

    let union = size_a + size_b - intersection;
    let jaccard = if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    };

    Ok(Json(OverlapResult {
        size_a,
        size_b,
        intersection,
        jaccard,
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::CohortFilter;

    #[test]
    fn op_whitelist() {
        assert_eq!(validated_op("==").unwrap(), "=");
        assert_eq!(validated_op(">=").unwrap(), ">=");
        assert_eq!(validated_op("<").unwrap(), "<");
        assert!(validated_op("; DROP TABLE faro.cohorts; --").is_err());
        assert!(validated_op("=1=1").is_err());
    }

    #[test]
    fn validate_def_bounds() {
        let bad_count = CohortDefinition {
            event: "x".into(),
            op: ">=".into(),
            count: 0,
            last_days: 1,
            filters: vec![],
            user_filters: vec![],
        };
        assert!(validate_def(&bad_count).is_err());

        let bad_days = CohortDefinition {
            event: "x".into(),
            op: ">=".into(),
            count: 1,
            last_days: 9999,
            filters: vec![],
            user_filters: vec![],
        };
        assert!(validate_def(&bad_days).is_err());

        let too_many_filters = CohortDefinition {
            event: "x".into(),
            op: ">=".into(),
            count: 1,
            last_days: 30,
            filters: (0..10)
                .map(|i| CohortFilter {
                    key: format!("k{i}"),
                    value: "v".into(),
                })
                .collect(),
            user_filters: vec![],
        };
        assert!(validate_def(&too_many_filters).is_err());

        let ok = CohortDefinition {
            event: "checkout_completed".into(),
            op: ">=".into(),
            count: 3,
            last_days: 30,
            filters: vec![CohortFilter {
                key: "plan".into(),
                value: "pro".into(),
            }],
            user_filters: vec![],
        };
        validate_def(&ok).unwrap();
    }

    #[test]
    fn validate_def_counts_event_and_user_filters_together() {
        let def = CohortDefinition {
            event: "checkout_completed".into(),
            op: ">=".into(),
            count: 1,
            last_days: 30,
            filters: vec![
                CohortFilter {
                    key: "currency".into(),
                    value: "USD".into(),
                },
                CohortFilter {
                    key: "coupon".into(),
                    value: "SPRING".into(),
                },
            ],
            user_filters: vec![
                CohortFilter {
                    key: "plan".into(),
                    value: "pro".into(),
                },
                CohortFilter {
                    key: "industry".into(),
                    value: "fintech".into(),
                },
            ],
        };

        assert!(validate_def(&def).is_err());
    }

    #[test]
    fn validate_def_rejects_empty_user_filter_key_or_value() {
        let empty_key = CohortDefinition {
            event: "checkout_completed".into(),
            op: ">=".into(),
            count: 1,
            last_days: 30,
            filters: vec![],
            user_filters: vec![CohortFilter {
                key: "".into(),
                value: "pro".into(),
            }],
        };
        assert!(validate_def(&empty_key).is_err());

        let empty_value = CohortDefinition {
            event: "checkout_completed".into(),
            op: ">=".into(),
            count: 1,
            last_days: 30,
            filters: vec![],
            user_filters: vec![CohortFilter {
                key: "plan".into(),
                value: "".into(),
            }],
        };
        assert!(validate_def(&empty_value).is_err());
    }

    #[test]
    fn build_cohort_query_shape() {
        let def = CohortDefinition {
            event: "checkout_completed".into(),
            op: ">=".into(),
            count: 3,
            last_days: 30,
            filters: vec![CohortFilter {
                key: "plan".into(),
                value: "pro".into(),
            }],
            user_filters: vec![],
        };
        let q = build_cohort_query(&def, "default", "").unwrap();
        // El SQL referencia los placeholders esperados — y NUNCA interpola
        // valores del usuario.
        assert!(q.sql.contains("{event:String}"));
        assert!(q.sql.contains("{count:UInt32}"));
        assert!(q.sql.contains("{last_days:UInt32}"));
        assert!(q.sql.contains("{project:String}"));
        assert!(q.sql.contains("HAVING count() >= {count:UInt32}"));
        assert!(q
            .sql
            .contains("JSONExtractString(properties, {fk_0:String})"));
        // El valor debe ir en owned, no embebido como literal en el SQL.
        assert!(!q.sql.contains("'pro'"));
        assert!(q.owned.iter().any(|(k, v)| k == "fv_0" && v == "pro"));
    }

    #[test]
    fn build_cohort_query_with_user_filters_shape() {
        let def = CohortDefinition {
            event: "checkout_completed".into(),
            op: ">=".into(),
            count: 3,
            last_days: 30,
            filters: vec![CohortFilter {
                key: "currency".into(),
                value: "USD".into(),
            }],
            user_filters: vec![
                CohortFilter {
                    key: "plan".into(),
                    value: "pro".into(),
                },
                CohortFilter {
                    key: "industry".into(),
                    value: "fintech".into(),
                },
            ],
        };
        let q = build_cohort_query(&def, "default", "").unwrap();

        assert!(q.sql.contains("FROM faro.product_events AS e"));
        assert!(q.sql.contains(
            "INNER JOIN (SELECT project_id, distinct_id, properties FROM faro.product_users FINAL) AS u"
        ));
        assert!(q.sql.contains("u.project_id = e.project_id"));
        assert!(q.sql.contains("u.distinct_id = e.distinct_id"));
        assert!(q
            .sql
            .contains("JSONExtractString(e.properties, {fk_0:String})"));
        assert!(q
            .sql
            .contains("JSONExtractString(u.properties, {ufk_0:String})"));
        assert!(q
            .sql
            .contains("JSONExtractString(u.properties, {ufk_1:String})"));
        assert!(q.sql.contains("GROUP BY e.distinct_id"));
        assert!(!q.sql.contains("'pro'"));
        assert!(!q.sql.contains("fintech"));
        assert!(q.owned.iter().any(|(k, v)| k == "ufv_0" && v == "pro"));
        assert!(q.owned.iter().any(|(k, v)| k == "ufv_1" && v == "fintech"));

        let prefixed = build_cohort_query(&def, "default", "a_").unwrap();
        assert!(prefixed
            .sql
            .contains("JSONExtractString(u.properties, {a_ufk_0:String})"));
        assert!(prefixed.sql.contains("{a_ufv_0:String}"));
        assert!(prefixed
            .owned
            .iter()
            .any(|(k, v)| k == "a_ufk_0" && v == "plan"));
        assert!(prefixed
            .owned
            .iter()
            .any(|(k, v)| k == "a_ufv_0" && v == "pro"));
    }
}
