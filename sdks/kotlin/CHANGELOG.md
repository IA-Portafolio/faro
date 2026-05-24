# Changelog — com.iaportafolio:faro (Kotlin / Android)

Cambios del SDK de Kotlin (Android + JVM). Sigue
[Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y
[SemVer](https://semver.org/lang/es/). Cada release se publica a
Maven Central empujando un tag `sdk-kotlin-v<semver>`.

## [Unreleased]

### Added
- Alias `Faro.warning()` (paridad cross-SDK con `WARNING`).
- Auto-redacción: `scrubFields`, `scrubHeaders`, `scrubPatterns` con los
  mismos defaults que el resto de SDKs. `WireEntry` ahora público para
  uso en `beforeSend`.
- Hook `beforeSend: ((WireEntry) -> WireEntry?)?` post-scrub.
- Validación temprana en `Faro.init()`: endpoint/token/service obligatorios
  lanzan `IllegalArgumentException` claro en lugar de fallar críptico más
  abajo (paridad cross-SDK).
- Suite JUnit5 (10 tests) con `com.sun.net.httpserver.HttpServer` cubriendo
  las 6 invariantes mínimas — init inválido (×3), payload shape, queue cap,
  retry 5xx, captureException, close graceful — más beforeSend/scrubbing.
  Dependencias `kotlin-test-junit5` y `kotlinx-coroutines-test` añadidas
  como `testImplementation`.

### Fixed
- **`Channel(capacity = 1)` ignoraba `maxQueueSize`** — el cap estaba
  hardcoded a 1, así que bajo carga el SDK perdía eventos en `trySend`
  cuando el flusher no se había vaciado todavía. Ahora `init()` crea el
  channel con `capacity = options.maxQueueSize`.
- **`init()` no era re-iniciable** — `scope` y `channel` declarados a nivel
  object con `val`. Tras `close()` el scope quedaba cancelado y un nuevo
  `init()` arrancaba un flusher en un scope muerto. Bug latente para tests,
  hot-reload en dev, fixtures multi-tenant. Ahora `init()` recrea scope y
  channel si están en estado terminal.
- **`send()` NO re-encolaba ante 5xx** — los otros 6 SDKs sí re-encolan;
  Kotlin solo loggeaba a stderr y descartaba el batch. Ahora 5xx (o fallo
  de red) re-encola; 4xx descarta (batch malformado / auth inválida).
- **`flush()` no esperaba al batch en vuelo** — solo verificaba `channel.isEmpty`,
  así que `close()` podía cancelar el scope a mitad de un HTTP POST y los
  eventos no llegaban. Ahora un `Mutex` que el flusher toma durante `send()`
  garantiza que `flush()` espere a que el batch en vuelo termine.
- **`beforeSend → null` NO descartaba el evento** — el operador `?:` lo
  rescataba con el `scrubbed` original. Crítico para usuarios que confiaban
  en `beforeSend` para muestrear o filtrar PII.

## [0.1.0] — 2026-05-22

### Added
- Cliente Kotlin sobre `kotlinx.coroutines` y `kotlinx.serialization`.
- API suspend-friendly para enviar logs y excepciones.
- Publicación firmada (PGP) a Maven Central vía OSSRH.
