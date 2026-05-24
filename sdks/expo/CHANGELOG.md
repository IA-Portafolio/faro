# Changelog — @iaportafolio/expo

Cambios del SDK de Expo / React Native. Sigue
[Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y
[SemVer](https://semver.org/lang/es/). Cada release se publica a npm
empujando un tag `sdk-expo-v<semver>`.

## [Unreleased]

### Added
- Alias `warning()` (paridad cross-SDK con `WARNING`).
- Auto-redacción: `scrubFields`, `scrubHeaders`, `scrubPatterns` con los
  mismos defaults que el resto de SDKs.
- Hook `beforeSend(event) → event | null` post-scrub.
- **Persistencia en AsyncStorage** (opt-in vía peer dep): si
  `@react-native-async-storage/async-storage` está instalado, la cola se
  serializa a disco al pasar a background o ante fatal exceptions, y se
  drena en el siguiente `init()`. Sobrevive a kills agresivos del SO
  (swipe en task switcher, OOM). TTL 24 h default, opciones
  `persistence: { ttlMs, maxBytes, key }`. Pasa `persistence: false`
  para desactivar.
- Listener `AppState 'background'/'inactive'` → flush + persist.
- Validación temprana en `init()`: endpoint/token/service obligatorios
  lanzan `Error` claro en lugar de TypeError críptico (paridad cross-SDK).
- Suite de tests Node-side (10 tests entre `client.test.mjs` y
  `expo.test.mjs`) con `createRequire` para simular el resolver de Metro.
  Cubre las 6 invariantes mínimas: init inválido (×3), payload shape,
  queue cap, retry 5xx, `captureException` (shape OTel + isFatal),
  `close()` graceful + beforeSend + scrubbing.

### Changed
- `close()` ahora desregistra el listener de `AppState` y persiste lo
  que quede tras el flush final.
- Timer interno usa `unref()` cuando está disponible (paridad con SDK
  Node). En React Native es no-op; en Node permite que el proceso salga
  sin esperar al timer y resuelve crashes de libuv en tests Windows.

## [0.1.0] — 2026-05-22

### Added
- Cliente HTTP para Expo / React Native (uses `fetch`, sin deps nativas).
- Hook de error boundary para capturar errores de componentes.
