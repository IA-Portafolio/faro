# Changelog

Todos los cambios notables del **core** de Faro (backend + frontend) se
documentan acá. Los SDKs llevan su propio versionado independiente vía
tags `sdk-<lang>-v<semver>` y releases automáticas en GitHub.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y el proyecto usa [Semantic Versioning](https://semver.org/lang/es/).

## [Unreleased]

### Added
- SDK `@iaportafolio/nextjs` con instrumentación server y client.
  El RUM del navegador (Web Vitals, breadcrumbs, `FaroErrorBoundary`)
  vive dentro del propio paquete desde `0.3.0` — antes fue un paquete
  aparte `@iaportafolio/browser` que se fusionó (ver
  [sdks/nextjs/CHANGELOG](sdks/nextjs/CHANGELOG.md)).
- Backend: scaffolding de **OpenAPI con `utoipa`** + Swagger UI montado en
  el enrutador principal (#17).
- Backend: scaffolding de **self-observability** — el backend exporta sus
  propios logs/trazas/métricas vía OTLP a sí mismo (#18).
- Backend: **contrato de versión SDK ↔ backend** vía header
  `Faro-Protocol-Version`, para detectar drift entre cliente y servidor
  antes de ingerir datos malformados (#19).
- Deploy: tabla `faro.integrations` añadida y `curl` instalado en la
  imagen del backend para healthchecks dentro del contenedor.
- Workflow `ci.yml` con `cargo fmt/clippy/test`, `npm run check/build` y
  validación de `docker-compose`.
- Configuración Dependabot para 9 ecosistemas (GitHub Actions, Cargo, npm
  x3, pip, gomod, gradle, docker).
- `SECURITY.md` con política de reporte de vulnerabilidades.
- `docs/development.md` y `docs/deployment.md`.
- `rust-toolchain.toml`, `.nvmrc` y `.editorconfig` para pinear toolchain.
- Templates de issue/PR y `CODEOWNERS`.

### Changed
- `backend/Cargo.toml`: licencia `MIT` → `LicenseRef-Proprietary` (alineado
  con el `LICENSE` raíz que es propietaria) y `publish = false` para evitar
  empujes accidentales a crates.io.
- `.gitignore` cubre ahora los artefactos de los 7 ecosistemas de SDKs y
  bloquea explícitamente `.env.prod` / `.env.*.local`.
- Scope npm renombrado de `@faro/*` a `@iaportafolio/*` (el scope `@faro`
  estaba tomado en el registry).
- Deploy: el `rsync` del runner excluye `.gradle/` — los permisos del host
  bloqueaban `--delete` sobre ese directorio.

### Fixed
- SDK Kotlin: publicación a Maven Central migrada al Central Portal nuevo
  (`com.gradleup.nmcp`); el OSSRH legacy ya no acepta uploads para
  namespaces nuevos.

## [0.1.0] — 2026-05-22

Primera versión etiquetable del core. Lo incluido aquí es el estado al
momento de crear este changelog; cambios anteriores quedan en el historial
de git (`git log`).

### Added
- Backend Rust (axum + tokio) con dos listeners HTTP:
  - `:8080` API REST + SSE para el dashboard y endpoint nativo de ingesta.
  - `:4318` receptores OTLP/HTTP+JSON para logs, traces, métricas.
- Workers en background: writer batched a ClickHouse, runner de monitores
  HTTP, evaluador de alertas, indexador de error groups.
- Almacenamiento en ClickHouse con esquemas para logs, spans, métricas,
  error events, monitores y reglas de alerta.
- Frontend SvelteKit con vistas de dashboard, logs (live tail), traces,
  métricas, errores, monitores y reglas.
- Webhook notifications (Slack/Discord/genérico) para incidentes.
- Docker Compose para dev (`docker-compose.yml`) y prod
  (`docker-compose.prod.yml`).
- Auto-deploy via self-hosted runner en `infra-iaportafolio` con
  migraciones idempotentes y healthcheck via dominio público.
- SDKs en Node, Python, Go, Flutter, Kotlin y Expo con publicación
  automatizada en sus registries respectivos.
