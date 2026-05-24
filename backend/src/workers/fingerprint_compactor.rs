//! Compactador de fingerprints: agrupa errores semánticamente equivalentes pero con
//! fingerprints distintos.
//!
//! El [`fingerprint`](crate::fingerprint) actual es hash exacto. Dos
//! `NullPointerException` en el mismo método pero con `$$Lambda$123` vs
//! `$$Lambda$987` (sufijos anónimos que rotan entre rebuilds) producen fingerprints
//! distintos y el dashboard los lista como dos issues. Operacionalmente es el mismo.
//!
//! Este worker corre cada [`Config::fingerprint_compactor_interval_secs`] minutos
//! (default 30) y:
//!
//! 1. Carga al arrancar el set de fingerprints ya conocidos (`faro.error_clusters`)
//!    en memoria — evita un `NOT IN` caro contra ClickHouse por cada tick.
//! 2. En cada tick, lee `error_events` recientes y filtra los fingerprints que no
//!    están en el set.
//! 3. Para cada fingerprint nuevo:
//!    - Calcula MinHash sobre shingles del `(exception_type, exception_message, stack_trace)`.
//!    - Busca clusters representantes existentes con mismo `(project, service, exception_type)`
//!      y similitud Jaccard ≥ [`Config::fingerprint_compactor_jaccard`] (default 0.85).
//!    - Si hay match, lo asigna al cluster existente. Si no, crea uno nuevo
//!      donde `cluster_id = fingerprint` (este fp es el nuevo representante).
//!
//! Estado: el HashSet de fingerprints conocidos vive en la tarea. Si el worker
//! crashea y reinicia, recarga desde DB en el siguiente arranque (idempotente
//! por ReplacingMergeTree).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::time::{interval, MissedTickBehavior};

use crate::minhash;
use crate::state::SharedState;
use crate::storage::ErrorClusterRow;

/// Cuántos fingerprints procesar como máximo por tick. Evita una sola tick que
/// se pasa minutos compactando si hay un burst raro de errores nuevos.
const BATCH_LIMIT: u32 = 500;
/// Cuántos representantes cargar por (project, service, exception_type) para
/// comparar contra. Si un combo tiene más, los más antiguos no compiten — eso es
/// OK porque tras 30 días esos rep ya son históricos.
const CANDIDATES_PER_KEY_LIMIT: usize = 50;
/// Horizonte para considerar representantes "vivos" en la búsqueda de match.
const CANDIDATE_HORIZON_DAYS: i64 = 30;

#[derive(Deserialize, Debug)]
struct NewFingerprintRow {
    project_id: String,
    service_name: String,
    #[serde(default)]
    exception_type: String,
    fingerprint: String,
    #[serde(default)]
    exception_message: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    stack_trace: String,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    first_seen: DateTime<Utc>,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    last_seen: DateTime<Utc>,
    #[serde(default)]
    occurrences: u64,
}

#[derive(Deserialize, Debug)]
struct FpOnlyRow {
    fingerprint: String,
}

pub fn start_fingerprint_compactor(state: SharedState) {
    if !state.cfg.fingerprint_compactor_enabled {
        tracing::info!("compactador de fingerprints deshabilitado");
        return;
    }
    let interval_secs = state.cfg.fingerprint_compactor_interval_secs.max(60);
    let jaccard_threshold = state.cfg.fingerprint_compactor_jaccard.clamp(0.5, 1.0);

    tracing::info!(
        every_secs = interval_secs,
        jaccard = jaccard_threshold,
        "arrancando compactador de fingerprints (MinHash K={})",
        minhash::K
    );

    tokio::spawn(async move {
        // Carga inicial: set de fingerprints ya clusterizados. Si falla, empezamos
        // con set vacío — el primer tick reinsertará todo pero idempotentemente
        // (ReplacingMergeTree dedup por version).
        let mut known_fps: HashSet<String> = match load_known_fingerprints(&state).await {
            Ok(set) => {
                tracing::info!(
                    count = set.len(),
                    "compactador: fingerprints conocidos cargados"
                );
                set
            }
            Err(e) => {
                tracing::warn!(error = %e, "compactador: no se pudo cargar set inicial — empezamos vacío");
                HashSet::new()
            }
        };

        // Espera inicial para que el resto del backend termine de arrancar.
        tokio::time::sleep(Duration::from_secs(30)).await;

        let mut tick = interval(Duration::from_secs(interval_secs));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.tick().await; // descarta el primer tick inmediato

        loop {
            tick.tick().await;
            match compact_once(&state, &mut known_fps, jaccard_threshold).await {
                Ok(0) => tracing::debug!("compactador: nada nuevo"),
                Ok(n) => {
                    tracing::info!(processed = n, "compactador: nuevos fingerprints procesados")
                }
                Err(e) => tracing::warn!(error = %e, "compactador: tick falló"),
            }
        }
    });
}

async fn load_known_fingerprints(state: &SharedState) -> anyhow::Result<HashSet<String>> {
    let rows: Vec<FpOnlyRow> = state
        .ch
        .select("SELECT fingerprint FROM faro.error_clusters")
        .await?;
    Ok(rows.into_iter().map(|r| r.fingerprint).collect())
}

async fn compact_once(
    state: &SharedState,
    known_fps: &mut HashSet<String>,
    jaccard_threshold: f64,
) -> anyhow::Result<usize> {
    // 1. Trae fingerprints distintos vistos en error_events recientes. Limitamos
    //    a 7 días para acotar coste (los más viejos ya fueron procesados o no
    //    nos interesa retomarlos). LIMIT BATCH_LIMIT corta el batch para
    //    no procesar demasiado por tick.
    let sql = format!(
        "SELECT
            project_id,
            service_name,
            any(exception_type) AS exception_type,
            fingerprint,
            any(exception_message) AS exception_message,
            any(message) AS message,
            any(stack_trace) AS stack_trace,
            min(timestamp) AS first_seen,
            max(timestamp) AS last_seen,
            toUInt64(count()) AS occurrences
         FROM faro.error_events
         WHERE timestamp > now() - INTERVAL 7 DAY
         GROUP BY project_id, service_name, fingerprint
         LIMIT {BATCH_LIMIT}"
    );
    let candidates: Vec<NewFingerprintRow> = state.ch.select(&sql).await?;

    // Filtra los que ya conocemos. Coste O(N) sobre el HashSet.
    let new_rows: Vec<NewFingerprintRow> = candidates
        .into_iter()
        .filter(|c| !known_fps.contains(&c.fingerprint))
        .collect();

    if new_rows.is_empty() {
        return Ok(0);
    }

    // 2. Pre-cargar representantes de los (project, service, exc_type) que aparecen
    //    en el batch. Esto evita un SELECT por fingerprint nuevo.
    let mut reps_by_key: HashMap<(String, String, String), Vec<ErrorClusterRow>> = HashMap::new();
    let keys: HashSet<(String, String, String)> = new_rows
        .iter()
        .map(|r| {
            (
                r.project_id.clone(),
                r.service_name.clone(),
                r.exception_type.clone(),
            )
        })
        .collect();
    for (project, service, exc_type) in keys {
        match load_representatives(state, &project, &service, &exc_type).await {
            Ok(rows) => {
                reps_by_key.insert((project, service, exc_type), rows);
            }
            Err(e) => {
                tracing::warn!(error = %e, project, service, exc_type, "compactador: load reps falló");
            }
        }
    }

    // 3. Para cada fingerprint nuevo, computar firma + asignar a cluster.
    let now = Utc::now();
    let mut rows_to_insert: Vec<ErrorClusterRow> = Vec::with_capacity(new_rows.len());
    let mut newly_known: Vec<String> = Vec::with_capacity(new_rows.len());

    for new_fp in new_rows {
        let shingles_owned = build_shingles(&new_fp);
        let shingles_refs: Vec<&str> = shingles_owned.iter().map(String::as_str).collect();
        let sig = minhash::signature(&shingles_refs);
        let sig_vec: Vec<u64> = sig.to_vec();

        let key = (
            new_fp.project_id.clone(),
            new_fp.service_name.clone(),
            new_fp.exception_type.clone(),
        );
        let reps = reps_by_key.get(&key);

        let mut best: Option<(f64, String)> = None;
        if let Some(reps) = reps {
            for r in reps {
                let j = minhash::jaccard(&sig_vec, &r.minhash);
                if best.as_ref().map_or(true, |(prev, _)| j > *prev) {
                    best = Some((j, r.cluster_id.clone()));
                }
            }
        }

        let (cluster_id, is_new_rep) = match best {
            Some((j, cid)) if j >= jaccard_threshold => (cid, false),
            _ => (new_fp.fingerprint.clone(), true),
        };

        let row = ErrorClusterRow {
            fingerprint: new_fp.fingerprint.clone(),
            cluster_id: cluster_id.clone(),
            project_id: new_fp.project_id.clone(),
            service_name: new_fp.service_name.clone(),
            exception_type: new_fp.exception_type.clone(),
            minhash: sig_vec.clone(),
            // Sólo el representante guarda message/stack — los miembros no, para no
            // gastar disco con texto duplicado del mismo cluster.
            representative_message: if is_new_rep {
                non_empty_or(&new_fp.exception_message, &new_fp.message)
            } else {
                String::new()
            },
            representative_stack: if is_new_rep {
                new_fp.stack_trace.clone()
            } else {
                String::new()
            },
            member_count: new_fp.occurrences,
            first_seen_at: new_fp.first_seen,
            last_seen_at: new_fp.last_seen,
            // Version monotónica para que reinserts del mismo rep posteriormente
            // (al actualizar last_seen) ganen contra la fila vieja.
            version: now.timestamp_millis() as u64,
        };

        // Si creamos un rep nuevo, añadirlo al map para que los siguientes fps del
        // mismo batch puedan engancharse con él en vez de crear duplicados.
        if is_new_rep {
            reps_by_key.entry(key).or_default().push(row.clone());
        }

        newly_known.push(new_fp.fingerprint);
        rows_to_insert.push(row);
    }

    if !rows_to_insert.is_empty() {
        state
            .ch
            .insert("faro.error_clusters", &rows_to_insert)
            .await?;
        for fp in newly_known {
            known_fps.insert(fp);
        }
    }

    Ok(rows_to_insert.len())
}

async fn load_representatives(
    state: &SharedState,
    project: &str,
    service: &str,
    exc_type: &str,
) -> anyhow::Result<Vec<ErrorClusterRow>> {
    // FINAL + filtro `fingerprint = cluster_id` para sólo traer representantes.
    // El LIMIT acota el número de comparisons por nuevo fingerprint.
    let sql = format!(
        "SELECT fingerprint, cluster_id, project_id, service_name, exception_type,
                minhash, representative_message, representative_stack,
                member_count, first_seen_at, last_seen_at, version
         FROM faro.error_clusters FINAL
         WHERE project_id = {{project:String}}
           AND service_name = {{service:String}}
           AND exception_type = {{exc:String}}
           AND fingerprint = cluster_id
           AND last_seen_at > now() - INTERVAL {CANDIDATE_HORIZON_DAYS} DAY
         ORDER BY last_seen_at DESC
         LIMIT {CANDIDATES_PER_KEY_LIMIT}"
    );
    state
        .ch
        .select_with_params(
            &sql,
            &[
                ("project", project),
                ("service", service),
                ("exc", exc_type),
            ],
        )
        .await
}

fn build_shingles(row: &NewFingerprintRow) -> Vec<String> {
    // Combinamos type + message + stack en un solo texto para shinglear. El type
    // aparece duplicado (peso extra para que NPE vs ConnectionRefused no se
    // confundan aunque su stack se parezca).
    let mut combined = String::with_capacity(
        row.exception_type.len()
            + row.exception_message.len()
            + row.message.len()
            + row.stack_trace.len()
            + 16,
    );
    combined.push_str(&row.exception_type);
    combined.push(' ');
    combined.push_str(&row.exception_type);
    combined.push(' ');
    if !row.exception_message.is_empty() {
        combined.push_str(&row.exception_message);
        combined.push(' ');
    } else if !row.message.is_empty() {
        combined.push_str(&row.message);
        combined.push(' ');
    }
    combined.push_str(&row.stack_trace);
    minhash::shingle(&combined)
}

fn non_empty_or(primary: &str, fallback: &str) -> String {
    if !primary.is_empty() {
        primary.to_string()
    } else {
        fallback.to_string()
    }
}
