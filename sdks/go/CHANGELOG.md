# Changelog — github.com/IA-Portafolio/faro/sdks/go

Cambios del SDK de Go. Sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y [SemVer](https://semver.org/lang/es/). Cada release se distribuye vía
tags `sdks/go/v<semver>` que indexa `proxy.golang.org` automáticamente
cuando se empuja un tag `sdk-go-v<semver>` al repo.

## [Unreleased]

### Added
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
