# Changelog — faro-sdk (Python)

Cambios del SDK de Python. Sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y [SemVer](https://semver.org/lang/es/). Cada release se publica a PyPI
empujando un tag `sdk-python-v<semver>`.

## [Unreleased]

### Added
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
