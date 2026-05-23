# ADR-0003: Rust + axum para el backend

- **Estado**: Accepted
- **Fecha**: 2026-05-23
- **Autores**: @victalejo

## Contexto

El backend hace tres cosas concurrentes: recibir ingesta HTTP (REST
nativa + OTLP/HTTP), persistir filas en ClickHouse en batches, y
correr workers de fondo (runner de monitores, evaluador de alertas,
indexador de errores). Necesitamos:

- Buen throughput por core (single-VM deployment).
- Latencia predecible bajo carga (sin GC pauses).
- Seguridad de memoria (vamos a parsear input no confiable de muchos
  clientes).
- Ergonomía razonable para mantener una persona.

## Decisión

Usamos **Rust** con **axum** + **tokio**. El backend es un solo
binario con dos listeners HTTP:

- `:8080` — API REST/SSE y endpoint nativo de ingesta.
- `:4318` — receptores OTLP/HTTP+JSON.

ClickHouse se accede vía `reqwest` con su HTTP interface (no usamos
crate de driver específico — keep it simple).

## Alternativas consideradas

- **Go** — buen runtime, ecosistema OTEL excelente, ergonomía menor.
  Descartado por preferencia de mantenedor y por la garantía de safety
  de Rust ante input adversario.
- **Node/TypeScript** — fricción de runtime en ingesta sostenida
  (single-thread del event loop bajo CPU-bound encoding/decoding).
- **Java/Kotlin (Spring)** — overhead operacional (JVM, GC tuning)
  innecesario para un binario small-footprint.

## Consecuencias

### Positivas
- Binario estático sin runtime — la imagen Docker pesa ~30MB.
- `tokio::sync::mpsc` modela limpiamente el patrón
  ingest-channel → batch-writer.
- `tower-http` da CORS, gzip y tracing sin escribir middleware
  custom.
- Sin riesgo de OOM por GC pausas en momentos de ingesta pico.

### Negativas / costo asumido
- Tiempo de compilación de CI más largo (~3 min en frío, ~30s con
  cache). Mitigamos con `Swatinem/rust-cache` en CI.
- Onboarding más lento si alguien nuevo no sabe Rust.
- Menos tooling drop-in para observabilidad de la propia app
  (pero usaremos Faro para observarse a sí mismo, eventualmente).

### Trabajo de seguimiento
- Considerar `axum-otel` o equivalente para auto-instrumentar la
  ingesta y mandarse spans a sí mismo (dogfooding).
