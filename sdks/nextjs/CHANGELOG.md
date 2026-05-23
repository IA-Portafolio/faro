# Changelog — @iaportafolio/nextjs

Cambios del SDK de Next.js. Sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y [SemVer](https://semver.org/lang/es/). Cada release se publica a npm
empujando un tag `sdk-nextjs-v<semver>`.

## [Unreleased]

## [0.1.0] — 2026-05-22

### Added
- Instrumentación server-side y client-side para apps Next.js.
- Exports separados `./client` y `./server` para no leakear código de
  servidor al bundle del navegador.
- Captura automática de errores no manejados y promesas rechazadas en
  el cliente.
