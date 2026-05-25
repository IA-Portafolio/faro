//! Ingest endpoint para product events (6º pilar).
//!
//! Espejo conceptual de `ingest::logs`, pero hablando `faro.product_events`. Los
//! SDKs envían eventos `track` / `identify` / `page` / `screen` / `alias` aquí;
//! cada `type` se traduce a un nombre canónico estilo PostHog (`$identify`,
//! `$pageview`, `$screen`, `$alias`) y los campos relevantes (`distinct_id`,
//! `anonymous_id`, `user_properties`) se rellenan según el tipo. Eso significa
//! que la tabla guarda eventos custom (track) y eventos especiales con la misma
//! shape — el consumidor filtra por `event_name` cuando necesita uno u otro.

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::observability::names;
use crate::state::SharedState;
use crate::storage::{AttrMap, ProductEventRow, ProductUserAliasRow, ProductUserRow};

const MAX_EVENT_NAME_LEN: usize = 64;
const MAX_PROPERTIES_BYTES: usize = 16 * 1024;
// Cota dura al tamaño del batch para evitar que un cliente patológico (o un bug
// del SDK con back-pressure mal calibrada) mande payloads de cientos de miles
// de eventos en un solo request. El handler los procesa todos en memoria antes
// de enviarlos al canal del writer; 100 sigue siendo cómodo para los SDKs
// reales (los defaults de @iaportafolio/nextjs y /node usan 50-100 por flush).
const MAX_BATCH_EVENTS: usize = 100;

pub fn router() -> Router<SharedState> {
    Router::new().route("/events", post(ingest_events))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct IngestPayload {
    service: Option<String>,
    /// Nuevo contrato público.
    #[serde(default)]
    batch: Option<Vec<RawEvent>>,
    /// Alias legacy usado por los SDKs existentes.
    #[serde(default)]
    events: Option<Vec<RawEvent>>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RawEvent {
    /// `track` | `identify` | `page` | `screen` | `alias` (o un nombre custom).
    /// Si falta, asumimos `track` para minimizar fricción.
    #[serde(default = "default_type")]
    r#type: String,
    /// Nombre del evento para `track`/`screen`; para `page` es el path; para
    /// `identify`/`alias` se ignora (los IDs viven en sus propios campos).
    #[serde(default)]
    name: String,
    /// Nombre canónico del contrato nuevo. `name` queda como alias legacy.
    #[serde(default)]
    event: String,
    #[serde(default)]
    timestamp: Option<DateTime<Utc>>,
    /// ID estable del usuario. Para `identify` y `alias` es el NUEVO id.
    #[serde(default)]
    distinct_id: String,
    /// ID anónimo previo al login. Para `alias` es el id PREVIO que se fusiona
    /// con `distinct_id`.
    #[serde(default)]
    anonymous_id: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    properties: Option<Value>,
    /// Solo `identify`: traits del usuario. Si el SDK manda esto en `track` se
    /// preserva, pero el flujo normal es identify.
    #[serde(default)]
    user_properties: Option<Value>,
    #[serde(default)]
    context: Option<Value>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    event_id: Option<String>,
}

fn default_type() -> String {
    "track".into()
}

impl IngestPayload {
    fn events_batch(&self) -> Result<&[RawEvent], ApiError> {
        if let Some(batch) = &self.batch {
            return Ok(batch.as_slice());
        }
        if let Some(events) = &self.events {
            return Ok(events.as_slice());
        }
        Err(ApiError::BadRequest(
            "payload debe incluir 'batch' (o 'events' legacy)".into(),
        ))
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/ingest/events",
    tag = "events",
    request_body = IngestPayload,
    responses(
        (status = 200, description = "Eventos aceptados", body = serde_json::Value),
        (status = 400, description = "Payload inválido"),
        (status = 401, description = "Token bearer inválido o ausente"),
        (status = 429, description = "Rate limit excedido")
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn ingest_events(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(payload): Json<IngestPayload>,
) -> ApiResult<Json<serde_json::Value>> {
    let project = super::resolve_project(&state, &headers)?;
    super::check_origin(&state, &project, &headers)?;

    // Mismo bucket que el resto de ingesta — `signal = "events"` lo distingue
    // en métricas. Si un proyecto satura, se rate-limita igual que logs/traces.
    let events = payload.events_batch()?;
    if events.len() > MAX_BATCH_EVENTS {
        return Err(ApiError::BadRequest(format!(
            "batch demasiado grande: {} eventos (máximo {MAX_BATCH_EVENTS} por request)",
            events.len()
        )));
    }
    let n: u32 = events.len().try_into().unwrap_or(u32::MAX);
    match state.limiter.check(&project, n) {
        super::rate_limit::LimitOutcome::Allowed => {}
        other => {
            let secs = other.retry_after_secs();
            tracing::warn!(
                project,
                records = n,
                retry_after_secs = secs,
                "ingest /events rate-limited"
            );
            metrics::counter!(
                names::RATE_LIMITED,
                "project" => project.clone(),
                "signal" => "events",
            )
            .increment(1);
            metrics::counter!(
                names::INGEST_RECORDS,
                "project" => project.clone(),
                "signal" => "events",
                "outcome" => "rate_limited",
            )
            .increment(n as u64);
            return Err(ApiError::TooManyRequests {
                retry_after_secs: secs,
            });
        }
    }

    let now = Utc::now();
    let svc_default = payload.service.clone().unwrap_or_else(|| "unknown".into());
    let redaction_rules = state.projects.redaction(&project);
    let mut rows = Vec::with_capacity(events.len());
    let mut alias_rows = Vec::new();
    let mut user_rows = Vec::new();

    for raw in events {
        let mut row = build_row(&project, &svc_default, raw, now, now)?;
        redact_event(redaction_rules.as_ref(), &mut row);
        if let Some((alias, user)) = alias_identity_rows(&row) {
            alias_rows.push(alias);
            user_rows.push(user);
        } else if let Some(user) = identify_upsert_row(&row) {
            user_rows.push(user);
        }
        rows.push(row);
    }

    upsert_alias_identity(&state, &alias_rows, &user_rows).await;

    let mut accepted = 0u64;
    for row in rows {
        let _ = state.live_bus.events.send(row.clone());
        if state.ingest.events_tx.try_send(row).is_ok() {
            accepted += 1;
        } else {
            tracing::warn!("event ingest channel full, dropping record");
        }
    }

    if accepted > 0 {
        metrics::counter!(
            names::INGEST_RECORDS,
            "project" => project.clone(),
            "signal" => "events",
            "outcome" => "accepted",
        )
        .increment(accepted);
    }

    Ok(Json(
        serde_json::json!({ "accepted": accepted, "project": project }),
    ))
}

fn build_row(
    project: &str,
    service_default: &str,
    raw: &RawEvent,
    ts_default: DateTime<Utc>,
    _observed: DateTime<Utc>,
) -> Result<ProductEventRow, ApiError> {
    let ts = raw.timestamp.unwrap_or(ts_default);
    let event_input = raw.event_name_input();
    let (event_name, distinct_id, anonymous_id, props_json) = normalize(
        &raw.r#type,
        event_input,
        &raw.distinct_id,
        &raw.anonymous_id,
        raw.properties.as_ref(),
    )?;

    Ok(ProductEventRow {
        timestamp: ts,
        project_id: project.to_string(),
        event_name,
        distinct_id,
        anonymous_id,
        session_id: raw.session_id.clone(),
        properties: props_json,
        user_properties: json_to_string(raw.user_properties.as_ref()),
        context: json_to_string(raw.context.as_ref()),
        source: raw
            .source
            .clone()
            .unwrap_or_else(|| default_source(service_default)),
        trace_id: raw.trace_id.clone().unwrap_or_default(),
        span_id: raw.span_id.clone().unwrap_or_default(),
        // event_id va por hilo como string-UUID porque el storage lo serializa así.
        // La tabla tiene DEFAULT generateUUIDv4(), pero JSONEachRow no aplica
        // DEFAULT con string vacío (falla al parsear como UUID), así que lo
        // generamos aquí cuando el SDK no manda uno.
        event_id: raw
            .event_id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok().map(|u| u.to_string()))
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
    })
}

impl RawEvent {
    fn event_name_input(&self) -> &str {
        if !self.event.is_empty() {
            self.event.as_str()
        } else {
            self.name.as_str()
        }
    }
}

/// Mapea `(type, name, ids, props)` a los campos canónicos de la tabla.
///
/// Convención estilo PostHog/Segment: los eventos especiales viajan con
/// nombres prefijados con `$` para que un dashboard pueda distinguirlos sin
/// tener una columna `type` adicional. `track` mantiene el nombre tal cual
/// (el usuario eligió `checkout_completed` y eso es lo que persistimos).
///
/// `properties` se serializa a JSON aquí (la tabla la guarda como String).
fn normalize(
    ty: &str,
    name: &str,
    distinct: &str,
    anon: &str,
    props: Option<&Value>,
) -> Result<(String, String, String, String), ApiError> {
    let normalized = match ty {
        "identify" => (
            "$identify".to_string(),
            distinct.to_string(),
            anon.to_string(),
            json_to_string(props),
        ),
        "page" => {
            // El path llega como `name`; si el SDK también puso properties, lo
            // mergeamos preservando lo que el usuario mandó explícito.
            let merged = merge_with_kv(props, "path", name);
            (
                "$pageview".to_string(),
                distinct.to_string(),
                anon.to_string(),
                merged,
            )
        }
        "screen" => {
            let merged = merge_with_kv(props, "name", name);
            (
                "$screen".to_string(),
                distinct.to_string(),
                anon.to_string(),
                merged,
            )
        }
        "alias" => (
            "$alias".to_string(),
            // distinct_id = nuevo id estable; anonymous_id = el id previo que
            // pre-login se mergea con éste.
            distinct.to_string(),
            anon.to_string(),
            alias_properties(props, anon, distinct),
        ),
        // `track` y cualquier otro `type` no reconocido → tratar como track.
        // Esto deja la puerta abierta a tipos futuros sin que un SDK viejo
        // rompa por leer un type que no conoce.
        _ => (
            name.to_string(),
            distinct.to_string(),
            anon.to_string(),
            json_to_string(props),
        ),
    };
    validate_event_name(&normalized.0)?;
    validate_properties_size(&normalized.3)?;
    Ok(normalized)
}

fn alias_properties(props: Option<&Value>, from: &str, to: &str) -> String {
    let mut map = match props {
        Some(Value::Object(m)) => m.clone(),
        _ => serde_json::Map::new(),
    };
    if !from.is_empty() {
        map.entry("from".to_string())
            .or_insert_with(|| Value::String(from.to_string()));
    }
    if !to.is_empty() {
        map.entry("to".to_string())
            .or_insert_with(|| Value::String(to.to_string()));
    }
    if map.is_empty() {
        String::new()
    } else {
        Value::Object(map).to_string()
    }
}

fn alias_identity_rows(row: &ProductEventRow) -> Option<(ProductUserAliasRow, ProductUserRow)> {
    if row.event_name != "$alias" || row.anonymous_id.is_empty() || row.distinct_id.is_empty() {
        return None;
    }
    Some((
        ProductUserAliasRow {
            project_id: row.project_id.clone(),
            anonymous_id: row.anonymous_id.clone(),
            distinct_id: row.distinct_id.clone(),
            linked_at: row.timestamp,
        },
        ProductUserRow {
            project_id: row.project_id.clone(),
            distinct_id: row.distinct_id.clone(),
            first_seen: row.timestamp,
            last_seen: row.timestamp,
            anonymous_ids: vec![row.anonymous_id.clone()],
            sources: if row.source.is_empty() {
                Vec::new()
            } else {
                vec![row.source.clone()]
            },
            event_count: 1,
            properties: row.user_properties.clone(),
        },
    ))
}

/// Fast-path para `$identify`: además del evento, hacemos un best-effort upsert
/// inmediato a `faro.product_users` para que el dashboard vea al usuario sin
/// esperar el próximo tick del worker `user_unifier` (que corre cada 60s por
/// defecto). El worker después corrige discrepancias (anonymous_ids unión,
/// event_count real) — esto es solo para que el row exista YA con
/// last_seen/properties frescas. ReplacingMergeTree(last_seen) garantiza que
/// el worker pueda re-insertar con un last_seen mayor sin conflicto.
fn identify_upsert_row(row: &ProductEventRow) -> Option<ProductUserRow> {
    if row.event_name != "$identify" || row.distinct_id.is_empty() {
        return None;
    }
    Some(ProductUserRow {
        project_id: row.project_id.clone(),
        distinct_id: row.distinct_id.clone(),
        first_seen: row.timestamp,
        last_seen: row.timestamp,
        anonymous_ids: if row.anonymous_id.is_empty() {
            Vec::new()
        } else {
            vec![row.anonymous_id.clone()]
        },
        sources: if row.source.is_empty() {
            Vec::new()
        } else {
            vec![row.source.clone()]
        },
        // El worker calcula el event_count real; 0 acá indica "no autoritativo".
        event_count: 0,
        properties: row.user_properties.clone(),
    })
}

async fn upsert_alias_identity(
    state: &SharedState,
    aliases: &[ProductUserAliasRow],
    users: &[ProductUserRow],
) {
    if aliases.is_empty() && users.is_empty() {
        return;
    }
    if !aliases.is_empty() {
        if let Err(e) = state.ch.insert("faro.product_user_aliases", aliases).await {
            tracing::warn!(error = %e, "best-effort alias identity upsert failed");
        }
    }
    if !users.is_empty() {
        if let Err(e) = state.ch.insert("faro.product_users", users).await {
            tracing::warn!(error = %e, "best-effort product user upsert failed");
        }
    }
}

fn validate_event_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty() || name.len() > MAX_EVENT_NAME_LEN {
        return Err(ApiError::BadRequest(format!(
            "event_name inválido: debe tener 1..={MAX_EVENT_NAME_LEN} chars"
        )));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'$'))
    {
        return Err(ApiError::BadRequest(
            "event_name inválido: usa solo letras, números, '_', '-', '.', '$'".into(),
        ));
    }
    Ok(())
}

fn validate_properties_size(properties: &str) -> Result<(), ApiError> {
    if properties.len() > MAX_PROPERTIES_BYTES {
        return Err(ApiError::BadRequest(format!(
            "properties excede el máximo de {MAX_PROPERTIES_BYTES} bytes"
        )));
    }
    Ok(())
}

fn json_to_string(v: Option<&Value>) -> String {
    match v {
        Some(value) if !value.is_null() => value.to_string(),
        _ => String::new(),
    }
}

/// Combina `properties` (objeto) con `key=value` adicional, sin pisar la clave
/// si el usuario ya la puso explícitamente. Si `properties` no es un objeto
/// (null, string, etc.) lo ignoramos y devolvemos `{key:value}`.
fn merge_with_kv(props: Option<&Value>, key: &str, value: &str) -> String {
    let mut map = match props {
        Some(Value::Object(m)) => m.clone(),
        _ => serde_json::Map::new(),
    };
    if !value.is_empty() {
        map.entry(key.to_string())
            .or_insert_with(|| Value::String(value.to_string()));
    }
    if map.is_empty() {
        String::new()
    } else {
        Value::Object(map).to_string()
    }
}

/// Inferencia barata de `source` cuando el SDK no lo manda: el nombre del
/// servicio suele dar pistas (`mi-app-web`, `mi-app-android`). Para los SDKs
/// nuevos la convención explícita es: web|mobile|backend. El default
/// `web` es el que cubre la mayoría de casos (RUM, Next.js client).
fn default_source(service: &str) -> String {
    let s = service.to_lowercase();
    if s.contains("android")
        || s.contains("ios")
        || s.contains("flutter")
        || s.contains("mobile")
        || s.contains("expo")
    {
        "mobile".to_string()
    } else if s.contains("backend")
        || s.contains("server")
        || s.contains("api")
        || s.contains("worker")
    {
        "backend".to_string()
    } else {
        "web".to_string()
    }
}

/// Aplica las reglas de redacción del proyecto a un product event. A
/// diferencia de los logs, `properties` / `user_properties` / `context` ya
/// viajan como JSON serializado en una sola columna String — la redacción se
/// aplica al texto completo. Eso es defensa en profundidad: el regex de PII
/// matchea valores que aparezcan dentro del JSON aunque la estructura sea
/// libre.
fn redact_event(rules: Option<&crate::redaction::CompiledRules>, row: &mut ProductEventRow) {
    let Some(r) = rules else { return };
    let mut attrs = AttrMap::new();
    attrs.insert("properties".into(), row.properties.clone());
    attrs.insert("user_properties".into(), row.user_properties.clone());
    attrs.insert("context".into(), row.context.clone());
    r.apply_to_attrs(&mut attrs);
    if let Some(v) = attrs.remove("properties") {
        row.properties = v;
    }
    if let Some(v) = attrs.remove("user_properties") {
        row.user_properties = v;
    }
    if let Some(v) = attrs.remove("context") {
        row.context = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_track(event: &str, properties: Value) -> RawEvent {
        RawEvent {
            r#type: "track".to_string(),
            name: String::new(),
            event: event.to_string(),
            timestamp: None,
            distinct_id: "user_42".to_string(),
            anonymous_id: "anon_abc".to_string(),
            session_id: "ses_xyz".to_string(),
            properties: Some(properties),
            user_properties: None,
            context: None,
            source: Some("web".to_string()),
            trace_id: Some("abc".to_string()),
            span_id: None,
            event_id: None,
        }
    }

    #[test]
    fn payload_accepts_batch_with_event_field() {
        let payload: IngestPayload = serde_json::from_value(serde_json::json!({
            "batch": [
                {
                    "type": "track",
                    "event": "checkout_completed",
                    "distinct_id": "user_42",
                    "anonymous_id": "anon_abc",
                    "properties": { "amount": 99.5 }
                }
            ]
        }))
        .unwrap();

        let events = payload.events_batch().unwrap();
        let row = build_row(
            "default",
            "checkout",
            events.first().unwrap(),
            Utc::now(),
            Utc::now(),
        )
        .unwrap();

        assert_eq!(row.event_name, "checkout_completed");
        assert_eq!(row.distinct_id, "user_42");
        assert_eq!(row.anonymous_id, "anon_abc");
        assert_eq!(row.properties, r#"{"amount":99.5}"#);
    }

    #[test]
    fn payload_keeps_legacy_events_with_name_field() {
        let payload: IngestPayload = serde_json::from_value(serde_json::json!({
            "events": [
                {
                    "type": "track",
                    "name": "$autocapture",
                    "distinct_id": "anon_1",
                    "anonymous_id": "anon_1",
                    "properties": { "type": "click" }
                }
            ]
        }))
        .unwrap();

        let events = payload.events_batch().unwrap();
        let row = build_row(
            "default",
            "web",
            events.first().unwrap(),
            Utc::now(),
            Utc::now(),
        )
        .unwrap();

        assert_eq!(row.event_name, "$autocapture");
        assert_eq!(row.properties, r#"{"type":"click"}"#);
    }

    #[test]
    fn alias_adds_from_to_properties_for_identity_join() {
        let payload: IngestPayload = serde_json::from_value(serde_json::json!({
            "batch": [
                {
                    "type": "alias",
                    "distinct_id": "user_42",
                    "anonymous_id": "11111111-1111-4111-8111-111111111111"
                }
            ]
        }))
        .unwrap();

        let events = payload.events_batch().unwrap();
        let row = build_row(
            "default",
            "web",
            events.first().unwrap(),
            Utc::now(),
            Utc::now(),
        )
        .unwrap();

        assert_eq!(row.event_name, "$alias");
        assert_eq!(row.distinct_id, "user_42");
        assert_eq!(row.anonymous_id, "11111111-1111-4111-8111-111111111111");
        assert_eq!(
            row.properties,
            r#"{"from":"11111111-1111-4111-8111-111111111111","to":"user_42"}"#
        );
    }

    #[test]
    fn rejects_invalid_event_name() {
        let raw = raw_track("checkout completed", serde_json::json!({ "amount": 99.5 }));

        let err = build_row("default", "checkout", &raw, Utc::now(), Utc::now()).unwrap_err();

        assert!(matches!(err, ApiError::BadRequest(_)));
        assert!(err.to_string().contains("event_name"));
    }

    #[test]
    fn rejects_properties_over_16kb() {
        let raw = raw_track(
            "checkout_completed",
            serde_json::json!({ "blob": "x".repeat(16 * 1024) }),
        );

        let err = build_row("default", "checkout", &raw, Utc::now(), Utc::now()).unwrap_err();

        assert!(matches!(err, ApiError::BadRequest(_)));
        assert!(err.to_string().contains("properties"));
    }

    #[test]
    fn redact_event_does_not_redact_identity_or_event_name() {
        let rules =
            crate::redaction::CompiledRules::from_config(&crate::redaction::RedactionConfig {
                enabled: true,
                builtins: vec!["apikey_kv".to_string()],
                custom: vec![],
            })
            .unwrap();
        let mut row = ProductEventRow {
            timestamp: Utc::now(),
            project_id: "default".to_string(),
            event_name: "token_refreshed".to_string(),
            distinct_id: "user_token_42".to_string(),
            anonymous_id: "anon_token".to_string(),
            session_id: String::new(),
            properties: r#"{"token":"secret"}"#.to_string(),
            user_properties: r#"{"plan":"pro"}"#.to_string(),
            context: r#"{"access_token":"secret"}"#.to_string(),
            source: "web".to_string(),
            trace_id: String::new(),
            span_id: String::new(),
            event_id: Uuid::new_v4().to_string(),
        };

        redact_event(Some(&rules), &mut row);

        assert_eq!(row.event_name, "token_refreshed");
        assert_eq!(row.distinct_id, "user_token_42");
        assert_eq!(row.anonymous_id, "anon_token");
        assert_eq!(row.properties, r#"{token=[REDACTED]}"#);
        assert_eq!(row.context, r#"{access_token=[REDACTED]}"#);
    }
}
