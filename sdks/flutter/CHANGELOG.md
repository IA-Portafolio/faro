# Changelog

## Unreleased

### Added
- Alias `warning()` (paridad cross-SDK con `WARNING`).
- Auto-redacción: `scrubFields`, `scrubHeaders`, `scrubPatterns` con los
  mismos defaults que el resto de SDKs. `WireEntry` ahora público para
  uso en `beforeSend`.
- Hook `beforeSend(WireEntry) → WireEntry?` post-scrub; devolver `null`
  descarta.
- `WidgetsBindingObserver` integrado: flush automático cuando la app
  pasa a `paused`/`hidden`/`detached` (no pierdes eventos al cerrar).
- Suite `flutter test` (11 tests entre `client_test.dart` y `faro_test.dart`)
  con HttpServer real en localhost cubriendo las 6 invariantes mínimas:
  init inválido (×3, `ArgumentError`), payload shape, queue cap, retry
  5xx, `captureException` (shape OTel) y `close()` graceful — más
  beforeSend y scrubbing.

### Changed
- `Faro.init()` valida endpoint/token/service no vacíos (lanza
  `ArgumentError` en lugar de fallar críptico más abajo).
- `close()` desregistra el `WidgetsBindingObserver`.

## 0.1.0 (2026-05-23)

- Lanzamiento inicial: API `Faro.run` / `Faro.init`, captura automática de errores
  vía `FlutterError.onError`, `PlatformDispatcher.onError` y `runZonedGuarded`,
  buffering asíncrono y flush periódico.
