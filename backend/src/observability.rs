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

    /// Counter — records DESCARTADOS por la ingesta antes de llegar a ClickHouse,
    /// porque el canal mpsc del writer estaba lleno (backpressure: ClickHouse no
    /// drena al ritmo de ingesta). Es el canario directo de pérdida de datos por
    /// saturación — distinto de [`CH_ERRORS`], que cubre el fallo del INSERT ya
    /// encolado. Alertar `rate(faro_ingest_dropped_total[5m]) > 0`.
    /// Labels: `signal` (logs|traces|metrics|events|monitor_results), `reason`.
    pub const INGEST_DROPPED: &str = "faro_ingest_dropped_total";

    /// Gauge — filas encoladas ahora mismo en el canal mpsc del writer
    /// (`max_capacity - capacity`). Es el leading indicator: avisa que la cola se
    /// está llenando ANTES de que `try_send` empiece a descartar. Junto con
    /// [`INGEST_CHANNEL_CAPACITY`] permite calcular la ocupación en PromQL y
    /// alertar a, p.ej., 80%.
    /// Label: `signal`.
    pub const INGEST_CHANNEL_DEPTH: &str = "faro_ingest_channel_depth";

    /// Gauge — capacidad máxima del canal mpsc del writer (constante por señal).
    /// Sirve de denominador para la ocupación: `depth / capacity`.
    /// Label: `signal`.
    pub const INGEST_CHANNEL_CAPACITY: &str = "faro_ingest_channel_capacity";

    /// Counter — FILAS perdidas cuando el flush de un lote a ClickHouse falla y se
    /// descarta entero (sin reintento ni buffer durable). [`CH_ERRORS`] cuenta los
    /// INSERTs fallidos; esta métrica cuenta cuántas FILAS se fueron con ellos, que
    /// es lo que de verdad importa para dimensionar la pérdida.
    /// Label: `table`.
    pub const CH_ROWS_DROPPED: &str = "faro_clickhouse_rows_dropped_total";

    /// Counter — resultado del despacho de UNA notificación de alerta a UN destino.
    /// Sin esto, un webhook/canal/token roto se tragaba el fallo en silencio y el
    /// panel mostraba "firing" mientras la alerta nunca salía. Alertar sobre
    /// `outcome="failed"` o `"unroutable"`.
    /// Labels: `kind` (channel|webhook|telegram|unknown), `outcome` (sent|failed|unroutable).
    pub const ALERT_NOTIFY: &str = "faro_alert_notify_total";

    /// Counter — iteraciones completadas por cada worker en segundo plano. Es el
    /// heartbeat: `rate(faro_worker_runs_total[5m]) == 0` para un worker que
    /// debería tickear = worker muerto/colgado (panic, deadlock). Junto con
    /// [`WORKER_ERRORS`] da la salud de los 10 workers que antes eran invisibles.
    /// Label: `worker`.
    pub const WORKER_RUNS: &str = "faro_worker_runs_total";

    /// Counter — errores no fatales dentro del ciclo de un worker (query fallida,
    /// insert fallido, etc. — el worker sigue vivo). Label: `worker`.
    pub const WORKER_ERRORS: &str = "faro_worker_errors_total";
}

/// Intervalo de muestreo del gauge de profundidad de los canales de ingesta.
const CHANNEL_DEPTH_SAMPLE_SECS: u64 = 5;

/// Registra el descarte de UN record en la ingesta por canal lleno (backpressure).
///
/// Centralizado aquí para que todos los sitios `try_send` fallidos emitan la misma
/// serie sin typos en el nombre/labels. `signal` es una etiqueta de baja
/// cardinalidad y conocida de antemano (logs|traces|metrics|events|monitor_results);
/// NO se usa `project` a propósito: el canal es compartido entre proyectos, así que
/// el drop es una condición global de saturación, no atribuible a un proyecto.
pub fn record_ingest_drop(signal: &'static str) {
    metrics::counter!(
        names::INGEST_DROPPED,
        "signal" => signal,
        "reason" => "channel_full",
    )
    .increment(1);
}

/// Profundidad actual de un canal acotado: cuántos slots están ocupados.
/// Función pura para poder testear el cálculo sin un canal real.
fn channel_depth(max_capacity: usize, available_capacity: usize) -> usize {
    max_capacity.saturating_sub(available_capacity)
}

/// Publica el gauge de profundidad/capacidad de un canal de ingesta.
fn sample_channel<T>(tx: &tokio::sync::mpsc::Sender<T>, signal: &'static str) {
    let max = tx.max_capacity();
    let depth = channel_depth(max, tx.capacity());
    metrics::gauge!(names::INGEST_CHANNEL_DEPTH, "signal" => signal).set(depth as f64);
    metrics::gauge!(names::INGEST_CHANNEL_CAPACITY, "signal" => signal).set(max as f64);
}

/// Arranca una tarea periódica que muestrea la ocupación de los 5 canales de
/// ingesta y la publica como gauges Prometheus. Es el leading indicator de
/// saturación: permite alertar ANTES de que se empiecen a descartar records.
///
/// Debe llamarse después de [`install`] (el recorder global ya instalado) y antes
/// de que el writer tome los receivers — sólo lee `capacity()`/`max_capacity()` de
/// los senders, así que es seguro correrla en paralelo a la ingesta.
pub fn spawn_channel_depth_sampler(state: crate::state::SharedState) {
    use tokio::time::{interval, Duration, MissedTickBehavior};
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(CHANNEL_DEPTH_SAMPLE_SECS));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            sample_channel(&state.ingest.logs_tx, "logs");
            sample_channel(&state.ingest.spans_tx, "traces");
            sample_channel(&state.ingest.metrics_tx, "metrics");
            sample_channel(&state.ingest.events_tx, "events");
            sample_channel(&state.ingest.monitor_results_tx, "monitor_results");
        }
    });
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

#[cfg(test)]
mod tests {
    use super::channel_depth;

    #[test]
    fn channel_depth_is_max_minus_available() {
        // Canal vacío: profundidad 0.
        assert_eq!(channel_depth(32_768, 32_768), 0);
        // Mitad ocupado.
        assert_eq!(channel_depth(32_768, 16_384), 16_384);
        // Lleno: toda la capacidad ocupada.
        assert_eq!(channel_depth(32_768, 0), 32_768);
        // Defensivo: si `available` excede `max` (no debería), saturating evita underflow.
        assert_eq!(channel_depth(10, 12), 0);
    }
}
