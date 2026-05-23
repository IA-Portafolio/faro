//! OTLP (OpenTelemetry Protocol) JSON message shapes.
//! We support OTLP/HTTP+JSON content type. Field names mirror the protobuf spec.
//! Reference: https://github.com/open-telemetry/opentelemetry-proto/blob/main/opentelemetry/proto/

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportLogsRequest {
    #[serde(default)]
    pub resource_logs: Vec<ResourceLogs>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLogs {
    #[serde(default)]
    pub resource: Option<Resource>,
    #[serde(default)]
    pub scope_logs: Vec<ScopeLogs>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeLogs {
    #[serde(default)]
    pub scope: Option<Scope>,
    #[serde(default)]
    pub log_records: Vec<LogRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    #[serde(default)]
    pub time_unix_nano: Option<StringOrU64>,
    #[serde(default)]
    pub observed_time_unix_nano: Option<StringOrU64>,
    #[serde(default)]
    pub severity_number: Option<u8>,
    #[serde(default)]
    pub severity_text: Option<String>,
    #[serde(default)]
    pub body: Option<AnyValue>,
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub span_id: Option<String>,
}

// ---------- Traces ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTracesRequest {
    #[serde(default)]
    pub resource_spans: Vec<ResourceSpans>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSpans {
    #[serde(default)]
    pub resource: Option<Resource>,
    #[serde(default)]
    pub scope_spans: Vec<ScopeSpans>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSpans {
    #[serde(default)]
    pub scope: Option<Scope>,
    #[serde(default)]
    pub spans: Vec<Span>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default)]
    pub parent_span_id: Option<String>,
    #[serde(default)]
    pub trace_state: Option<String>,
    pub name: String,
    #[serde(default)]
    pub kind: Option<u8>,
    pub start_time_unix_nano: StringOrU64,
    pub end_time_unix_nano: StringOrU64,
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
    #[serde(default)]
    pub events: Vec<SpanEvent>,
    #[serde(default)]
    pub links: Vec<SpanLink>,
    #[serde(default)]
    pub status: Option<SpanStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanEvent {
    pub time_unix_nano: StringOrU64,
    pub name: String,
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanLink {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanStatus {
    #[serde(default)]
    pub code: Option<u8>,
    #[serde(default)]
    pub message: Option<String>,
}

// ---------- Metrics ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportMetricsRequest {
    #[serde(default)]
    pub resource_metrics: Vec<ResourceMetrics>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceMetrics {
    #[serde(default)]
    pub resource: Option<Resource>,
    #[serde(default)]
    pub scope_metrics: Vec<ScopeMetrics>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeMetrics {
    #[serde(default)]
    pub scope: Option<Scope>,
    #[serde(default)]
    pub metrics: Vec<Metric>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metric {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub gauge: Option<GaugeData>,
    #[serde(default)]
    pub sum: Option<SumData>,
    #[serde(default)]
    pub histogram: Option<HistogramData>,
    #[serde(default)]
    pub summary: Option<SummaryData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GaugeData {
    #[serde(default)]
    pub data_points: Vec<NumberDataPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SumData {
    #[serde(default)]
    pub data_points: Vec<NumberDataPoint>,
    #[serde(default)]
    pub is_monotonic: Option<bool>,
    #[serde(default)]
    pub aggregation_temporality: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramData {
    #[serde(default)]
    pub data_points: Vec<HistogramDataPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryData {
    #[serde(default)]
    pub data_points: Vec<SummaryDataPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberDataPoint {
    #[serde(default)]
    pub time_unix_nano: Option<StringOrU64>,
    #[serde(default)]
    pub as_double: Option<f64>,
    #[serde(default)]
    pub as_int: Option<StringOrI64>,
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramDataPoint {
    #[serde(default)]
    pub time_unix_nano: Option<StringOrU64>,
    #[serde(default)]
    pub count: Option<StringOrU64>,
    #[serde(default)]
    pub sum: Option<f64>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub explicit_bounds: Vec<f64>,
    #[serde(default)]
    pub bucket_counts: Vec<StringOrU64>,
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryDataPoint {
    #[serde(default)]
    pub time_unix_nano: Option<StringOrU64>,
    #[serde(default)]
    pub count: Option<StringOrU64>,
    #[serde(default)]
    pub sum: Option<f64>,
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
}

// ---------- Common ----------

#[derive(Debug, Deserialize)]
pub struct Resource {
    #[serde(default)]
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Deserialize)]
pub struct Scope {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct KeyValue {
    pub key: String,
    #[serde(default)]
    pub value: Option<AnyValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnyValue {
    #[serde(default)]
    pub string_value: Option<String>,
    #[serde(default)]
    pub bool_value: Option<bool>,
    #[serde(default)]
    pub int_value: Option<StringOrI64>,
    #[serde(default)]
    pub double_value: Option<f64>,
    #[serde(default)]
    pub array_value: Option<ArrayValue>,
    #[serde(default)]
    pub kvlist_value: Option<KvListValue>,
    #[serde(default)]
    pub bytes_value: Option<String>,
}

impl AnyValue {
    pub fn to_string_value(&self) -> String {
        if let Some(s) = &self.string_value {
            return s.clone();
        }
        if let Some(b) = self.bool_value {
            return b.to_string();
        }
        if let Some(i) = &self.int_value {
            return i.to_string();
        }
        if let Some(d) = self.double_value {
            return d.to_string();
        }
        if let Some(a) = &self.array_value {
            let v: Vec<String> = a.values.iter().map(|x| x.to_string_value()).collect();
            return format!("[{}]", v.join(","));
        }
        if let Some(kv) = &self.kvlist_value {
            let parts: Vec<String> = kv
                .values
                .iter()
                .map(|p| format!("\"{}\":\"{}\"", p.key, p.value.as_ref().map(|v| v.to_string_value()).unwrap_or_default()))
                .collect();
            return format!("{{{}}}", parts.join(","));
        }
        if let Some(b) = &self.bytes_value {
            return b.clone();
        }
        String::new()
    }
}

#[derive(Debug, Deserialize)]
pub struct ArrayValue {
    #[serde(default)]
    pub values: Vec<AnyValue>,
}

#[derive(Debug, Deserialize)]
pub struct KvListValue {
    #[serde(default)]
    pub values: Vec<KeyValue>,
}

/// Algunos exportadores OTel emiten enteros grandes como strings JSON (porque los enteros
/// de 64 bits exceden el rango seguro de JS); otros los emiten como números. Acepta ambos.
#[derive(Debug, Clone)]
pub struct StringOrU64(pub u64);

impl<'de> Deserialize<'de> for StringOrU64 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let n = match v {
            Value::Number(n) => n.as_u64().unwrap_or(0),
            Value::String(s) => s.parse().unwrap_or(0),
            _ => 0,
        };
        Ok(StringOrU64(n))
    }
}

#[derive(Debug, Clone)]
pub struct StringOrI64(pub i64);

impl<'de> Deserialize<'de> for StringOrI64 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let n = match v {
            Value::Number(n) => n.as_i64().unwrap_or(0),
            Value::String(s) => s.parse().unwrap_or(0),
            _ => 0,
        };
        Ok(StringOrI64(n))
    }
}

impl std::fmt::Display for StringOrI64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn attrs_to_map(attrs: &[KeyValue]) -> crate::storage::AttrMap {
    let mut out = crate::storage::AttrMap::new();
    for kv in attrs {
        if let Some(v) = &kv.value {
            out.insert(kv.key.clone(), v.to_string_value());
        }
    }
    out
}

pub fn service_name(resource: &Option<Resource>) -> String {
    if let Some(r) = resource {
        for kv in &r.attributes {
            if kv.key == "service.name" {
                if let Some(v) = &kv.value {
                    return v.to_string_value();
                }
            }
        }
    }
    "unknown".into()
}
