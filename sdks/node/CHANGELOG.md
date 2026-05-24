# Changelog — @iaportafolio/node

Cambios del SDK de Node.js. Sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y [SemVer](https://semver.org/lang/es/). Cada release se publica a npm
empujando un tag `sdk-node-v<semver>`.

## [Unreleased]

### Added
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
