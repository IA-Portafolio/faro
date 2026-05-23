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

/// Accept either RFC3339 ("2024-01-01T12:34:56.123Z") or ClickHouse's space-separated
/// `toString(DateTime64)` output ("2024-01-01 12:34:56.123456789").
pub fn de_dt<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
    let s = String::deserialize(d)?;
    parse_dt(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid datetime: {s}")))
}

fn de_dt_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
    let opt: Option<String> = Option::deserialize(d)?;
    match opt.as_deref() {
        None => Ok(None),
        Some("") => Ok(None),
        Some(s) => Ok(parse_dt(s)),
    }
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
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt", default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt", default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted: u8,
    #[serde(default = "default_version")]
    pub version: u64,
}

fn default_interval() -> u32 { 60 }
fn default_timeout() -> u32 { 30 }
fn default_status_min() -> u16 { 200 }
fn default_status_max() -> u16 { 299 }
fn default_true() -> u8 { 1 }
fn default_version() -> u64 { 1 }
fn default_project() -> String { "default".into() }

// ---------- Projects ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectRow {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub ingest_token: String,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt", default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt", default = "Utc::now")]
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
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt", default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt", default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted: u8,
    #[serde(default = "default_version")]
    pub version: u64,
}

fn default_window() -> u32 { 300 }
fn default_check_interval() -> u32 { 60 }
fn default_severity() -> String { "warn".into() }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlertIncidentRow {
    pub id: Uuid,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub rule_id: Uuid,
    pub rule_name: String,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt")]
    pub started_at: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339_millis_opt", deserialize_with = "de_dt_opt", default)]
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
