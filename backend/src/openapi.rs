//! Documento OpenAPI generado por `utoipa`.
//!
//! Empezamos con scaffolding: el documento existe, se sirve en
//! `/api/v1/openapi.json` y la UI Swagger en `/docs`, pero la
//! anotación handler-por-handler con `#[utoipa::path]` es trabajo
//! mecánico que se irá haciendo en PRs posteriores (uno por
//! sub-router: logs, traces, metrics, errors, etc.).
//!
//! Ver `docs/adr/0006-openapi-utoipa.md` para el porqué y el plan.

use utoipa::OpenApi;

use crate::api::dashboard::DashboardSummary;
use crate::api::services::Service;

/// Punto de entrada del documento OpenAPI. Lista los handlers anotados
/// (vacío por ahora) y los schemas exportados. A medida que se anoten
/// handlers con `#[utoipa::path]`, se referencian acá en `paths(...)`.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Faro API",
        version = env!("CARGO_PKG_VERSION"),
        description = "API REST y SSE para el dashboard de Faro. \
                       La superficie OTLP/HTTP vive en otro listener (`:4318`) \
                       y sigue el contrato estándar de OpenTelemetry.",
        license(
            name = "Proprietary",
            identifier = "LicenseRef-Proprietary"
        )
    ),
    servers(
        (url = "http://localhost:8080", description = "Local dev"),
        (url = "https://faro.iaportafolio.com", description = "Producción")
    ),
    paths(
        // TODO: añadir aquí los `crate::api::<mod>::<handler>` a medida
        // que se anoten con `#[utoipa::path]`. Plan en ADR-0006.
    ),
    components(
        schemas(
            DashboardSummary,
            Service,
        )
    ),
    tags(
        (name = "dashboard", description = "Resumen agregado para el dashboard principal"),
        (name = "logs", description = "Logs estructurados y live tail"),
        (name = "traces", description = "Trazas distribuidas (OTLP spans)"),
        (name = "metrics", description = "Métricas OTLP con bucketing por tiempo"),
        (name = "errors", description = "Issues agrupados por fingerprint"),
        (name = "monitors", description = "Chequeos HTTP sintéticos"),
        (name = "alerts", description = "Reglas e incidentes de alertas"),
        (name = "services", description = "Servicios visibles en los datos"),
        (name = "projects", description = "Proyectos / tenants lógicos"),
        (name = "users", description = "Gestión de usuarios del dashboard")
    )
)]
pub struct ApiDoc;
