# Changelog — @iaportafolio/nextjs

Cambios del SDK de Next.js. Sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y [SemVer](https://semver.org/lang/es/). Cada release se publica a npm
empujando un tag `sdk-nextjs-v<semver>`.

## [Unreleased]

### Added
- Alias `warning()` en el cliente browser (re-exportado desde `/client`).
- Auto-redacción del cliente browser: `scrubFields`, `scrubHeaders`,
  `scrubPatterns` con los mismos defaults que el resto de SDKs. Aplica a
  values de attributes y al `message`.
- Validación temprana en `initFaroClient()`: endpoint/token/service
  obligatorios lanzan `Error` claro en lugar de TypeError críptico (paridad
  cross-SDK; aplica tanto en cliente como en SSR para no tragar el error).
- Suite de tests del browser-core (10 tests) con stubs mínimos de globales
  del DOM (`window`, `document`, `navigator`, `location`, storage) — corre
  con `node --test`. Cubre las 6 invariantes mínimas: init inválido (×3),
  payload shape, queue cap, retry 5xx, captureException, close graceful
  + beforeSend + scrubbing.

### Changed
- El pipeline del cliente browser pasa por scrubbing **antes** de
  `beforeSend` (el hook recibe el wire ya saneado).

## [0.3.0] — 2026-05-23

### Changed
- **El RUM del navegador vuelve a vivir dentro de este paquete.** El código
  de Web Vitals, captura de errores, breadcrumbs y `FaroErrorBoundary` se
  movió a `src/browser-core.ts` y `src/browser-react.tsx`. Ya no hace falta
  instalar `@iaportafolio/browser`.
- `web-vitals` pasa a ser dependencia directa (antes lo traía el paquete
  browser).

### Removed
- Dependencia sobre `@iaportafolio/browser` — ese paquete queda
  **deprecado y unpublished** en npm. Migración: `npm uninstall
  @iaportafolio/browser && npm install @iaportafolio/nextjs@^0.3.0`. La
  API pública de `@iaportafolio/nextjs/client` no cambia.

### Why
- En la práctica el único consumidor de `@iaportafolio/browser` era este
  paquete. Mantener dos releases acoplados (peer dep + versionado en
  paralelo) era pura fricción sin beneficio real.

## [0.2.2] — 2026-05-23

### Fixed
- Build de `tsup --dts` con subpath exports: añadido `tsconfig.json`
  explícito con `moduleResolution: "Bundler"`, `@iaportafolio/node` y
  `@iaportafolio/browser` como `devDependencies` para resolver tipos, y
  marcados como `--external` en el bundle.
- `initFaroClient`: mezcla `??`/`||` que provocaba TS5076.

### Note
- `0.2.0` y `0.2.1` se publicaron sin `dist/` por un fallo del workflow y
  fueron **unpublished** en npm. Usa directamente `0.2.2` (o, mejor,
  `0.3.0`).

## [0.2.0] — 2026-05-23 *(unpublished)*

### Added
- RUM completo client-side delegando en el nuevo paquete
  `@iaportafolio/browser`: Web Vitals (LCP/CLS/INP/FCP/TTFB),
  `window.onerror`, `unhandledrejection`, breadcrumbs de clicks y
  navegaciones, `setUser`, `FaroErrorBoundary` y flush con
  `navigator.sendBeacon` al cerrar el tab.
- Auto-detección del `release` desde `NEXT_PUBLIC_VERCEL_GIT_COMMIT_SHA`.

## [0.1.0] — 2026-05-22

### Added
- Instrumentación server-side y client-side para apps Next.js.
- Exports separados `./client` y `./server` para no leakear código de
  servidor al bundle del navegador.
- Captura automática de errores no manejados y promesas rechazadas en
  el cliente.
