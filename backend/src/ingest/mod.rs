use axum::http::HeaderMap;
use axum::Router;

use crate::error::ApiError;
use crate::state::SharedState;

pub mod logs;
pub mod otlp;
pub mod otlp_types;

/// OTLP-compatible HTTP endpoints, served on a dedicated port (defaults to 4318)
/// so they can be exposed independently from the dashboard API.
pub fn otlp_router(state: SharedState) -> Router {
    Router::new().merge(otlp::router()).with_state(state)
}

/// Resolve the bearer token in incoming headers to a project slug. Returns 401
/// when the token is missing or doesn't match any known project.
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
