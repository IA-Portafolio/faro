//! Helpers compartidos por los 4 endpoints de insights: cálculo de conversion
//! rate, dropped points, y formateo de summaries en lenguaje natural.
//!
//! No tienen dependencias de ClickHouse — son funciones puras, lo que permite
//! testearlas sin levantar el stack de integración.

/// Evento default de checkout para los cálculos de revenue y funnel impact.
/// Compartido por revenue_impact, latency_funnel_impact (como `funnel_to`),
/// web_vitals_conversion_impact y service_dashboard (como `funnel_to`).
pub const DEFAULT_CHECKOUT_EVENT: &str = "checkout_completed";

/// AOV default en USD (asume e-commerce). Solo lo usa `revenue_impact`.
pub const DEFAULT_AVERAGE_ORDER_VALUE: f64 = 100.0;

/// Evento "inicio de funnel" default. Lo usan `latency_funnel_impact` y
/// `service_dashboard`.
pub const DEFAULT_FUNNEL_FROM_EVENT: &str = "checkout_started";

/// Threshold p95 default (en ms) para clasificar buckets "lentos" en
/// `latency_funnel_impact`.
pub const DEFAULT_LATENCY_THRESHOLD_MS: u32 = 2_000;

/// Métrica de web vitals default (LCP / FID / CLS / INP).
pub const DEFAULT_WEB_VITAL_METRIC: &str = "LCP";

/// Threshold default (en ms) para clasificar sesiones "lentas" en
/// `web_vitals_conversion_impact` — 4s es el umbral "good" de LCP.
pub const DEFAULT_WEB_VITAL_THRESHOLD_MS: f64 = 4_000.0;

/// Evento de pageview default que une la sesión de producto con la sesión
/// de web vitals.
pub const DEFAULT_PAGEVIEW_EVENT: &str = "$pageview";

/// Tamaño de bucket default (en minutos) para `latency_funnel_impact`.
pub const DEFAULT_BUCKET_MINUTES: u32 = 60;

/// Tope máximo para el bucket de `latency_funnel_impact`. Lo aplica el
/// backend vía `clamp` para que un cliente no pida buckets absurdamente
/// grandes que revienten memoria.
pub const MAX_BUCKET_MINUTES: u32 = 24 * 60;

/// Ratio de conversión de un cohort: `completed / affected` ∈ [0, 1].
/// Devuelve 0 si no hay sesiones afectadas (no es un error de medición).
pub(crate) fn conversion_rate(affected_sessions: u64, sessions_without_checkout: u64) -> f64 {
    if affected_sessions == 0 {
        return 0.0;
    }
    let completed = affected_sessions.saturating_sub(sessions_without_checkout);
    completed as f64 / affected_sessions as f64
}

/// Revenue perdido estimado: gap de conversión × sesiones afectadas × AOV.
/// Clampea a ≥ 0 (un issue que convierte mejor que la baseline no genera
/// pérdida — sería ganancia, que no medimos en este endpoint).
pub(crate) fn estimated_lost_revenue(
    baseline_conversion_rate: f64,
    issue_conversion_rate: f64,
    affected_sessions: u64,
    average_order_value: f64,
) -> f64 {
    let gap = (baseline_conversion_rate - issue_conversion_rate).max(0.0);
    gap * affected_sessions as f64 * average_order_value
}

/// Ratio de funnel: `completed / started` ∈ [0, 1].
pub(crate) fn funnel_conversion_rate(started: u64, completed: u64) -> f64 {
    if started == 0 {
        return 0.0;
    }
    completed as f64 / started as f64
}

/// Diferencia de conversion rates en puntos porcentuales (× 100). Clampea a
/// ≥ 0 porque un "drop" negativo significa mejora, no es lo que reporta
/// este endpoint.
pub(crate) fn conversion_drop_points(
    baseline_conversion_rate: f64,
    slow_conversion_rate: f64,
) -> f64 {
    (baseline_conversion_rate - slow_conversion_rate).max(0.0) * 100.0
}

pub(crate) fn latency_funnel_summary(
    span_name: &str,
    latency_threshold_ms: u32,
    drop_points: f64,
) -> String {
    let threshold = if latency_threshold_ms % 1_000 == 0 {
        format!("{}s", latency_threshold_ms / 1_000)
    } else {
        format!("{:.1}s", latency_threshold_ms as f64 / 1_000.0)
    };
    format!(
        "Cuando {span_name} p95 supera {threshold}, el funnel checkout cae {:.0} puntos.",
        drop_points
    )
}

pub(crate) fn web_vitals_conversion_summary(
    metric: &str,
    threshold_ms: f64,
    drop_points: f64,
) -> String {
    format!(
        "Los usuarios con {metric} > {} convierten {:.0} puntos menos.",
        threshold_label(threshold_ms),
        drop_points
    )
}

fn threshold_label(threshold_ms: f64) -> String {
    let seconds = threshold_ms / 1_000.0;
    if seconds.fract().abs() < f64::EPSILON {
        format!("{seconds:.0}s")
    } else {
        format!("{seconds:.1}s")
    }
}

pub(crate) fn service_dashboard_summary(
    service: &str,
    funnel_from: &str,
    funnel_to: &str,
    started_events: u64,
    completed_events: u64,
    linked_error_sessions: u64,
    failed_sessions: u64,
    span_name: &str,
    p95_latency_ms: f64,
) -> String {
    let conversion = funnel_conversion_rate(started_events, completed_events) * 100.0;
    format!(
        "{service}: {completed_events}/{started_events} {funnel_to} desde {funnel_from} ({conversion:.1}%). {linked_error_sessions} de {failed_sessions} sesiones fallidas tienen errores linkeados; p95 {span_name}: {p95_latency_ms:.0}ms."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_rate_counts_completed_sessions_over_affected_sessions() {
        let rate = conversion_rate(12, 5);

        assert!((rate - 7.0 / 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn conversion_rate_is_zero_when_there_are_no_affected_sessions() {
        assert_eq!(conversion_rate(0, 0), 0.0);
    }

    #[test]
    fn estimated_lost_revenue_uses_positive_conversion_gap() {
        let lost = estimated_lost_revenue(0.71, 0.60, 1_247, 100.0);

        assert!((lost - 13_717.0).abs() < 0.0001);
    }

    #[test]
    fn estimated_lost_revenue_clamps_when_issue_outperforms_baseline() {
        let lost = estimated_lost_revenue(0.40, 0.60, 1_247, 100.0);

        assert_eq!(lost, 0.0);
    }

    #[test]
    fn funnel_conversion_rate_counts_completed_over_started() {
        let rate = funnel_conversion_rate(10, 7);

        assert!((rate - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn funnel_conversion_rate_is_zero_when_no_one_started() {
        assert_eq!(funnel_conversion_rate(0, 7), 0.0);
    }

    #[test]
    fn conversion_drop_points_clamps_negative_drop() {
        assert_eq!(conversion_drop_points(0.50, 0.75), 0.0);
    }

    #[test]
    fn conversion_drop_points_returns_percentage_points() {
        let points = conversion_drop_points(0.71, 0.59);

        assert!((points - 12.0).abs() < 0.0001);
    }

    #[test]
    fn latency_funnel_summary_formats_threshold_and_drop() {
        let text = latency_funnel_summary("/api/checkout", 2_000, 12.0);

        assert_eq!(
            text,
            "Cuando /api/checkout p95 supera 2s, el funnel checkout cae 12 puntos."
        );
    }

    #[test]
    fn web_vitals_conversion_summary_formats_threshold_and_drop() {
        let text = web_vitals_conversion_summary("LCP", 4_000.0, 13.0);

        assert_eq!(
            text,
            "Los usuarios con LCP > 4s convierten 13 puntos menos."
        );
    }

    #[test]
    fn service_dashboard_summary_links_events_errors_and_latency() {
        let text = service_dashboard_summary(
            "checkout",
            "checkout_started",
            "checkout_completed",
            12_453,
            8_901,
            18,
            3_552,
            "/api/checkout",
            230.0,
        );

        assert_eq!(
            text,
            "checkout: 8901/12453 checkout_completed desde checkout_started (71.5%). 18 de 3552 sesiones fallidas tienen errores linkeados; p95 /api/checkout: 230ms."
        );
    }
}
