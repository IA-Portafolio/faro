# Changelog — @iaportafolio/node

Cambios del SDK de Node.js. Sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y [SemVer](https://semver.org/lang/es/). Cada release se publica a npm
empujando un tag `sdk-node-v<semver>`.

## [Unreleased]

## [0.1.0] — 2026-05-22

### Added
- Cliente HTTP ligero para enviar logs y excepciones al endpoint nativo
  de Faro (`/api/v1/ingest/logs`).
- Build dual ESM + CJS via `tsup`, types `.d.ts` incluidos.
- Publicación con `npm provenance`.
