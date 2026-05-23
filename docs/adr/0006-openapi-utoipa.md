# ADR-0006: OpenAPI spec autogenerada con utoipa

- **Estado**: Accepted
- **Fecha**: 2026-05-23
- **Autores**: @victalejo

## Contexto

La API REST del backend está documentada como una **tabla en
prosa** en el README (sección "Superficie de la API REST"). Esa tabla
es manual: se desincroniza con cada PR que añada/cambie un endpoint, y
no permite generar clientes tipados automáticamente. Los SDKs que
publicamos están hechos a mano contra la misma documentación, lo que
multiplica el costo de cada cambio de contrato.

Tenemos ~30 endpoints REST en `backend/src/api/*` con request/response
types ya definidos como structs Rust con `serde::{Serialize, Deserialize}`.
Es la mitad del trabajo necesario para tener una spec OpenAPI viva.

## Decisión

Generamos la spec OpenAPI **desde el código Rust** usando
[`utoipa`](https://docs.rs/utoipa). El documento se sirve en
`/api/v1/openapi.json` y una UI Swagger en `/docs`. El documento es la
**fuente de verdad** del contrato; la tabla del README pasa a ser
un resumen narrativo.

Mecanismo:

- Cada struct request/response anota `#[derive(ToSchema)]`.
- Cada handler anota `#[utoipa::path(...)]` con método, path,
  parámetros y respuestas posibles.
- `crate::openapi::ApiDoc` es el `#[derive(OpenApi)]` raíz que lista
  paths y schemas.
- `utoipa-swagger-ui` monta `/docs` con el spec embebido.

## Alternativas consideradas

- **Mantener la tabla a mano en el README** — estado actual. Costo de
  mantenimiento creciente y cero garantías de exactitud.
- **Escribir un `openapi.yaml` a mano** — desacopla del código pero
  introduce un segundo lugar que también se desincroniza.
- **`aide` / `axum-openapi3`** — alternativas válidas; `utoipa` se
  elige por ser el más maduro del ecosistema y por su integración
  oficial con axum vía `utoipa-axum`.
- **`paperclip`** — primario para actix, no para axum.

## Consecuencias

### Positivas

- **Generación de clientes tipados** vía `openapi-generator` o
  `openapi-typescript` — los SDKs en `sdks/{node,nextjs,expo}` pueden
  reducir boilerplate.
- **Pruebas de contrato**: el `openapi.json` puede pasarse a Spectral /
  Schemathesis en CI para detectar breaking changes y validar
  respuestas reales.
- **Swagger UI en `/docs`** sirve como documentación viva para
  cualquiera que toque la API (incluyendo nosotros mismos en 6 meses).
- El derive empuja a documentar cada response con su shape exacto, no
  con "devuelve JSON".

### Negativas / costo asumido

- Cada handler nuevo requiere ~6 líneas extra de macro `#[utoipa::path]`.
- El tipo `crate::api::params::Range` usa custom deserializers; hay que
  proveer `IntoParams` manualmente o aceptar que el schema sea menos
  preciso ahí (mejor que nada).
- Tres crates nuevos en la árbol de dependencias (`utoipa`,
  `utoipa-axum`, `utoipa-swagger-ui`), ~1.5 MB compilados.

### Trabajo de seguimiento

Esta ADR cierra con el **scaffolding** del PR
`feat/openapi-utoipa`:

- ✅ Crates añadidos a `Cargo.toml`.
- ✅ `crate::openapi::ApiDoc` con `#[derive(OpenApi)]` listando
  tags y server URLs.
- ✅ `/api/v1/openapi.json` y `/docs` montados y excluidos del
  middleware de auth.
- ✅ `DashboardSummary` y `Service` anotados con `ToSchema` como
  prueba de viabilidad.

Queda por hacer (en PRs incrementales, uno por sub-router):

- [ ] Anotar `logs::*` (5 endpoints).
- [ ] Anotar `traces::*` (2 endpoints).
- [ ] Anotar `metrics::*` (2 endpoints).
- [ ] Anotar `errors::*` (3 endpoints).
- [ ] Anotar `monitors::*` (6 endpoints).
- [ ] Anotar `alerts::*` (5 endpoints).
- [ ] Anotar `services::*`, `dashboard::*`, `projects::*`, `users::*`.
- [ ] `IntoParams` manual para `api::params::Range`.
- [ ] CI: pasar el spec por `spectral lint`.
- [ ] CI: validar que `openapi.json` no rompe contratos vs la versión
  anterior (`oasdiff`).
- [ ] Regenerar SDKs npm desde el spec en lugar de a mano (PR aparte).
