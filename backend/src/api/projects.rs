use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::storage::ProjectRow;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/:slug", get(get_project).put(update_project).delete(delete_project))
        .route("/projects/:slug/rotate", axum::routing::post(rotate_token))
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
        let dsn = format!("{}|{}|{}", public_base.trim_end_matches('/'), r.slug, r.ingest_token);
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

const SELECT_COLS: &str = "id, slug, name, description, ingest_token, \
    created_at, updated_at, deleted, version";

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
    if trimmed.is_empty() { "proyecto".into() } else { trimmed }
}

fn random_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(40)
        .map(char::from)
        .collect()
}

async fn create_project(
    State(state): State<SharedState>,
    Json(input): Json<ProjectInput>,
) -> ApiResult<Json<ProjectView>> {
    let slug = input.slug.map(|s| slugify(&s)).unwrap_or_else(|| slugify(&input.name));
    if slug.is_empty() {
        return Err(ApiError::BadRequest("slug vacío".into()));
    }
    // Verifica unicidad del slug — ReplacingMergeTree deduplica por `id`, no por slug,
    // así que la unicidad del slug se impone en la capa de aplicación.
    let existing: Option<ProjectRow> = state
        .ch
        .select_one(&format!(
            "SELECT {SELECT_COLS} FROM faro.projects FINAL WHERE slug = '{}' AND deleted = 0 LIMIT 1",
            slug.replace('\'', "''")
        ))
        .await?;
    if existing.is_some() {
        return Err(ApiError::BadRequest(format!("ya existe un proyecto con slug '{slug}'")));
    }

    let now = Utc::now();
    let row = ProjectRow {
        id: Uuid::new_v4(),
        slug,
        name: input.name,
        description: input.description,
        ingest_token: random_token(),
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
    let escaped = slug.replace('\'', "''");
    let row: ProjectRow = state
        .ch
        .select_one(&format!(
            "SELECT {SELECT_COLS} FROM faro.projects FINAL WHERE slug = '{escaped}' AND deleted = 0 LIMIT 1"
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(ProjectView::from_row(row, &state.cfg.public_base_url)))
}

async fn update_project(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    Json(input): Json<ProjectInput>,
) -> ApiResult<Json<ProjectView>> {
    let escaped = slug.replace('\'', "''");
    let mut row: ProjectRow = state
        .ch
        .select_one(&format!(
            "SELECT {SELECT_COLS} FROM faro.projects FINAL WHERE slug = '{escaped}' AND deleted = 0 LIMIT 1"
        ))
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
    let escaped = slug.replace('\'', "''");
    let mut row: ProjectRow = state
        .ch
        .select_one(&format!(
            "SELECT {SELECT_COLS} FROM faro.projects FINAL WHERE slug = '{escaped}' LIMIT 1"
        ))
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
    let escaped = slug.replace('\'', "''");
    let mut row: ProjectRow = state
        .ch
        .select_one(&format!(
            "SELECT {SELECT_COLS} FROM faro.projects FINAL WHERE slug = '{escaped}' AND deleted = 0 LIMIT 1"
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    row.ingest_token = random_token();
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.projects", &[row.clone()]).await?;
    let _ = state.projects.reload(&state.ch).await;
    Ok(Json(ProjectView::from_row(row, &state.cfg.public_base_url)))
}
