//! Endpoint de análisis de experimentos A/B:
//!   POST /experiments/analyze → dado un `flag_key` (feature flag) y un evento de
//!   conversión, calcula la conversión por variante en la ventana indicada.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::api::params::ch_dt;
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new().route("/experiments/analyze", post(analyze_experiment))
}

#[derive(Debug, Deserialize)]
pub struct ExperimentAnalyzeRequest {
    pub flag_key: String,
    pub conversion_event: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub to: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_minutes: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ExperimentVariantResult {
    pub variant: String,
    pub sample: u64,
    pub conversions: u64,
    pub conversion_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct ExperimentAnalyzeResponse {
    pub flag_key: String,
    pub conversion_event: String,
    pub project: String,
    pub from: String,
    pub to: String,
    pub variants: Vec<ExperimentVariantResult>,
    pub sample: u64,
    pub winner: String,
    pub absolute_delta: f64,
    pub relative_lift: f64,
    pub p_value: f64,
    pub ci95_low: f64,
    pub ci95_high: f64,
    pub summary: String,
}

#[derive(Debug, Clone, Copy)]
pub struct VariantCounts {
    pub sample: u64,
    pub conversions: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ExperimentStats {
    pub absolute_delta: f64,
    pub relative_lift: f64,
    pub p_value: f64,
    pub ci95_low: f64,
    pub ci95_high: f64,
}

pub fn compute_stats(control: VariantCounts, treatment: VariantCounts) -> ExperimentStats {
    if control.sample == 0 || treatment.sample == 0 {
        return ExperimentStats {
            absolute_delta: 0.0,
            relative_lift: 0.0,
            p_value: 1.0,
            ci95_low: 0.0,
            ci95_high: 0.0,
        };
    }
    let n_a = control.sample as f64;
    let n_b = treatment.sample as f64;
    let p_a = (control.conversions.min(control.sample) as f64) / n_a;
    let p_b = (treatment.conversions.min(treatment.sample) as f64) / n_b;
    let absolute_delta = p_b - p_a;
    let relative_lift = if p_a == 0.0 {
        0.0
    } else {
        absolute_delta / p_a
    };

    let ci_se = ((p_a * (1.0 - p_a) / n_a) + (p_b * (1.0 - p_b) / n_b)).sqrt();
    let pooled = (control.conversions.min(control.sample)
        + treatment.conversions.min(treatment.sample)) as f64
        / (n_a + n_b);
    let test_se = (pooled * (1.0 - pooled) * (1.0 / n_a + 1.0 / n_b)).sqrt();
    if ci_se == 0.0 || test_se == 0.0 {
        return ExperimentStats {
            absolute_delta,
            relative_lift,
            p_value: if absolute_delta == 0.0 { 1.0 } else { 0.0 },
            ci95_low: absolute_delta,
            ci95_high: absolute_delta,
        };
    }

    let z = absolute_delta / test_se;
    let p_value = 2.0 * (1.0 - normal_cdf(z.abs()));
    let margin = 1.96 * ci_se;
    ExperimentStats {
        absolute_delta,
        relative_lift,
        p_value: p_value.clamp(0.0, 1.0),
        ci95_low: absolute_delta - margin,
        ci95_high: absolute_delta + margin,
    }
}

async fn analyze_experiment(
    State(state): State<SharedState>,
    Json(req): Json<ExperimentAnalyzeRequest>,
) -> ApiResult<Json<ExperimentAnalyzeResponse>> {
    let flag_key = req.flag_key.trim();
    let conversion_event = req.conversion_event.trim();
    if flag_key.is_empty() {
        return Err(ApiError::BadRequest("flag_key no puede ser vacío".into()));
    }
    if conversion_event.is_empty() {
        return Err(ApiError::BadRequest(
            "conversion_event no puede ser vacío".into(),
        ));
    }

    let to = req.to.unwrap_or_else(Utc::now);
    let from = req.from.unwrap_or_else(|| match req.last_minutes {
        Some(m) if m > 0 => to - Duration::minutes(m),
        _ => to - Duration::days(7),
    });
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }
    let project = req.project.unwrap_or_default();
    let project_value = if project.is_empty() {
        "all"
    } else {
        project.as_str()
    };

    let from_s = ch_dt(from);
    let to_s = ch_dt(to);
    let project_clause_exp = if project.is_empty() {
        ""
    } else {
        " AND project_id = {project:String}"
    };
    let project_clause_pe = if project.is_empty() {
        ""
    } else {
        " AND pe.project_id = {project:String}"
    };

    let sql = format!(
        "WITH \
           exposures AS ( \
             SELECT project_id, \
                    distinct_id, \
                    argMin(JSONExtractString(properties, 'variant'), timestamp) AS variant, \
                    min(timestamp) AS exposed_at \
             FROM faro.product_events \
             WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
               AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
               AND event_name = '$feature_exposure' \
               AND JSONExtractString(properties, 'flag_key') = {{flag_key:String}}{project_clause_exp} \
             GROUP BY project_id, distinct_id \
           ), \
           samples AS ( \
             SELECT variant, toUInt64(uniqExact(tuple(project_id, distinct_id))) AS sample \
             FROM exposures \
             WHERE variant IN ('A', 'B') \
             GROUP BY variant \
           ), \
           conversions AS ( \
             SELECT e.variant AS variant, toUInt64(uniqExact(tuple(e.project_id, e.distinct_id))) AS conversions \
             FROM exposures AS e \
             INNER JOIN faro.product_events AS pe \
               ON pe.project_id = e.project_id AND pe.distinct_id = e.distinct_id \
             WHERE e.variant IN ('A', 'B') \
               AND pe.event_name = {{conversion_event:String}} \
               AND pe.timestamp >= e.exposed_at \
               AND pe.timestamp <  toDateTime64({{to:DateTime64(9)}}, 9){project_clause_pe} \
             GROUP BY e.variant \
           ) \
         SELECT s.variant AS variant, s.sample AS sample, ifNull(c.conversions, 0) AS conversions \
         FROM samples AS s \
         LEFT JOIN conversions AS c USING (variant) \
         ORDER BY variant"
    );

    let mut params = vec![
        ("from", from_s.as_str()),
        ("to", to_s.as_str()),
        ("flag_key", flag_key),
        ("conversion_event", conversion_event),
    ];
    if !project.is_empty() {
        params.push(("project", project_value));
    }

    #[derive(Debug, Deserialize)]
    struct Row {
        variant: String,
        sample: u64,
        conversions: u64,
    }

    let rows: Vec<Row> = state.ch.select_with_params(&sql, &params).await?;
    let mut a = VariantCounts {
        sample: 0,
        conversions: 0,
    };
    let mut b = VariantCounts {
        sample: 0,
        conversions: 0,
    };
    for row in rows {
        let counts = VariantCounts {
            sample: row.sample,
            conversions: row.conversions.min(row.sample),
        };
        match row.variant.as_str() {
            "A" => a = counts,
            "B" => b = counts,
            _ => {}
        }
    }

    let stats = compute_stats(a, b);
    let variants = vec![variant_result("A", a), variant_result("B", b)];
    let sample = a.sample + b.sample;
    let winner = if stats.absolute_delta > 0.0 {
        "B"
    } else if stats.absolute_delta < 0.0 {
        "A"
    } else {
        "tie"
    }
    .to_string();

    Ok(Json(ExperimentAnalyzeResponse {
        flag_key: flag_key.to_string(),
        conversion_event: conversion_event.to_string(),
        project: project_value.to_string(),
        from: from.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        to: to.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        variants,
        sample,
        winner,
        absolute_delta: stats.absolute_delta,
        relative_lift: stats.relative_lift,
        p_value: stats.p_value,
        ci95_low: stats.ci95_low,
        ci95_high: stats.ci95_high,
        summary: summary_text(stats, sample),
    }))
}

fn variant_result(variant: &str, counts: VariantCounts) -> ExperimentVariantResult {
    let conversion_rate = if counts.sample == 0 {
        0.0
    } else {
        counts.conversions as f64 / counts.sample as f64
    };
    ExperimentVariantResult {
        variant: variant.to_string(),
        sample: counts.sample,
        conversions: counts.conversions,
        conversion_rate,
    }
}

fn summary_text(stats: ExperimentStats, sample: u64) -> String {
    let better = if stats.relative_lift >= 0.0 {
        "mejor"
    } else {
        "peor"
    };
    format!(
        "Variante B convierte {:.1}% {} (p={:.3}, sample={}, 95% CI: {:.1}% - {:.1}%)",
        stats.relative_lift.abs() * 100.0,
        better,
        stats.p_value,
        sample,
        stats.ci95_low * 100.0,
        stats.ci95_high * 100.0,
    )
}

fn normal_cdf(x: f64) -> f64 {
    // Abramowitz-Stegun 7.1.26 approximation. Error < 7.5e-8 for CDF use.
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let erf = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();
    0.5 * (1.0 + sign * erf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_lift_p_value_and_ci_for_two_proportions() {
        let stats = compute_stats(
            VariantCounts {
                sample: 4_100,
                conversions: 410,
            },
            VariantCounts {
                sample: 4_100,
                conversions: 582,
            },
        );

        assert!((stats.absolute_delta - 0.041951).abs() < 0.0005);
        assert!((stats.relative_lift - 0.41951).abs() < 0.005);
        assert!(stats.p_value < 0.001, "p_value={}", stats.p_value);
        assert!(stats.ci95_low > 0.025 && stats.ci95_low < 0.030);
        assert!(stats.ci95_high > 0.055 && stats.ci95_high < 0.060);
    }

    #[test]
    fn zero_samples_return_neutral_stats() {
        let stats = compute_stats(
            VariantCounts {
                sample: 0,
                conversions: 0,
            },
            VariantCounts {
                sample: 10,
                conversions: 2,
            },
        );

        assert_eq!(stats.absolute_delta, 0.0);
        assert_eq!(stats.relative_lift, 0.0);
        assert_eq!(stats.p_value, 1.0);
        assert_eq!(stats.ci95_low, 0.0);
        assert_eq!(stats.ci95_high, 0.0);
    }
}
