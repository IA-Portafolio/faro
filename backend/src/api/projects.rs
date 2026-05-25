use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use rand::distr::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::origin_check::{validate_pattern as validate_origin, OriginConfig};
use crate::redaction::{builtin_catalog, validate_custom_pattern, BuiltinInfo, RedactionConfig};
use crate::state::SharedState;
use crate::storage::ProjectRow;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/projects", get(list_projects).post(create_project))
        .route(
            "/projects/{slug}",
            get(get_project).put(update_project).delete(delete_project),
        )
        .route("/projects/{slug}/rotate", axum::routing::post(rotate_token))
        .route(
            "/projects/{slug}/redaction",
            get(get_redaction).put(put_redaction),
        )
        .route("/projects/{slug}/origins", get(get_origins).put(put_origins))
}

#[derive(Debug, Serialize)]
pub struct ProjectView {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub ingest_token: String,
    pub dsn: String,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

impl ProjectView {
    fn from_row(r: ProjectRow, public_base: &str) -> Self {
        let dsn = format!(
            "{}|{}|{}",
            public_base.trim_end_matches('/'),
            r.slug,
            r.ingest_token
        );
        Self {
            id: r.id,
            slug: r.slug,
            name: r.name,
            description: r.description,
            ingest_token: r.ingest_token,
            dsn,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const SELECT_COLS: &str = "id, slug, name, description, ingest_token, redaction_rules, \
    allowed_origins, created_at, updated_at, deleted, version";

async fn list_projects(State(state): State<SharedState>) -> ApiResult<Json<Vec<ProjectView>>> {
    let rows: Vec<ProjectRow> = state
        .ch
        .select(&format!(
            "SELECT {SELECT_COLS} FROM faro.projects FINAL WHERE deleted = 0 ORDER BY name"
        ))
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| ProjectView::from_row(r, &state.cfg.public_base_url))
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ProjectInput {
    pub name: String,
    pub slug: Option<String>,
    #[serde(default)]
    pub description: String,
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "proyecto".into()
    } else {
        trimmed
    }
}

fn random_token() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(40)
        .map(char::from)
        .collect()
}

async fn create_project(
    State(state): State<SharedState>,
    Json(input): Json<ProjectInput>,
) -> ApiResult<Json<ProjectView>> {
    let slug = input
        .slug
        .map(|s| slugify(&s))
        .unwrap_or_else(|| slugify(&input.name));
    if slug.is_empty() {
        return Err(ApiError::BadRequest("slug vacío".into()));
    }
    // Verifica unicidad del slug — ReplacingMergeTree deduplica por `id`, no por slug,
    // así que la unicidad del slug se impone en la capa de aplicación.
    let existing: Option<ProjectRow> = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.projects FINAL \
                 WHERE slug = {{slug:String}} AND deleted = 0 LIMIT 1"
            ),
            &[("slug", &slug)],
        )
        .await?;
    if existing.is_some() {
        return Err(ApiError::BadRequest(format!(
            "ya existe un proyecto con slug '{slug}'"
        )));
    }

    let now = Utc::now();
    let row = ProjectRow {
        id: Uuid::new_v4(),
        slug,
        name: input.name,
        description: input.description,
        ingest_token: random_token(),
        redaction_rules: String::new(),
        allowed_origins: String::new(),
        created_at: now,
        updated_at: now,
        deleted: 0,
        version: now.timestamp_millis() as u64,
    };
    state.ch.insert("faro.projects", &[row.clone()]).await?;
    // Refresca la caché para que el nuevo token pueda usarse de inmediato.
    let _ = state.projects.reload(&state.ch).await;
    Ok(Json(ProjectView::from_row(row, &state.cfg.public_base_url)))
}

async fn get_project(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<ProjectView>> {
    let row: ProjectRow = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.projects FINAL \
                 WHERE slug = {{slug:String}} AND deleted = 0 LIMIT 1"
            ),
            &[("slug", &slug)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(ProjectView::from_row(row, &state.cfg.public_base_url)))
}

async fn update_project(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    Json(input): Json<ProjectInput>,
) -> ApiResult<Json<ProjectView>> {
    let mut row: ProjectRow = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.projects FINAL \
                 WHERE slug = {{slug:String}} AND deleted = 0 LIMIT 1"
            ),
            &[("slug", &slug)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    row.name = input.name;
    row.description = input.description;
    // el slug es inmutable; ignora cualquier valor recibido.
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.projects", &[row.clone()]).await?;
    let _ = state.projects.reload(&state.ch).await;
    Ok(Json(ProjectView::from_row(row, &state.cfg.public_base_url)))
}

async fn delete_project(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut row: ProjectRow = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.projects FINAL WHERE slug = {{slug:String}} LIMIT 1"
            ),
            &[("slug", &slug)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    row.deleted = 1;
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.projects", &[row]).await?;
    let _ = state.projects.reload(&state.ch).await;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn rotate_token(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<ProjectView>> {
    let mut row: ProjectRow = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.projects FINAL \
                 WHERE slug = {{slug:String}} AND deleted = 0 LIMIT 1"
            ),
            &[("slug", &slug)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    row.ingest_token = random_token();
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.projects", &[row.clone()]).await?;
    let _ = state.projects.reload(&state.ch).await;
    Ok(Json(ProjectView::from_row(row, &state.cfg.public_base_url)))
}

// ---------- PII redaction config ----------

#[derive(Serialize)]
pub struct RedactionView {
    /// La config tal como está persistida.
    pub config: RedactionConfig,
    /// Catálogo estático de built-ins disponibles, para que el frontend pueda
    /// renderizar la lista sin hardcodearla.
    pub available_builtins: Vec<BuiltinInfo>,
}

async fn get_redaction(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<RedactionView>> {
    let row: ProjectRow = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.projects FINAL \
                 WHERE slug = {{slug:String}} AND deleted = 0 LIMIT 1"
            ),
            &[("slug", &slug)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    let config: RedactionConfig = if row.redaction_rules.trim().is_empty() {
        RedactionConfig::default()
    } else {
        serde_json::from_str(&row.redaction_rules).unwrap_or_default()
    };
    Ok(Json(RedactionView {
        config,
        available_builtins: builtin_catalog(),
    }))
}

async fn put_redaction(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    Json(input): Json<RedactionConfig>,
) -> ApiResult<Json<RedactionView>> {
    // Validamos cada custom rule antes de guardar — si una regla rompe el regex
    // engine la rechazamos AHORA, no en silencio al cachearla. La lista de
    // built-in slugs no necesita validación (los desconocidos se ignoran al cargar).
    for rule in &input.custom {
        if let Err(e) = validate_custom_pattern(&rule.pattern) {
            return Err(ApiError::BadRequest(format!(
                "regla '{}' inválida: {}",
                rule.name, e
            )));
        }
    }

    let mut row: ProjectRow = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.projects FINAL \
                 WHERE slug = {{slug:String}} AND deleted = 0 LIMIT 1"
            ),
            &[("slug", &slug)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    row.redaction_rules =
        serde_json::to_string(&input).map_err(|e| ApiError::Internal(e.to_string()))?;
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.projects", &[row]).await?;
    // Reload sincrónico para que el siguiente POST de ingest del SDK ya vea las
    // reglas nuevas. Sin esto, hay una ventana de hasta 15 s (el refresh tick)
    // donde el SDK loguea cosas que el dashboard pidió redactar y NO se redactan.
    let _ = state.projects.reload(&state.ch).await;
    Ok(Json(RedactionView {
        config: input,
        available_builtins: builtin_catalog(),
    }))
}

// ---------- Allowed origins (RUM SDK origin verification) ----------

#[derive(Serialize)]
pub struct OriginsView {
    pub config: OriginConfig,
}

async fn get_origins(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<OriginsView>> {
    let row: ProjectRow = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.projects FINAL \
                 WHERE slug = {{slug:String}} AND deleted = 0 LIMIT 1"
            ),
            &[("slug", &slug)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    let config: OriginConfig = if row.allowed_origins.trim().is_empty() {
        OriginConfig::default()
    } else {
        serde_json::from_str(&row.allowed_origins).unwrap_or_default()
    };
    Ok(Json(OriginsView { config }))
}

async fn put_origins(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    Json(input): Json<OriginConfig>,
) -> ApiResult<Json<OriginsView>> {
    // Validamos cada entry antes de persistir. Si activan la whitelist con una
    // entry inválida, sería peor: la regla inválida se descarta en silencio al
    // cachear y el proyecto queda con menos protección de la esperada.
    for raw in &input.origins {
        if let Err(e) = validate_origin(raw) {
            return Err(ApiError::BadRequest(format!("origen inválido: {e}")));
        }
    }
    // Si el master switch está ON pero la lista está vacía, bloqueamos al user
    // explícitamente — el efecto sería "no aceptar nada del browser", que casi
    // siempre es un error (config a medio terminar). Si realmente quieren matar
    // el RUM, que apaguen el SDK.
    if input.enabled && input.origins.iter().all(|s| s.trim().is_empty()) {
        return Err(ApiError::BadRequest(
            "lista activa pero vacía: agrega al menos un origen o desactiva la whitelist".into(),
        ));
    }

    let mut row: ProjectRow = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.projects FINAL \
                 WHERE slug = {{slug:String}} AND deleted = 0 LIMIT 1"
            ),
            &[("slug", &slug)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    row.allowed_origins =
        serde_json::to_string(&input).map_err(|e| ApiError::Internal(e.to_string()))?;
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.projects", &[row]).await?;
    // Reload sincrónico — sin esto hay hasta 15 s de gap donde el ingest todavía
    // acepta orígenes que el dashboard acaba de bloquear (o rechaza recién
    // permitidos, peor para UX).
    let _ = state.projects.reload(&state.ch).await;
    Ok(Json(OriginsView { config: input }))
}
