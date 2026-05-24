use axum::http::HeaderMap;
use axum::Router;

use crate::error::ApiError;
use crate::redaction::CompiledRules;
use crate::state::SharedState;
use crate::storage::{LogRow, SpanRow};

pub mod events;
pub mod logs;
pub mod otlp;
pub mod otlp_grpc;
pub mod otlp_types;
pub mod rate_limit;
pub mod replay;

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

/// Valida el header `Origin` contra la whitelist del proyecto.
///
/// - Si el proyecto NO tiene whitelist activa → siempre OK (fail-open por compat).
/// - Si el request NO trae `Origin` → OK (cliente server-side; el bearer alcanza).
/// - Si trae `Origin` y NO matchea → `Err(Forbidden)`. Esto cubre el RUM SDK
///   ejecutándose en un dominio no autorizado que copió el token público del bundle.
///
/// Nota: el header `Origin` es controlado por el browser y NO es asignable desde
/// JS, así que un sitio atacante no puede falsificarlo. Esa es exactamente la
/// propiedad que hace que esta validación tenga valor.
pub fn check_origin(
    state: &SharedState,
    project: &str,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let Some(rules) = state.projects.origins(project) else {
        return Ok(());
    };
    let Some(origin) = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
    else {
        // Sin Origin = no es un browser. Los chequeos de bearer ya pasaron.
        return Ok(());
    };
    if rules.matches(origin) {
        return Ok(());
    }
    tracing::warn!(project, %origin, "ingest rechazado por Origin no permitido");
    Err(ApiError::Forbidden(format!(
        "origen no permitido para el proyecto '{project}': {origin}"
    )))
}

fn extract_token(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(rest) = v.strip_prefix("Bearer ") {
            return Some(rest.trim().to_string());
        }
    }
    if let Some(v) = headers.get("x-faro-token").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    None
}

/// Aplica las reglas de redacción del proyecto a los campos de texto-libre y a los
/// valores de atributos de un `LogRow`. Es un no-op si el proyecto no tiene
/// redacción activa (path 100% sin asignación gracias al Cow del módulo).
///
/// Mantenerlo en `ingest::` (no en `redaction::`) evita que el módulo de redacción
/// dependa de los row types de storage — `redaction` queda como una utility pura
/// de texto + AttrMap.
pub fn redact_log(rules: Option<&CompiledRules>, row: &mut LogRow) {
    let Some(r) = rules else { return };
    r.apply_in_place(&mut row.body);
    r.apply_to_attrs(&mut row.attributes);
    r.apply_to_attrs(&mut row.resource_attributes);
}

/// Span: el `name` lo dejamos sin redactar — es un identificador estructural
/// (endpoint, función, query name) que el dashboard usa para agrupar. Si lo
/// redactamos, el grouping se rompe silenciosamente. Sí redactamos los atributos,
/// `status_message` y los events_attributes (que llegan ya JSON-serializados).
pub fn redact_span(rules: Option<&CompiledRules>, row: &mut SpanRow) {
    let Some(r) = rules else { return };
    r.apply_in_place(&mut row.status_message);
    r.apply_to_attrs(&mut row.span_attributes);
    r.apply_to_attrs(&mut row.resource_attributes);
    for e in &mut row.events_attributes {
        r.apply_in_place(e);
    }
}
