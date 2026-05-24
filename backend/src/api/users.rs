use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{
    hash_password, revoke_user_sessions, AuthUser, CurrentSessionTokenHash, UserRow,
};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route(
            "/users/:id",
            get(get_user).put(update_user).delete(delete_user),
        )
        .route("/users/:id/password", axum::routing::put(change_password))
}

#[derive(Serialize)]
pub struct UserView {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: String,
    pub created_at: chrono::DateTime<Utc>,
}

impl From<UserRow> for UserView {
    fn from(r: UserRow) -> Self {
        Self {
            id: r.id,
            email: r.email,
            name: r.name,
            role: r.role,
            created_at: r.created_at,
        }
    }
}

const SELECT_COLS: &str = "id, email, password_hash, name, role, \
    created_at, updated_at, deleted, version, totp_secret, totp_enabled";

async fn list_users(
    _admin: AuthUser,
    State(state): State<SharedState>,
) -> ApiResult<Json<Vec<UserView>>> {
    let rows: Vec<UserRow> = state
        .ch
        .select(&format!(
            "SELECT {SELECT_COLS} FROM faro.users FINAL WHERE deleted = 0 ORDER BY email"
        ))
        .await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

#[derive(Deserialize)]
pub struct CreateInput {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_role")]
    pub role: String,
}

fn default_role() -> String {
    "admin".into()
}

async fn create_user(
    _admin: AuthUser,
    State(state): State<SharedState>,
    Json(input): Json<CreateInput>,
) -> ApiResult<Json<UserView>> {
    let email = input.email.trim().to_lowercase();
    if email.is_empty() || input.password.len() < 8 {
        return Err(ApiError::BadRequest(
            "email obligatorio y contraseña de al menos 8 caracteres".into(),
        ));
    }
    let existing: Option<UserRow> = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.users FINAL \
                 WHERE email = {{email:String}} AND deleted = 0 LIMIT 1"
            ),
            &[("email", &email)],
        )
        .await?;
    if existing.is_some() {
        return Err(ApiError::BadRequest(format!(
            "ya existe un usuario con email {email}"
        )));
    }
    let row = UserRow {
        id: Uuid::new_v4(),
        email,
        password_hash: hash_password(&input.password)
            .map_err(|e| ApiError::Internal(e.to_string()))?,
        name: input.name,
        role: input.role,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        deleted: 0,
        version: Utc::now().timestamp_millis() as u64,
        totp_secret: String::new(),
        totp_enabled: 0,
    };
    state.ch.insert("faro.users", &[row.clone()]).await?;
    Ok(Json(row.into()))
}

async fn get_user(
    _admin: AuthUser,
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<UserView>> {
    let id_s = id.to_string();
    let row: UserRow = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.users FINAL \
                 WHERE id = {{id:UUID}} AND deleted = 0 LIMIT 1"
            ),
            &[("id", &id_s)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(row.into()))
}

#[derive(Deserialize)]
pub struct UpdateInput {
    pub name: String,
    pub role: String,
}

async fn update_user(
    _admin: AuthUser,
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateInput>,
) -> ApiResult<Json<UserView>> {
    let id_s = id.to_string();
    let mut row: UserRow = state
        .ch
        .select_one_with_params(
            &format!("SELECT {SELECT_COLS} FROM faro.users FINAL WHERE id = {{id:UUID}} LIMIT 1"),
            &[("id", &id_s)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    row.name = input.name;
    row.role = input.role;
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.users", &[row.clone()]).await?;
    Ok(Json(row.into()))
}

async fn delete_user(
    admin: AuthUser,
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    if admin.id == id {
        return Err(ApiError::BadRequest("no puedes borrarte a ti mismo".into()));
    }
    let id_s = id.to_string();
    let mut row: UserRow = state
        .ch
        .select_one_with_params(
            &format!("SELECT {SELECT_COLS} FROM faro.users FINAL WHERE id = {{id:UUID}} LIMIT 1"),
            &[("id", &id_s)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    row.deleted = 1;
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.users", &[row]).await?;
    // También revoca cualquier sesión activa de ese usuario — no deben permanecer logueados.
    let _ = state
        .ch
        .query_raw_with_params(
            "INSERT INTO faro.user_sessions \
             SELECT token_hash, user_id, user_email, user_name, user_role, created_at, expires_at, \
                    1 AS revoked, toUInt64(toUnixTimestamp64Milli(now64(3))) AS version \
             FROM faro.user_sessions FINAL WHERE user_id = {id:UUID}",
            &[("id", &id_s)],
        )
        .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct PasswordInput {
    pub password: String,
}

async fn change_password(
    admin: AuthUser,
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    current_session: CurrentSessionTokenHash,
    Json(input): Json<PasswordInput>,
) -> ApiResult<Json<serde_json::Value>> {
    if input.password.len() < 8 {
        return Err(ApiError::BadRequest(
            "contraseña de al menos 8 caracteres".into(),
        ));
    }
    // Solo el dueño o un admin pueden cambiar una contraseña. Por ahora todos son admin.
    let id_s = id.to_string();
    let mut row: UserRow = state
        .ch
        .select_one_with_params(
            &format!(
                "SELECT {SELECT_COLS} FROM faro.users FINAL \
                 WHERE id = {{id:UUID}} AND deleted = 0 LIMIT 1"
            ),
            &[("id", &id_s)],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
    row.password_hash =
        hash_password(&input.password).map_err(|e| ApiError::Internal(e.to_string()))?;
    row.updated_at = Utc::now();
    row.version = Utc::now().timestamp_millis() as u64;
    state.ch.insert("faro.users", &[row]).await?;

    // Rotación de sesiones post-password-change: si el password cambió, asumimos
    // que el motivo es que el password viejo está comprometido (o el user lo cree
    // así). Cualquier sesión emitida con el password viejo debe morir.
    //
    // Si quien cambia el password es el PROPIO user (caso típico desde
    // /settings/security), preservamos la sesión actual para no echarlo de su
    // propio browser. Si un admin lo cambia para OTRO user, no hay sesión "actual"
    // del target que preservar — revoca todas.
    let keep = if admin.id == id {
        current_session.0.as_str()
    } else {
        ""
    };
    let _ = revoke_user_sessions(&state, id, keep).await;

    Ok(Json(serde_json::json!({"ok": true})))
}
