use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type AttrMap = BTreeMap<String, String>;

fn rfc3339_nanos<S: serde::Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&t.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
}

fn rfc3339_millis<S: serde::Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn rfc3339_millis_opt<S: serde::Serializer>(
    t: &Option<DateTime<Utc>>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match t {
        Some(v) => s.serialize_str(&v.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        None => s.serialize_none(),
    }
}

/// Acepta tanto RFC3339 ("2024-01-01T12:34:56.123Z") como la salida con espacio de
/// `toString(DateTime64)` de ClickHouse ("2024-01-01 12:34:56.123456789").
pub fn de_dt<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
    let s = String::deserialize(d)?;
    parse_dt(&s).ok_or_else(|| serde::de::Error::custom(format!("datetime inválido: {s}")))
}

fn de_dt_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
    let opt: Option<String> = Option::deserialize(d)?;
    match opt.as_deref() {
        None => Ok(None),
        Some("") => Ok(None),
        Some(s) => Ok(parse_dt(s)),
    }
}

pub fn parse_dt_pub(s: &str) -> Option<DateTime<Utc>> {
    parse_dt(s)
}

fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in &[
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(Utc.from_utc_datetime(&naive));
        }
    }
    None
}

// ---------- Logs ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogRow {
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub timestamp: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub observed_timestamp: DateTime<Utc>,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub service_name: String,
    pub severity_text: String,
    pub severity_number: u8,
    pub body: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub span_id: String,
    #[serde(default)]
    pub scope_name: String,
    #[serde(default)]
    pub resource_attributes: AttrMap,
    #[serde(default)]
    pub attributes: AttrMap,
}

impl LogRow {
    pub fn severity_from_text(s: &str) -> u8 {
        match s.to_ascii_uppercase().as_str() {
            "TRACE" | "TRACE1" | "TRACE2" | "TRACE3" | "TRACE4" => 1,
            "DEBUG" | "DEBUG1" | "DEBUG2" | "DEBUG3" | "DEBUG4" => 5,
            "INFO" | "INFO1" | "INFO2" | "INFO3" | "INFO4" => 9,
            "WARN" | "WARNING" | "WARN1" | "WARN2" | "WARN3" | "WARN4" => 13,
            "ERROR" | "ERR" | "ERROR1" | "ERROR2" | "ERROR3" | "ERROR4" => 17,
            "FATAL" | "CRITICAL" | "FATAL1" | "FATAL2" | "FATAL3" | "FATAL4" => 21,
            _ => 9,
        }
    }
}

// ---------- Spans ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpanRow {
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub timestamp: DateTime<Utc>,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub trace_id: String,
    pub span_id: String,
    #[serde(default)]
    pub parent_span_id: String,
    #[serde(default)]
    pub trace_state: String,
    pub name: String,
    pub kind: String,
    pub service_name: String,
    pub duration_ns: u64,
    pub status_code: String,
    #[serde(default)]
    pub status_message: String,
    #[serde(default)]
    pub resource_attributes: AttrMap,
    #[serde(default)]
    pub span_attributes: AttrMap,
    #[serde(default)]
    pub events_timestamps: Vec<String>,
    #[serde(default)]
    pub events_names: Vec<String>,
    #[serde(default)]
    pub events_attributes: Vec<String>,
    #[serde(default)]
    pub links_trace_ids: Vec<String>,
    #[serde(default)]
    pub links_span_ids: Vec<String>,
}

// ---------- Metrics ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricRow {
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub timestamp: DateTime<Utc>,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub metric_name: String,
    pub metric_type: String,
    #[serde(default)]
    pub metric_unit: String,
    pub service_name: String,
    pub value: f64,
    #[serde(default)]
    pub resource_attributes: AttrMap,
    #[serde(default)]
    pub attributes: AttrMap,
    #[serde(default)]
    pub hist_count: u64,
    #[serde(default)]
    pub hist_sum: f64,
    #[serde(default)]
    pub hist_min: f64,
    #[serde(default)]
    pub hist_max: f64,
    #[serde(default)]
    pub hist_bucket_bounds: Vec<f64>,
    #[serde(default)]
    pub hist_bucket_counts: Vec<u64>,
}

// ---------- Error clusters (MinHash compactor) ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorClusterRow {
    /// Fingerprint original del error (PK).
    pub fingerprint: String,
    /// Cluster al que pertenece. Si `cluster_id == fingerprint`, esta fila es el
    /// REPRESENTANTE del cluster (el primer fp que vio el compactador con esa firma).
    pub cluster_id: String,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub service_name: String,
    #[serde(default)]
    pub exception_type: String,
    /// Firma MinHash (K=128 enteros). En representantes se compara contra firmas
    /// nuevas para decidir si caen en el cluster.
    pub minhash: Vec<u64>,
    #[serde(default)]
    pub representative_message: String,
    #[serde(default)]
    pub representative_stack: String,
    #[serde(default = "default_one_u64")]
    pub member_count: u64,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub first_seen_at: DateTime<Utc>,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub last_seen_at: DateTime<Utc>,
    #[serde(default = "default_one_u64")]
    pub version: u64,
}

fn default_one_u64() -> u64 {
    1
}

// ---------- Notification channels (configurable notifier plugins) ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotificationChannelRow {
    /// Slug human-readable. Es el identificador estable que las reglas usan
    /// como target: `channel://<id>`.
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Selecciona qué Notifier instanciar. Valores soportados:
    /// `webhook` | `slack` | `discord` | `pagerduty` | `opsgenie` |
    /// `email_resend` | `telegram`.
    pub kind: String,
    #[serde(default = "default_one_u8")]
    pub enabled: u8,
    /// JSON con la config específica del kind. El backend deserializa con
    /// la struct de cada plugin (`notify::*::Config`).
    #[serde(default)]
    pub config: String,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub created_at: DateTime<Utc>,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_by: String,
    #[serde(default)]
    pub deleted: u8,
    #[serde(default = "default_one_u64")]
    pub version: u64,
}

fn default_one_u8() -> u8 {
    1
}

// ---------- Service stale events ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceStaleEventRow {
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt")]
    pub timestamp: DateTime<Utc>,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub service_name: String,
    /// `stale` (cruzó el umbral sin tráfico) o `recovered` (volvió a reportar tras estar stale).
    pub event: String,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt")]
    pub last_seen_at: DateTime<Utc>,
    pub silence_hours: f64,
}

// ---------- Errors ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorEventRow {
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub timestamp: DateTime<Utc>,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub fingerprint: String,
    pub service_name: String,
    pub severity_text: String,
    pub message: String,
    #[serde(default)]
    pub exception_type: String,
    #[serde(default)]
    pub exception_message: String,
    #[serde(default)]
    pub stack_trace: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub span_id: String,
    #[serde(default)]
    pub attributes: AttrMap,
}

// ---------- Monitors ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonitorRow {
    pub id: Uuid,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: AttrMap,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_interval")]
    pub interval_seconds: u32,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
    #[serde(default = "default_status_min")]
    pub expected_status_min: u16,
    #[serde(default = "default_status_max")]
    pub expected_status_max: u16,
    #[serde(default)]
    pub expected_body_regex: String,
    #[serde(default = "default_true")]
    pub enabled: u8,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub created_at: DateTime<Utc>,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted: u8,
    #[serde(default = "default_version")]
    pub version: u64,
}

fn default_interval() -> u32 {
    60
}
fn default_timeout() -> u32 {
    30
}
fn default_status_min() -> u16 {
    200
}
fn default_status_max() -> u16 {
    299
}
fn default_true() -> u8 {
    1
}
fn default_version() -> u64 {
    1
}
fn default_project() -> String {
    "default".into()
}

// ---------- Projects ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectRow {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub ingest_token: String,
    /// JSON con la config de redacción PII. Vacío = no redacta. El parsing/compilado
    /// vive en `crate::redaction`; aquí lo guardamos crudo para preservar el round-trip
    /// y permitir que el frontend reciba la config exacta.
    #[serde(default)]
    pub redaction_rules: String,
    /// JSON con la whitelist de orígenes browser para el RUM SDK. Vacío = sin
    /// verificación. El parsing/compilado vive en `crate::origin_check`.
    #[serde(default)]
    pub allowed_origins: String,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub created_at: DateTime<Utc>,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted: u8,
    #[serde(default = "default_version")]
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonitorResultRow {
    pub monitor_id: Uuid,
    #[serde(default = "default_project")]
    pub project_id: String,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt")]
    pub timestamp: DateTime<Utc>,
    pub success: u8,
    pub status_code: u16,
    pub duration_ms: u32,
    #[serde(default)]
    pub error_message: String,
    #[serde(default)]
    pub response_size: u32,
}

// ---------- Alerts ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlertRuleRow {
    pub id: Uuid,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub source: String,
    pub query: String,
    pub condition: String,
    pub threshold: f64,
    #[serde(default = "default_window")]
    pub window_seconds: u32,
    #[serde(default = "default_check_interval")]
    pub interval_seconds: u32,
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default)]
    pub notification_targets: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: u8,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub created_at: DateTime<Utc>,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted: u8,
    #[serde(default = "default_version")]
    pub version: u64,
}

fn default_window() -> u32 {
    300
}
fn default_check_interval() -> u32 {
    60
}
fn default_severity() -> String {
    "warn".into()
}

// ---------- Product events (6º pilar) ----------

/// Fila de `faro.product_events`. Refleja el schema definido en
/// `clickhouse/init/85-product-events.sql`.
///
/// `properties`, `user_properties` y `context` viajan como String JSON; la
/// tabla los almacena así (Map de cardinalidad infinita rompe ClickHouse).
/// El cliente los parsea cuando los necesita renderizar.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProductEventRow {
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub timestamp: DateTime<Utc>,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub event_name: String,
    pub distinct_id: String,
    #[serde(default)]
    pub anonymous_id: String,
    #[serde(default)]
    pub session_id: String,
    /// JSON serializado de las properties del evento (NO Map; ver schema SQL).
    #[serde(default)]
    pub properties: String,
    /// JSON serializado de user properties (atributos del usuario al disparar el evento).
    #[serde(default)]
    pub user_properties: String,
    /// JSON serializado de context (page_url, user_agent, ip, etc.).
    #[serde(default)]
    pub context: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub span_id: String,
    #[serde(default)]
    pub event_id: String,
}

fn default_source() -> String {
    "web".into()
}

// ---------- Product users (unificación multi-device, goal 10.E.1) ----------

/// Fila de `faro.product_users`. El worker `user_unifier` la mantiene
/// agregando eventos por `(project_id, distinct_id)`: une `anonymous_ids`,
/// `sources` y empuja `first_seen` / `last_seen` / `event_count`.
///
/// `ReplacingMergeTree(last_seen)` deduplica por la PK al merge — las queries
/// deben usar `FINAL` o `argMax` para leer una sola versión por usuario.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProductUserRow {
    #[serde(default = "default_project")]
    pub project_id: String,
    pub distinct_id: String,
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub first_seen: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub last_seen: DateTime<Utc>,
    #[serde(default)]
    pub anonymous_ids: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub event_count: u64,
    #[serde(default)]
    pub properties: String,
}

/// Fila de `faro.product_user_aliases`. Lookup reverso anon → distinct_id.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProductUserAliasRow {
    #[serde(default = "default_project")]
    pub project_id: String,
    pub anonymous_id: String,
    pub distinct_id: String,
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub linked_at: DateTime<Utc>,
}

// ---------- Product sessions (sesionización temporal, goal 10.F.1) ----------

/// Fila de `faro.product_sessions`. La rellena el worker `session_aggregator`:
/// si el SDK manda `session_id` se respeta; si no, se sintetiza un id estable
/// derivado de `(project_id, distinct_id, started_at)` y se cortan sesiones por
/// gap > `FARO_SESSION_GAP_MINUTES` (default 30 — convención GA/Mixpanel).
///
/// `ReplacingMergeTree(ended_at)` deduplica por `(project_id, session_id)` y
/// gana la fila con mayor `ended_at`: mientras una sesión sigue viva, cada
/// tick reinserta extendida.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProductSessionRow {
    #[serde(default = "default_project")]
    pub project_id: String,
    pub session_id: String,
    pub distinct_id: String,
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub started_at: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339_nanos", deserialize_with = "de_dt")]
    pub ended_at: DateTime<Utc>,
    #[serde(default)]
    pub page_count: u32,
    #[serde(default)]
    pub duration_seconds: u32,
    #[serde(default)]
    pub event_count: u32,
    #[serde(default)]
    pub pageview_count: u32,
    #[serde(default)]
    pub is_bounce: u8,
    #[serde(default)]
    pub is_engaged: u8,
    #[serde(default)]
    pub converted: u8,
    #[serde(default)]
    pub quality_score: f32,
    #[serde(default)]
    pub trace_ids: Vec<String>,
    #[serde(default)]
    pub trace_count: u32,
    #[serde(default = "default_source")]
    pub source: String,
}

// ---------- Integrations ----------

// ---------- Feature flags ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeatureFlagRow {
    #[serde(default = "default_project")]
    pub project_id: String,
    pub key: String,
    #[serde(default)]
    pub rollout_percentage: u8,
    #[serde(default)]
    pub conditions: String,
    #[serde(default = "default_true")]
    pub active: u8,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub updated_at: DateTime<Utc>,
    #[serde(default = "default_version")]
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntegrationRow {
    pub kind: String,
    #[serde(default = "default_true")]
    pub enabled: u8,
    /// JSON serializado con la config concreta de la integración. El esquema
    /// vive en el lado del módulo que la consume.
    #[serde(default)]
    pub config: String,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_by: String,
    #[serde(default = "default_version")]
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlertIncidentRow {
    pub id: Uuid,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub rule_id: Uuid,
    pub rule_name: String,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt")]
    pub started_at: DateTime<Utc>,
    #[serde(
        serialize_with = "rfc3339_millis_opt",
        deserialize_with = "de_dt_opt",
        default
    )]
    pub resolved_at: Option<DateTime<Utc>>,
    pub value: f64,
    pub threshold: f64,
    pub severity: String,
    pub status: String,
    #[serde(default)]
    pub note: String,
    #[serde(default = "default_version")]
    pub version: u64,
}

// ---------- Cohorts (segmentación de usuarios sobre product_events) ----------

/// Definición declarativa de un cohort, almacenada en `faro.cohorts.definition`
/// como JSON. Mantenerla aquí (y no como columnas tipadas) permite extender el
/// vocabulario de reglas sin romper la tabla.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CohortDefinition {
    /// Nombre del evento a contar.
    pub event: String,
    /// Comparador entre `count()` y `count`. Valores: `==`, `>=`, `>`, `<=`, `<`.
    pub op: String,
    /// Umbral del comparador.
    pub count: u32,
    /// Tamaño de la ventana hacia atrás desde "ahora", en días. Acotado por el
    /// backend a [1, 365] al evaluar.
    pub last_days: u32,
    /// Filtros opcionales sobre properties del evento:
    /// `JSONExtractString(product_events.properties, key) = value`.
    /// Tope práctico: 3 (más reduce la utilidad del bloom filter y se traduce
    /// en escaneos de columnas comprimidas con ZSTD(3)).
    #[serde(default)]
    pub filters: Vec<CohortFilter>,
    /// Filtros opcionales sobre traits persistidos del usuario desde
    /// `identify(user_id, traits)` en `product_users.properties`.
    #[serde(default)]
    pub user_filters: Vec<CohortFilter>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CohortFilter {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CohortRow {
    pub id: Uuid,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON serializado de [`CohortDefinition`]. El parser canónico vive en
    /// `api::cohorts`; aquí queda como String para preservar el round-trip
    /// y permitir extender el esquema sin migrar la tabla.
    pub definition: String,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub created_at: DateTime<Utc>,
    #[serde(
        serialize_with = "rfc3339_millis",
        deserialize_with = "de_dt",
        default = "Utc::now"
    )]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub deleted: u8,
    #[serde(default = "default_version")]
    pub version: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cohort_definition_defaults_user_filters_for_existing_json() {
        let json = r#"{
            "event": "checkout_completed",
            "op": ">=",
            "count": 1,
            "last_days": 30,
            "filters": [{ "key": "amount", "value": "99" }]
        }"#;

        let definition: CohortDefinition = serde_json::from_str(json).unwrap();

        assert!(definition.user_filters.is_empty());
    }

    #[test]
    fn cohort_definition_round_trips_user_filters() {
        let json = r#"{
            "event": "checkout_completed",
            "op": ">=",
            "count": 1,
            "last_days": 30,
            "filters": [],
            "user_filters": [
                { "key": "plan", "value": "pro" },
                { "key": "industry", "value": "fintech" }
            ]
        }"#;

        let definition: CohortDefinition = serde_json::from_str(json).unwrap();

        assert_eq!(definition.user_filters.len(), 2);
        assert_eq!(definition.user_filters[0].key, "plan");
        assert_eq!(definition.user_filters[0].value, "pro");
        assert_eq!(definition.user_filters[1].key, "industry");
        assert_eq!(definition.user_filters[1].value, "fintech");

        let encoded = serde_json::to_string(&definition).unwrap();
        let round_tripped: CohortDefinition = serde_json::from_str(&encoded).unwrap();

        assert_eq!(round_tripped.user_filters.len(), 2);
        assert_eq!(round_tripped.user_filters[0].key, "plan");
        assert_eq!(round_tripped.user_filters[0].value, "pro");
        assert_eq!(round_tripped.user_filters[1].key, "industry");
        assert_eq!(round_tripped.user_filters[1].value, "fintech");
    }
}
