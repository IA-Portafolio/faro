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
use crate::api::funnels::{FunnelRequest, FunnelResult, FunnelStep};
use crate::api::services::{Service, ServiceMap, ServiceMapEdge, ServiceMapNode};
use crate::ingest::events::{IngestPayload, RawEvent};

/// Punto de entrada del documento OpenAPI. Lista los handlers anotados con
/// `#[utoipa::path]`. El primer batch documentado son los del 6º pilar
/// (product analytics) — el resto se irá sumando en PRs siguientes.
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
        // 6º pilar: product analytics. Solo los handlers cuyos request/response
        // tienen `ToSchema` derivable hoy. El resto (events query, cohorts,
        // product-users, etc.) requiere primero agregar `ToSchema` en cascada
        // a structs como `Range`, `EventQuery`, `CohortDefinition`, `CohortRow`,
        // `ProductUserSummary`, `ProductEventRow`, etc. — trabajo mecánico
        // pero amplio (deuda transversal del repo, ADR-0006).
        crate::ingest::events::ingest_events,
        crate::api::funnels::compute,
    ),
    components(
        schemas(
            DashboardSummary,
            Service,
            ServiceMap,
            ServiceMapNode,
            ServiceMapEdge,
            IngestPayload,
            RawEvent,
            FunnelRequest,
            FunnelResult,
            FunnelStep,
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
        (name = "users", description = "Gestión de usuarios del dashboard"),
        (name = "events", description = "Product events (6º pilar): ingesta y consulta"),
        (name = "funnels", description = "Conversión por pasos con windowFunnel"),
        (name = "cohorts", description = "Segmentación de usuarios persistida"),
        (name = "product-users", description = "Perfil de usuario unificado multi-device")
    )
)]
pub struct ApiDoc;
