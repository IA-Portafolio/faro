use axum::http::HeaderMap;
use axum::Router;

use crate::error::ApiError;
use crate::state::SharedState;

pub mod logs;
pub mod otlp;
pub mod otlp_types;

/// Endpoints HTTP compatibles con OTLP, servidos en un puerto dedicado (por defecto 4318)
/// para poder exponerlos de forma independiente de la API del dashboard.
pub fn otlp_router(state: SharedState) -> Router {
    Router::new().merge(otlp::router()).with_state(state)
}

/// Resuelve el token Bearer de las cabeceras entrantes a un slug de proyecto. Devuelve 401
/// cuando el token falta o no coincide con ningún proyecto conocido.
pub fn resolve_project(state: &SharedState, headers: &HeaderMap) -> Result<String, ApiError> {
    let token = extract_token(headers).ok_or(ApiError::Unauthorized)?;
    state.projects.lookup(&token).ok_or(ApiError::Unauthorized)
}

fn extract_token(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(rest) = v.strip_prefix("Bearer ") {
            return Some(rest.trim().to_string());
        }
    }
    if let Some(v) = headers.get("x-faro-token").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    None
}
