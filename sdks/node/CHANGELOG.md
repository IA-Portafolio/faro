# Changelog — @iaportafolio/node

Cambios del SDK de Node.js. Sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y [SemVer](https://semver.org/lang/es/). Cada release se publica a npm
empujando un tag `sdk-node-v<semver>`.

## [Unreleased]

### Breaking
- **El tracing ahora está respaldado por `@opentelemetry/sdk-trace-node` + `auto-instrumentations-node`.**
  La API pública (`startSpan` / `withSpan` / `activeSpan` / `Span` / `traceparent()`)
  se mantiene compat, pero por dentro envuelve `@opentelemetry/api`. Esto añade
  spans **automáticos** para http, fetch, express, fastify, koa, pg, mongodb,
  redis, ioredis, grpc, kafka, … sin instrumentar manualmente — Service Map
  y la pestaña Trazas se llenan solas en `faro.iaportafolio.com`.
- **Nuevas dependencias runtime** (~50 paquetes OTel): `@opentelemetry/api`,
  `sdk-trace-node`, `auto-instrumentations-node`, `exporter-trace-otlp-http`,
  `instrumentation`, `resources`, `semantic-conventions`. Si necesitás opt-out,
  usá `enableTracing: false` en `init(...)` — el SDK funciona como antes (solo
  logs/errores/events).
- **Auto-instrumentación requiere init temprano.** En Node, OTel debe
  inicializarse antes de que se importen las librerías a instrumentar. Camino
  recomendado: `node --import @iaportafolio/node/instrument server.js` (lee
  `FARO_ENDPOINT` / `FARO_INGEST_TOKEN` / `OTEL_SERVICE_NAME` del entorno).
  Inline también funciona pero el `faro.init(...)` o `initTracing(...)` debe
  ser la primera línea del entrypoint.
- **La cola interna de spans (`spansQueue`) y el emisor OTLP/JSON propio
  desaparecen.** El BatchSpanProcessor de OTel se encarga del batching y la
  exportación a `/v1/traces`. `c.flush()` ahora hace `forceFlush()` del
  provider y drena en milisegundos.
- **`@opentelemetry/api` pasa de optional peer a dependency directa** — ya no
  hace falta instalarlo aparte.

### Added
- Subimport `@iaportafolio/node/instrument` — pre-loader para `--import` que
  inicializa el tracing desde env vars antes de tu primera línea de código.
- Subimport `@iaportafolio/node/tracing` — exporta `initTracing`,
  `shutdownTracing`, `flushTracing`, `getTracer` para usar OTel directamente.
- Opciones `enableTracing` (default `true`), `tracesEndpoint`,
  `resourceAttributes`, `disabledInstrumentations` en `FaroOptions`.
- `service.version` y `deployment.environment[.name]` se emiten en el Resource
  OTel y por lo tanto aparecen en cada span exportado.
- Alias `warning()` (paridad con `logging.WARNING` de Python y otros loggers).
- Auto-redacción: `scrubFields` (default: password/token/secret/authorization/
  cookie/set-cookie/api_key/apikey), `scrubHeaders` (default `true`),
  `scrubPatterns` (presets `email`/`jwt`/`credit-card`/`api-key`; defaults
  `['jwt','api-key']`).
- Hook `beforeSend(entry) → entry | null` para muestrear / transformar /
  descartar antes de enqueue (post-scrub).
- Subimport `@iaportafolio/node/pino` — transport para [pino](https://github.com/pinojs/pino)
  vía `pino-abstract-transport` (worker thread).
- Subimport `@iaportafolio/node/winston` — `FaroTransport` class para
  [winston](https://github.com/winstonjs/winston); acepta `{ client }` para
  reutilizar un singleton ya inicializado.
- Validación temprana en `init()`: endpoint/token/service obligatorios
  lanzan `Error` claro en lugar del críptico "Cannot read properties of
  undefined" más abajo.
- Suite de tests unitaria (12 tests) — node test runner built-in.

### Changed
- `flush()` ahora **re-encola batches en 5xx** (antes solo lo hacía en
  fallo de red). 4xx se descartan deliberadamente (batch malformado /
  auth inválida; reintentar acumularía basura).
- `close(timeoutMs = 5000)` acepta un timeout explícito — antes podía
  quedar en bucle si la red estaba flaky.

### Documented
- Sección "Integración con loggers" en README con pino y winston.
- Patrón `SIGTERM → await faro.close()` documentado en `sdks/README.md`.

## [0.1.0] — 2026-05-22

### Added
- Cliente HTTP ligero para enviar logs y excepciones al endpoint nativo
  de Faro (`/api/v1/ingest/logs`).
- Build dual ESM + CJS via `tsup`, types `.d.ts` incluidos.
- Publicación con `npm provenance`.
