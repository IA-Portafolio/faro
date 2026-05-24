//! Preferencias de UI por usuario (tema y defaults de exploración). Una sola
//! fila por usuario en `faro.user_preferences`. El backend solo guarda los
//! valores; aplicarlos es responsabilidad del frontend.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new().route("/me/preferences", get(get_prefs).put(update_prefs))
}

/// Temas válidos. `system` deja que el frontend siga `prefers-color-scheme`.
const VALID_THEMES: &[&str] = &["light", "dark", "system"];

/// Presets de rango temporal aceptados — espejo de los del frontend.
/// `''` significa "no aplicar default, usar `1h`".
const VALID_RANGES: &[&str] = &["", "5m", "15m", "1h", "6h", "24h", "7d"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreferencesRow {
    pub user_id: Uuid,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub default_project: String,
    #[serde(default = "default_range")]
    pub default_time_range: String,
    #[serde(
        serialize_with = "ser_dt_ms",
        deserialize_with = "crate::storage::models::de_dt",
        default = "Utc::now"
    )]
    pub updated_at: DateTime<Utc>,
    #[serde(default = "default_version")]
    pub version: u64,
}

fn default_theme() -> String {
    "system".into()
}
fn default_range() -> String {
    "1h".into()
}
fn default_version() -> u64 {
    1
}

fn ser_dt_ms<S: serde::Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

#[derive(Serialize)]
pub struct PreferencesView {
    pub theme: String,
    pub default_project: String,
    pub default_time_range: String,
    pub updated_at: DateTime<Utc>,
}

impl From<PreferencesRow> for PreferencesView {
    fn from(r: PreferencesRow) -> Self {
        Self {
            theme: r.theme,
            default_project: r.default_project,
            default_time_range: r.default_time_range,
            updated_at: r.updated_at,
        }
    }
}

/// Cada campo es opcional para permitir actualizaciones parciales (PATCH-style).
/// `None` significa "no tocar"; `Some` significa "establecer este valor".
#[derive(Deserialize)]
pub struct UpdateInput {
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub default_project: Option<String>,
    #[serde(default)]
    pub default_time_range: Option<String>,
}

async fn load_row(state: &SharedState, user_id: Uuid) -> ApiResult<PreferencesRow> {
    let id_s = user_id.to_string();
    let row: Option<PreferencesRow> = state
        .ch
        .select_one_with_params(
            "SELECT user_id, theme, default_project, default_time_range, updated_at, version \
             FROM faro.user_preferences FINAL WHERE user_id = {id:UUID} LIMIT 1",
            &[("id", &id_s)],
        )
        .await?;
    Ok(row.unwrap_or_else(|| PreferencesRow {
        user_id,
        theme: default_theme(),
        default_project: String::new(),
        default_time_range: default_range(),
        updated_at: Utc::now(),
        version: 1,
    }))
}

async fn get_prefs(
    user: AuthUser,
    State(state): State<SharedState>,
) -> ApiResult<Json<PreferencesView>> {
    let row = load_row(&state, user.id).await?;
    Ok(Json(row.into()))
}

async fn update_prefs(
    user: AuthUser,
    State(state): State<SharedState>,
    Json(input): Json<UpdateInput>,
) -> ApiResult<Json<PreferencesView>> {
    let mut row = load_row(&state, user.id).await?;

    if let Some(t) = input.theme {
        let theme = t.trim().to_lowercase();
        if !VALID_THEMES.contains(&theme.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "theme inválido: {theme}. Valores aceptados: light, dark, system"
            )));
        }
        row.theme = theme;
    }

    if let Some(p) = input.default_project {
        // El slug en sí ya lo valida `projects` cuando se crea — aquí solo
        // recortamos espacios y aceptamos vacío como "sin default".
        row.default_project = p.trim().to_string();
    }

    if let Some(r) = input.default_time_range {
        let range = r.trim().to_string();
        if !VALID_RANGES.contains(&range.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "default_time_range inválido: {range}. Valores aceptados: 5m, 15m, 1h, 6h, 24h, 7d"
            )));
        }
        // Normaliza '' → '1h' para no guardar valor sin sentido.
        row.default_time_range = if range.is_empty() {
            default_range()
        } else {
            range
        };
    }

    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state
        .ch
        .insert("faro.user_preferences", &[row.clone()])
        .await?;
    Ok(Json(row.into()))
}
