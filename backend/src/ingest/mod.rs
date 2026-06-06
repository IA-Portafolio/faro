//! Capa de ingesta: recepción de la telemetría entrante de los SDKs.
//!
//! Agrupa los receptores nativos (logs, metrics, spans, events, replay) y los
//! compatibles con OTLP (HTTP/JSON y gRPC), servidos en un puerto dedicado
//! (`:4318`). Aquí viven también las utilidades comunes: resolver el proyecto a
//! partir del token Bearer (`resolve_project`) y validar el header `Origin` contra
//! la whitelist del proyecto (`check_origin`). Los endpoints nativos aceptan además
//! el token vía query `?_token=` (`resolve_project_with_query`) como fallback para
//! `navigator.sendBeacon`, que no puede setear headers al cerrar la pestaña.

use axum::http::HeaderMap;
use axum::Router;

use crate::error::ApiError;
use crate::redaction::CompiledRules;
use crate::state::SharedState;
use crate::storage::{LogRow, SpanRow};

pub mod events;
pub mod logs;
pub mod metrics;
pub mod otlp;
pub mod otlp_grpc;
pub mod otlp_types;
pub mod rate_limit;
pub mod replay;
pub mod spans;

/// Endpoints HTTP compatibles con OTLP, servidos en un puerto dedicado (por defecto 4318)
/// para poder exponerlos de forma independiente de la API del dashboard.
pub fn otlp_router(state: SharedState) -> Router {
    Router::new().merge(otlp::router()).with_state(state)
}

/// Query string aceptado por los endpoints de ingesta nativos para autenticar vía
/// `?_token=` cuando el transporte no puede setear headers. Lo usa
/// `navigator.sendBeacon` del SDK browser al cerrar la pestaña: el beacon no permite
/// `Authorization`, así que el token viaja en la URL. El token de ingesta del browser
/// es público —va en el bundle y la defensa real es `check_origin`—, por lo que
/// aceptarlo por query es aceptable. El header siempre tiene prioridad.
#[derive(Debug, Default, serde::Deserialize)]
pub struct IngestQuery {
    #[serde(default, rename = "_token")]
    pub token: Option<String>,
}

/// Resuelve el token Bearer de las cabeceras entrantes a un slug de proyecto. Devuelve 401
/// cuando el token falta o no coincide con ningún proyecto conocido.
pub fn resolve_project(state: &SharedState, headers: &HeaderMap) -> Result<String, ApiError> {
    resolve_project_with_query(state, headers, None)
}

/// Igual que [`resolve_project`] pero acepta además el token vía query param `_token`
/// (ver [`IngestQuery`]) como fallback para `navigator.sendBeacon`. El header tiene
/// prioridad; el query solo se usa si no hay token en cabeceras.
pub fn resolve_project_with_query(
    state: &SharedState,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<String, ApiError> {
    let token = select_ingest_token(headers, query_token).ok_or(ApiError::Unauthorized)?;
    state.projects.lookup(&token).ok_or(ApiError::Unauthorized)
}

/// Elige el token de ingesta: primero el header (`Authorization: Bearer` o
/// `x-faro-token`), y si no hay, el query `_token` (fallback para `sendBeacon`).
/// Normaliza espacios y descarta valores vacíos.
fn select_ingest_token(headers: &HeaderMap, query_token: Option<&str>) -> Option<String> {
    extract_token(headers).or_else(|| {
        query_token
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::{HeaderValue, AUTHORIZATION};

    fn headers_with_bearer(token: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        h
    }

    #[test]
    fn extract_token_reads_bearer_and_x_faro_token() {
        assert_eq!(
            extract_token(&headers_with_bearer("abc")).as_deref(),
            Some("abc")
        );
        let mut h = HeaderMap::new();
        h.insert("x-faro-token", HeaderValue::from_static("xyz"));
        assert_eq!(extract_token(&h).as_deref(), Some("xyz"));
        assert_eq!(extract_token(&HeaderMap::new()), None);
    }

    #[test]
    fn select_token_prefers_header_over_query() {
        let h = headers_with_bearer("from-header");
        assert_eq!(
            select_ingest_token(&h, Some("from-query")).as_deref(),
            Some("from-header")
        );
    }

    #[test]
    fn select_token_falls_back_to_query_for_beacon() {
        let h = HeaderMap::new();
        assert_eq!(
            select_ingest_token(&h, Some("beacon-token")).as_deref(),
            Some("beacon-token")
        );
    }

    #[test]
    fn select_token_ignores_empty_or_whitespace_query() {
        let h = HeaderMap::new();
        assert_eq!(select_ingest_token(&h, Some("")), None);
        assert_eq!(select_ingest_token(&h, Some("   ")), None);
        assert_eq!(select_ingest_token(&h, None), None);
    }

    #[test]
    fn ingest_query_maps_underscore_token_field() {
        // El parseo real de query lo hace el extractor `Query` de axum; acá solo
        // verificamos el mapeo del campo (`rename = "_token"`) y el default a None.
        let q: IngestQuery = serde_json::from_str(r#"{"_token":"hex123"}"#).unwrap();
        assert_eq!(q.token.as_deref(), Some("hex123"));
        let empty: IngestQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.token, None);
    }
}
