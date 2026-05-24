//! Exposición Prometheus del propio backend.
//!
//! Faro mide a las apps; Prometheus+Grafana mide a Faro — no nos automonitoreamos
//! para no quedar ciegos cuando la propia ingesta esté caída. Este módulo monta
//! `/metrics` en formato Prometheus textual.
//!
//! **Reglas de cardinalidad** — las labels solo se usan para dimensiones acotadas
//! y conocidas de antemano:
//!
//! - `project` — ≈ nº de proyectos del tenant (~20)
//! - `signal` — `logs` | `traces` | `metrics`
//! - `outcome` — `accepted` | `rate_limited`
//! - `table` — ≈ 4 tablas ClickHouse
//! - `operation` — `insert` por ahora
//!
//! NUNCA labels con `trace_id`, `span_id`, `user_id`, `request_id` ni texto
//! libre de errores — cada combinación es una serie de tiempo en memoria, y
//! con cardinalidad alta Prometheus se cae. Si necesitas debuggear con
//! cardinalidad alta, eso es trabajo para logs y traces.
//!
//! Los nombres se centralizan en [`names`] para evitar typos que partan una
//! serie en dos.

use axum_prometheus::{
    metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle},
    PrometheusMetricLayer, PrometheusMetricLayerBuilder,
};

/// Nombres de métricas custom. Mantenidos en un solo lugar para que el sitio
/// que emite (`metrics::counter!(NAME)`) y el dashboard que consume hablen el
/// mismo idioma sin typos.
pub mod names {
    /// Counter — records aceptados o bloqueados por la ingesta.
    /// Labels: `project`, `signal` (logs|traces|metrics), `outcome` (accepted|rate_limited).
    pub const INGEST_RECORDS: &str = "faro_ingest_records_total";

    /// Counter — veces que un proyecto pegó el rate limit. Subset de INGEST_RECORDS
    /// con `outcome=rate_limited`, pero útil para alertar sin tener que
    /// sumar/filtrar en PromQL.
    /// Labels: `project`, `signal`.
    pub const RATE_LIMITED: &str = "faro_rate_limited_total";

    /// Histogram — duración de un INSERT batch a ClickHouse, en segundos.
    /// Label: `table` (faro.logs, faro.spans, faro.metrics, faro.monitor_results).
    pub const CH_INSERT_SECONDS: &str = "faro_clickhouse_insert_duration_seconds";

    /// Counter — filas confirmadas por ClickHouse.
    /// Label: `table`.
    pub const CH_ROWS_INSERTED: &str = "faro_clickhouse_rows_inserted_total";

    /// Counter — INSERTs que fallaron. El batch se descarta (no hay buffer durable),
    /// así que esta métrica es el canario de "estamos perdiendo datos".
    /// Labels: `table`, `operation`.
    pub const CH_ERRORS: &str = "faro_clickhouse_errors_total";
}

/// Instala el recorder global de Prometheus y devuelve el layer HTTP + el handle
/// para renderizar `/metrics`.
///
/// Debe llamarse **una sola vez** en el arranque del proceso, antes de que
/// cualquier `metrics::counter!` o macro similar se ejecute — son no-ops si
/// el recorder no está instalado.
///
/// El layer se aplica con `.layer(layer.clone())` a cada Router HTTP en cuyo
/// tráfico queramos ver métricas (api del dashboard + OTLP/HTTP). El tráfico
/// OTLP/gRPC pasa por tonic, no por axum, así que no se mide aquí — sus
/// counters viven en los handlers a través de [`names::INGEST_RECORDS`].
pub fn install() -> (PrometheusMetricLayer<'static>, PrometheusHandle) {
    // En axum-prometheus 0.7, el shortcut `PrometheusMetricLayer::pair()` instala
    // su propio recorder con defaults; cuando se quiere customizar el layer
    // (`.with_prefix` / `.with_ignore_patterns`) hay que proveer el recorder a
    // mano vía `with_metrics_from_fn`, que es donde lo instalamos en el global.
    PrometheusMetricLayerBuilder::new()
        .with_prefix("faro")
        // Excluimos el propio `/metrics` y `/healthz` — el primero loopea
        // (cada scrape generaría una entrada de `faro_http_requests_total`
        // contra `/metrics` y ensuciaría las series); el segundo es ruido
        // del healthcheck de docker / Caddy.
        .with_ignore_patterns(&["/metrics", "/healthz"])
        .with_metrics_from_fn(|| {
            PrometheusBuilder::new()
                .install_recorder()
                .expect("no se pudo instalar el recorder Prometheus")
        })
        .build_pair()
}
