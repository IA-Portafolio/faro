# Changelog — faro-sdk (Python)

Cambios del SDK de Python. Sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y [SemVer](https://semver.org/lang/es/). Cada release se publica a PyPI
empujando un tag `sdk-python-v<semver>`.

## [Unreleased]

## [0.2.0] — 2026-06-16

### Breaking
- **El tracing ahora está respaldado por OpenTelemetry SDK + auto-instrumentación.**
  La API pública (`start_span` / `use_span` / `active_span` / `Span` /
  `traceparent()` / `record_exception`) se mantiene compat, pero por dentro
  envuelve `opentelemetry.trace.Span`. Esto añade spans **automáticos** para
  `requests`, `urllib3`, y los que el usuario active opt-in via extras
  (`fastapi`, `flask`, `psycopg`, `redis`, `sqlalchemy`, …) — Service Map y
  pestaña Trazas se llenan solas.
- **Nuevas dependencias runtime**: `opentelemetry-api`, `opentelemetry-sdk`,
  `opentelemetry-instrumentation`, `-instrumentation-requests`, `-instrumentation-urllib3`.
  El exporter HTTP/JSON contra `/v1/traces` lo escribimos in-house
  (`FaroJsonSpanExporter`) para no traer `opentelemetry-exporter-otlp-proto-http`
  (que es protobuf-only y nuestro backend habla JSON).
- **Worker de spans, `_spans_queue` y `_send_spans` eliminados** — OTel
  `BatchSpanProcessor` maneja batching y export. `client.flush()` ahora hace
  `force_flush()` del provider.
- **`_ACTIVE_SPAN` contextvar interno reemplazado** por el ContextManager de
  OTel. Para activar un span externo al middleware Faro, usá
  `opentelemetry.context.attach` / `detach` con `trace.set_span_in_context`.
- **`parent=None` semántica**: sigue significando "forzar root span" (no hereda
  del contexto activo). El default `parent=...` (Ellipsis) hereda como antes.

### Added
- Extras opcionales (`pip install faro-sdk[fastapi]`, `[flask]`, `[django]`,
  `[postgres]`, `[mongo]`, `[redis]`, `[sqlalchemy]`, `[celery]`, `[httpx]`,
  `[aiohttp]`, `[starlette]`, `[all-instrumentations]`). El SDK detecta cada
  instrumentor instalado al init y lo activa.
- Opciones nuevas en `init(...)`: `enable_tracing` (default `True`),
  `traces_endpoint`, `resource_attributes`, `disabled_instrumentations`.
- Funciones expuestas: `init_tracing`, `shutdown_tracing`, `flush_tracing`,
  `get_tracer` — para users que prefieren control fino sin pasar por
  `faro.init()` (p. ej. apps que ya tienen OTel configurado).
- `service.version` y `deployment.environment[.name]` se emiten en el Resource
  OTel y aparecen en cada span exportado.
- Lectura de `FARO_INGEST_TOKEN` además de `FARO_TOKEN` (paridad cross-SDK).
- Alias `warning()` (paridad con `logging.WARNING` — antes solo `warn()`).
- Auto-redacción: `scrub_fields` (defaults sensatos), `scrub_headers=True`,
  `scrub_patterns=('jwt','api-key')`. Aplica a values de attributes y al
  `message` antes de enqueue.
- Hook `before_send(entry) -> entry | None` para muestrear / transformar /
  descartar (post-scrub).
- Suite pytest (12 tests) cubriendo queue cap, retry 5xx, before_send,
  scrubbing, validación de init y lectura de env vars.

### Changed
- `_send()` ahora **re-encola batches en 5xx**. 4xx se descartan (batch
  malformado / auth inválida).
- `close(timeout=5.0)` hace `worker.join(timeout=...)` además del drenado
  — el worker es daemon, sin join podía truncarse a mitad de POST.

## [0.1.0] — 2026-05-22

### Added
- Cliente sincrónico para enviar logs y excepciones capturadas a Faro.
- Handler `logging.Handler` para integrar con la stdlib de logging.
- Publicación a PyPI vía Trusted Publishing (sin tokens en el repo).
