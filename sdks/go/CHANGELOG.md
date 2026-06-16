# Changelog — github.com/IA-Portafolio/faro/sdks/go

Cambios del SDK de Go. Sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y [SemVer](https://semver.org/lang/es/). Cada release se distribuye vía
tags `sdks/go/v<semver>` que indexa `proxy.golang.org` automáticamente
cuando se empuja un tag `sdk-go-v<semver>` al repo.

## [Unreleased]

### Added — v0.2.0 (tracing)
- **El tracing ahora está respaldado por `go.opentelemetry.io/otel/sdk`.**
  API pública (`StartSpan` / `WithSpan` / `Span` / `ContextWithSpan` /
  `SpanFromContext` / `Traceparent`) se mantiene compat, pero por dentro
  envuelve `go.opentelemetry.io/otel/trace.Span`. Esto permite combinar
  instrumentación manual con las auto-instrumentaciones OTel estándar
  (`otelhttp`, `otelgrpc`, `otelsql`, `otelpgx`) en un solo pipeline.
- **Nuevas deps runtime**: `go.opentelemetry.io/otel`, `otel/sdk`, `otel/trace`,
  `otel/semconv/v1.26.0`. ~6MB de impacto en `go.sum`.
- **`Client.spansCh`, `spansLoop`, `buildSpansPayload`, `sendSpans`
  eliminados** — el BatchSpanProcessor de OTel + nuestro
  `faroJSONSpanExporter` se encargan de batching y export a `/v1/traces`.
  `Client.Flush` ahora llama a `provider.ForceFlush()`.
- **`Client.Close` también apaga el provider** vía `ShutdownTracing(ctx)`.

### Added — v0.2.0 (tracing)
- Funciones públicas `InitTracing`, `ShutdownTracing`, `FlushTracing`,
  `GetTracer` — para users que prefieren control fino del tracing sin
  pasar por `faro.Init()` (apps que ya tienen OTel configurado, tests, etc.).
- **Cómo usar otelhttp con Faro** (HTTP server + client auto-instrumentado):
  ```go
  import "go.opentelemetry.io/contrib/instrumentation/net/http/otelhttp"
  client := &http.Client{Transport: otelhttp.NewTransport(http.DefaultTransport)}
  handler = otelhttp.NewHandler(handler, "myapp")
  ```
  Cada request entrante/saliente genera un span auto-emitido por Faro.
- Tracing nativo OTLP/HTTP/JSON. API paridad cross-SDK con `@iaportafolio/node`
  y `faro_sdk` (Python):
  - `client.StartSpan(ctx, name, faro.SpanOptions{Kind, Attributes, Parent}) → (ctx, *Span)`
  - `client.WithSpan(ctx, name, func(ctx, span) error, opts) → error` con
    auto-close + `RecordException` en error.
  - Métodos del span: `SetAttribute / SetAttributes / AddEvent / SetStatus /
    RecordException / End / Traceparent / TraceID / SpanID`.
  - `SpanKind*` enums (Internal/Server/Client/Producer/Consumer) y
    `Status*` (Unset/OK/Error) con valores numéricos OTLP.
  - `ContextWithSpan(ctx, span)` / `SpanFromContext(ctx)` para integraciones
    custom; `SpanParent` admite `Traceparent` W3C, `TraceID/SpanID`, o
    `ForceRoot: true` para romper la herencia.
- **Auto-correlación logs ↔ trazas**: `client.LogContext(ctx, ...)` (alias
  `InfoContext / WarnContext / ErrorContext`) lee el span activo del `ctx`
  y adjunta `trace_id` + `span_id` automáticamente.
- Subpaquete `sdks/go/ginfaro` (go.mod aparte) con `ginfaro.Tracing()`
  middleware para Gin: crea span SERVER por request, hereda W3C `traceparent`
  entrante, propaga al response, registra status/errors. Lo separamos en
  su propio módulo para que el SDK core siga zero-dep.
- 9 tests de tracing con `httptest.NewServer` separando `/v1/traces` y
  `/api/v1/ingest/logs`: emisión OTLP shape, parent contextual, `WithSpan`
  con error, `ForceRoot`, traceparent parent, auto-correlación, formato W3C,
  `RecordException`, idempotencia de `End()`.

### Added — v0.1.x
- Alias `Warning()` además de `Warn()` (paridad cross-SDK con `WARNING`).
- Auto-redacción: opciones `ScrubFields`, `DisableHeaderScrub`,
  `ScrubPatterns`. Defaults: lista común de campos sensibles +
  `["jwt","api-key"]`.
- Hook `BeforeSend func(*Entry) *Entry` — devolver `nil` descarta el evento.
- Suite `go test` (12 tests) con `httptest.NewServer` cubriendo las 6
  invariantes mínimas: init inválido (×3 incluyendo env vars), payload
  shape, queue cap, retry 5xx, `Recover()` panic-handling, `Close()`
  graceful + BeforeSend + scrubbing.

### Changed
- `send()` ahora **re-encola batches en 5xx** vía el canal de la cola
  (antes solo loggeaba y los perdía). 4xx se descartan.

## [0.1.0] — 2026-05-22

### Added
- Cliente Go zero-dep (solo stdlib) para logs y eventos de error.
- Bufferring básico con flush periódico para reducir RTT.
