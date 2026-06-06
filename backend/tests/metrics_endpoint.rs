//! Regresión: las métricas custom (`metrics::counter!/gauge!/histogram!`) DEBEN
//! renderizar en la salida Prometheus de `/metrics`.
//!
//! Guard contra el split de versiones del crate `metrics` vs el recorder que
//! instala `axum-prometheus` (ver el comentario en `Cargo.toml`). Cada versión
//! mayor de `metrics` tiene su propio recorder global estático: si el app emite
//! con una versión y el recorder instalado es otra, los counters custom son
//! no-ops silenciosos y SÓLO renderizan las métricas HTTP del layer. Este test
//! emite un counter custom real (vía `observability::record_ingest_drop`, que
//! usa el mismo macro que el código de ingesta) y exige verlo en el render.

use faro::observability;

#[test]
fn custom_metrics_render_in_prometheus_output() {
    // `install()` instala el recorder global Prometheus y devuelve el handle.
    // Sólo puede instalarse una vez por proceso; este test es el único que lo
    // hace en su binario de integración.
    let (_layer, handle) = observability::install();

    // Emitimos un counter custom por el mismo camino que la ingesta real.
    observability::record_ingest_drop("logs");

    let rendered = handle.render();
    assert!(
        rendered.contains("faro_ingest_dropped_total"),
        "el counter custom `faro_ingest_dropped_total` no aparece en /metrics — \
         recorder split entre `metrics` y `axum-prometheus`. Output:\n{rendered}"
    );
}
