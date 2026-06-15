//! Self-observability — el backend se emite a sí mismo telemetría OTLP.
//!
//! Opt-in: activado solo si `FARO_SELF_OBSERVE=true`. El default es OFF
//! por dos razones:
//!
//! 1. Evita un loop de arranque en frío. En el primer boot del stack,
//!    ClickHouse puede no estar lista; si el backend ya está emitiendo
//!    spans a `:4318`, las ingestas fallidas se vuelven más spans
//!    fallidos.
//! 2. Permite usar Faro contra OTRO Faro (o cualquier collector OTLP)
//!    cambiando `FARO_SELF_OBSERVE_ENDPOINT`.
//!
//! Ver `docs/adr/0007-self-observability.md` para el porqué y los
//! trade-offs.

use std::time::Duration;

use anyhow::Result;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;

/// Inicializa el exportador OTLP de tracing si `FARO_SELF_OBSERVE=true`.
/// Devuelve un guard cuyo `Drop` hace flush + shutdown ordenado de los
/// providers OTel; el guard se debe mantener vivo durante toda la vida
/// del proceso (en `main`).
///
/// Convive con el `tracing_subscriber` plano existente: el subscriber
/// sigue escribiendo a stderr para no perder visibilidad si el exporter
/// falla, y `tracing-opentelemetry` se añade como capa que duplica los
/// spans hacia OTLP.
pub fn init_otel() -> Result<Option<OtelGuard>> {
    if !is_enabled() {
        return Ok(None);
    }

    let endpoint = std::env::var("FARO_SELF_OBSERVE_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4318".into());
    let service_name = std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "faro-backend".into());

    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    let resource = Resource::builder()
        .with_attributes([
            KeyValue::new(
                opentelemetry_semantic_conventions::resource::SERVICE_NAME,
                service_name.clone(),
            ),
            KeyValue::new(
                opentelemetry_semantic_conventions::resource::SERVICE_VERSION,
                env!("CARGO_PKG_VERSION"),
            ),
        ])
        .build();

    // Exporter HTTP/proto apuntando al listener OTLP del propio backend.
    // API migrada a opentelemetry-otlp 0.32: el builder tipado se mantiene,
    // pero `with_batch_exporter` ya no recibe runtime — desde 0.28 el
    // BatchSpanProcessor corre en su propio hilo dedicado.
    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(format!("{endpoint}/v1/traces"))
        .with_timeout(Duration::from_secs(5))
        .build()?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer(service_name);
    opentelemetry::global::set_tracer_provider(provider.clone());

    tracing::info!(
        endpoint = %endpoint,
        "self-observability ENABLED — el backend emitirá spans OTLP a sí mismo"
    );

    Ok(Some(OtelGuard {
        provider,
        _tracer: tracer,
    }))
}

/// Drop guard que hace flush + shutdown ordenado del provider al salir.
pub struct OtelGuard {
    provider: SdkTracerProvider,
    _tracer: opentelemetry_sdk::trace::SdkTracer,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Err(e) = self.provider.shutdown() {
            tracing::warn!(error = %e, "fallo al shutdown del tracer OTel");
        }
    }
}

fn is_enabled() -> bool {
    std::env::var("FARO_SELF_OBSERVE")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}
