//! Tipo de error de la API y su conversión a respuestas HTTP.
//!
//! `ApiError` enumera los modos de fallo (no autorizado, prohibido, petición
//! inválida, no encontrado, error de ClickHouse, rate limit…) y los mapea a su
//! código de estado y body JSON. `ApiResult<T>` es el alias que devuelven los
//! handlers.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("no autorizado")]
    Unauthorized,
    /// 403. Las credenciales fueron aceptadas pero la acción está bloqueada por
    /// política (e.g. origen del browser fuera de la whitelist del proyecto).
    #[error("prohibido: {0}")]
    Forbidden(String),
    #[error("petición inválida: {0}")]
    BadRequest(String),
    #[error("no encontrado")]
    NotFound,
    #[error("clickhouse: {0}")]
    Clickhouse(String),
    #[error("interno: {0}")]
    Internal(String),
    /// Rate limit por proyecto en la ingesta. `retry_after_secs` se devuelve
    /// como header `Retry-After` además de en el body, para clientes que sólo
    /// miran headers HTTP estándar.
    #[error("rate limit por proyecto excedido (reintenta en {retry_after_secs}s)")]
    TooManyRequests { retry_after_secs: u64 },
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        ApiError::Clickhouse(e.to_string())
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::BadRequest(format!("JSON inválido: {e}"))
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let ApiError::TooManyRequests { retry_after_secs } = self {
            let body = Json(json!({
                "error": "rate_limited",
                "message": format!(
                    "rate limit por proyecto excedido (reintenta en {retry_after_secs}s)"
                ),
                "retry_after_secs": retry_after_secs,
            }));
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(
                    axum::http::header::RETRY_AFTER,
                    retry_after_secs.to_string(),
                )],
                body,
            )
                .into_response();
        }
        let (status, code) = match &self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::Clickhouse(_) => (StatusCode::BAD_GATEWAY, "clickhouse_error"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
            ApiError::TooManyRequests { .. } => unreachable!("manejado arriba"),
        };
        (
            status,
            Json(json!({ "error": code, "message": self.to_string() })),
        )
            .into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
