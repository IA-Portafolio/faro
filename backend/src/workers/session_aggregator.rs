//! Session aggregator (goal 10.F.1).
//!
//! Cada [`Config::session_aggregator_interval_secs`] (default 5 min) el worker
//! toma una ventana hacia atrás de `session_aggregator_lookback_minutes`
//! (default 6h) de `faro.product_events` y mantiene `faro.product_sessions`.
//!
//! ## Dos brazos de sesionización (UNION ALL en una sola query)
//!
//! 1. **SDK manda `session_id`** → trust + `GROUP BY (project, actor, session_id)`.
//!    Convención que usan los SDKs nativos cuando manejan el ciclo de vida
//!    (PostHog, Amplitude, RUM browsers que rotan id tras inactividad o tras
//!    cambio de tab).
//!
//! 2. **`session_id` vacío** → sesionalización retroactiva. Para cada
//!    `(project, actor)` ordenamos por `timestamp` y cortamos cuando el gap
//!    entre dos eventos consecutivos excede `session_aggregator_gap_minutes`
//!    (default 30 — convención GA/Mixpanel). El actor efectivo es `distinct_id`
//!    si existe, si no `anonymous_id`. El `session_id` sintético se deriva
//!    de `cityHash64(project_id, actor_id, started_at)` → estable entre runs
//!    siempre que `started_at` (= min(timestamp) en la ventana) no cambie.
//!
//! ## ReplacingMergeTree(ended_at)
//!
//! `product_sessions` es `ReplacingMergeTree(ended_at)` con PK
//! `(project_id, session_id)`. Mientras una sesión sigue activa, cada tick
//! reinserta la fila con `ended_at` más reciente; el merge se queda con la
//! última versión. Idempotente.
//!
//! ## Link session -> trace (goal 10.F.3)
//!
//! Cada sesión materializa los `trace_id` no vacíos de sus eventos en
//! `trace_ids`/`trace_count`. Esto es necesario para sesiones sintéticas: como
//! los eventos originales tienen `session_id = ''`, una query posterior por
//! `session_id` no podría reconstruir qué traces sirvieron esa sesión.
//!
//! ## Caveat: drift de `started_at` en sesiones más largas que el lookback
//!
//! Si una sesión real arrancó hace más tiempo que `lookback_minutes`, el
//! worker no ve el primer evento y `started_at` (y por tanto el `session_id`
//! sintético) se sitúa al borde de la ventana. En la práctica 6h cubre
//! casi cualquier sesión legítima; subir el lookback si tu producto tiene
//! sesiones más largas. Para flujos donde el SDK manda `session_id` esto
//! no es problema — el id es estable independientemente del lookback.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::time::{interval, MissedTickBehavior};

use crate::api::params::ch_dt;
use crate::state::SharedState;
use crate::storage::ProductSessionRow;

pub fn start_session_aggregator(state: SharedState) {
    if !state.cfg.session_aggregator_enabled {
        tracing::info!("session_aggregator deshabilitado");
        return;
    }
    // Clampeos defensivos: tick < 30s no aporta nada (los inserts son async),
    // gap < 1 min rompe la convención, lookback < gap genera sesiones partidas
    // que el siguiente tick no puede reparar.
    let interval_secs = state.cfg.session_aggregator_interval_secs.max(30);
    let gap_minutes = state.cfg.session_aggregator_gap_minutes.max(1);
    let lookback_minutes = state
        .cfg
        .session_aggregator_lookback_minutes
        .max(gap_minutes);

    tracing::info!(
        every_secs = interval_secs,
        gap_minutes,
        lookback_minutes,
        "arrancando session_aggregator (goal 10.F.1)"
    );

    tokio::spawn(async move {
        // Espera inicial para que el ingest writer drene su primer batch y haya
        // algo que sesionalizar en el primer tick.
        tokio::time::sleep(Duration::from_secs(20)).await;

        let mut tick = interval(Duration::from_secs(interval_secs));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.tick().await; // descarta tick inmediato

        loop {
            tick.tick().await;
            metrics::counter!(crate::observability::names::WORKER_RUNS, "worker" => "session_aggregator").increment(1);
            let from = Utc::now() - chrono::Duration::minutes(lookback_minutes as i64);
            match aggregate_once(&state, from, gap_minutes).await {
                Ok(0) => tracing::debug!("session_aggregator: tick sin sesiones"),
                Ok(n) => tracing::info!(sessions = n, "session_aggregator: tick completo"),
                Err(e) => {
                    tracing::warn!(error = %e, "session_aggregator: tick falló");
                    metrics::counter!(crate::observability::names::WORKER_ERRORS, "worker" => "session_aggregator").increment(1);
                }
            }
        }
    });
}

#[derive(Debug, Deserialize)]
struct AggSessionRow {
    project_id: String,
    session_id: String,
    distinct_id: String,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    started_at: DateTime<Utc>,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    ended_at: DateTime<Utc>,
    page_count: u32,
    duration_seconds: u32,
    event_count: u32,
    pageview_count: u32,
    is_bounce: u8,
    is_engaged: u8,
    converted: u8,
    quality_score: f32,
    #[serde(default)]
    trace_ids: Vec<String>,
    trace_count: u32,
    #[serde(default)]
    source: String,
}

/// Runs one session aggregation pass. Public for integration tests and
/// maintenance jobs; the spawned worker calls the same function on each tick.
pub async fn aggregate_once(
    state: &SharedState,
    from: DateTime<Utc>,
    gap_minutes: u32,
) -> anyhow::Result<usize> {
    let from_s = ch_dt(from);
    let gap_secs_s = ((gap_minutes as u64) * 60).to_string();

    // Dos brazos en UNION ALL:
    //   1. Eventos con session_id provisto → GROUP BY (project, actor, session_id).
    //   2. Eventos sin session_id → ventana lagInFrame para detectar gap > umbral,
    //      luego sum() acumulado sobre is_new_session asigna sess_idx,
    //      finalmente GROUP BY (project, actor, sess_idx).
    //
    // El session_id sintético se construye con cityHash64 sobre started_at
    // (unix ts), de modo que el mismo conjunto de eventos siempre produce el
    // mismo id entre ticks (estable mientras el primer evento siga en la
    // ventana — ver caveat en el doc del módulo).
    let sql = session_aggregation_sql();

    let rows: Vec<AggSessionRow> = state
        .ch
        .select_with_params(
            sql,
            &[("from", from_s.as_str()), ("gap_secs", gap_secs_s.as_str())],
        )
        .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    let upserts: Vec<ProductSessionRow> = rows
        .into_iter()
        .map(|r| ProductSessionRow {
            project_id: r.project_id,
            session_id: r.session_id,
            distinct_id: r.distinct_id,
            started_at: r.started_at,
            ended_at: r.ended_at,
            page_count: r.page_count,
            duration_seconds: r.duration_seconds,
            event_count: r.event_count,
            pageview_count: r.pageview_count,
            is_bounce: r.is_bounce,
            is_engaged: r.is_engaged,
            converted: r.converted,
            quality_score: r.quality_score,
            trace_ids: r.trace_ids,
            trace_count: r.trace_count,
            source: if r.source.is_empty() {
                "web".to_string()
            } else {
                r.source
            },
        })
        .collect();

    let n = upserts.len();
    state.ch.insert("faro.product_sessions", &upserts).await?;
    Ok(n)
}

fn session_aggregation_sql() -> &'static str {
    "
        SELECT project_id, session_id, distinct_id, started_at, ended_at,
               page_count, duration_seconds, event_count, pageview_count,
               is_bounce, is_engaged, converted, quality_score, trace_ids,
               trace_count, source
        FROM (
            SELECT
                project_id,
                session_id,
                distinct_id,
                started_at,
                ended_at,
                pageview_count AS page_count,
                duration_seconds,
                event_count,
                pageview_count,
                toUInt8(event_count <= 1) AS is_bounce,
                toUInt8(event_count > 1) AS is_engaged,
                converted,
                toFloat32(
                    least(event_count / 10.0, 1.0) * 35.0
                    + least(duration_seconds / 300.0, 1.0) * 35.0
                    + if(converted = 1, 30.0, 0.0)
                ) AS quality_score,
                trace_ids,
                trace_count,
                source
            FROM (
                SELECT
                    project_id,
                    session_id,
                    actor_id AS distinct_id,
                    min(timestamp) AS started_at,
                    max(timestamp) AS ended_at,
                    toUInt32(count()) AS event_count,
                    toUInt32(countIf(event_name IN ('$pageview', 'page_view', '$screen', 'screen_view'))) AS pageview_count,
                    toUInt32(dateDiff('second', min(timestamp), max(timestamp))) AS duration_seconds,
                    toUInt8(countIf(event_name IN ('$conversion', 'checkout_completed', 'purchase', 'signup_completed', 'trial_started')) > 0) AS converted,
                    groupUniqArrayIf(trace_id, trace_id != '') AS trace_ids,
                    toUInt32(length(trace_ids)) AS trace_count,
                    any(toString(source)) AS source
                FROM (
                    SELECT
                        project_id,
                        if(distinct_id != '', distinct_id, anonymous_id) AS actor_id,
                        session_id,
                        event_name,
                        trace_id,
                        timestamp,
                        source
                    FROM faro.product_events
                    WHERE timestamp >= toDateTime64({from:DateTime64(9)}, 9)
                      AND session_id != ''
                      AND (distinct_id != '' OR anonymous_id != '')
                )
                GROUP BY project_id, actor_id, session_id
            )

            UNION ALL

            SELECT
                project_id,
                session_id,
                distinct_id,
                started_at,
                ended_at,
                pageview_count AS page_count,
                duration_seconds,
                event_count,
                pageview_count,
                toUInt8(event_count <= 1) AS is_bounce,
                toUInt8(event_count > 1) AS is_engaged,
                converted,
                toFloat32(
                    least(event_count / 10.0, 1.0) * 35.0
                    + least(duration_seconds / 300.0, 1.0) * 35.0
                    + if(converted = 1, 30.0, 0.0)
                ) AS quality_score,
                trace_ids,
                trace_count,
                source
            FROM (
                SELECT
                    project_id,
                    concat('s-', lower(hex(cityHash64(
                        toString(project_id),
                        actor_id,
                        toUInt64(toUnixTimestamp(min(timestamp)))
                    )))) AS session_id,
                    actor_id AS distinct_id,
                    min(timestamp) AS started_at,
                    max(timestamp) AS ended_at,
                    toUInt32(count()) AS event_count,
                    toUInt32(countIf(event_name IN ('$pageview', 'page_view', '$screen', 'screen_view'))) AS pageview_count,
                    toUInt32(dateDiff('second', min(timestamp), max(timestamp))) AS duration_seconds,
                    toUInt8(countIf(event_name IN ('$conversion', 'checkout_completed', 'purchase', 'signup_completed', 'trial_started')) > 0) AS converted,
                    groupUniqArrayIf(trace_id, trace_id != '') AS trace_ids,
                    toUInt32(length(trace_ids)) AS trace_count,
                    any(toString(source)) AS source
                FROM (
                    SELECT
                        project_id,
                        actor_id,
                        event_name,
                        trace_id,
                        timestamp,
                        source,
                        sum(if(prev_ts = toDateTime64(0, 9)
                               OR dateDiff('second', prev_ts, timestamp) > {gap_secs:UInt32}, 1, 0))
                            OVER (PARTITION BY project_id, actor_id ORDER BY timestamp
                                  ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS sess_idx
                    FROM (
                        SELECT
                            project_id,
                            actor_id,
                            event_name,
                            trace_id,
                            timestamp,
                            source,
                            lagInFrame(timestamp, 1, toDateTime64(0, 9))
                                OVER (PARTITION BY project_id, actor_id ORDER BY timestamp) AS prev_ts
                        FROM (
                            SELECT
                                project_id,
                                if(distinct_id != '', distinct_id, anonymous_id) AS actor_id,
                                event_name,
                                trace_id,
                                timestamp,
                                source
                            FROM faro.product_events
                            WHERE timestamp >= toDateTime64({from:DateTime64(9)}, 9)
                              AND session_id = ''
                              AND (distinct_id != '' OR anonymous_id != '')
                        )
                    )
                )
                GROUP BY project_id, actor_id, sess_idx
            )
        )
    "
}
