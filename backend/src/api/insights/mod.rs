//! Endpoints de "insights" (hallazgos de negocio por servicio):
//!   GET /insights/service-dashboard            → resumen combinado por servicio
//!   GET /insights/revenue-impact               → impacto estimado en ingresos
//!   GET /insights/latency-funnel-impact        → latencia vs conversión del funnel
//!   GET /insights/web-vitals-conversion-impact → web vitals vs conversión
//!
//! Cruzan product events, funnels, errores y latencia para estimar impacto.
//!
//! Layout: un archivo por endpoint, todos en este módulo. Los helpers
//! compartidos (cálculo de conversion, formateo de summaries) viven en
//! `util` y los tests unitarios de los helpers también.

use axum::routing::get;
use axum::Router;

use crate::state::SharedState;

mod latency_funnel_impact;
mod revenue_impact;
mod service_dashboard;
mod util;
mod web_vitals_conversion_impact;

pub use latency_funnel_impact::{
    LatencyFunnelBucket, LatencyFunnelImpactQuery, LatencyFunnelImpactResult,
};
pub use revenue_impact::{RevenueImpactIssue, RevenueImpactQuery};
pub use service_dashboard::{
    ServiceDashboardInsight, ServiceDashboardIssue, ServiceDashboardQuery,
};
pub use web_vitals_conversion_impact::{
    WebVitalsConversionImpactQuery, WebVitalsConversionImpactResult,
};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route(
            "/insights/service-dashboard",
            get(service_dashboard::service_dashboard),
        )
        .route(
            "/insights/revenue-impact",
            get(revenue_impact::revenue_impact),
        )
        .route(
            "/insights/latency-funnel-impact",
            get(latency_funnel_impact::latency_funnel_impact),
        )
        .route(
            "/insights/web-vitals-conversion-impact",
            get(web_vitals_conversion_impact::web_vitals_conversion_impact),
        )
}
