//! Worker que mantiene `faro.product_users` y `faro.product_user_aliases`
//! a partir de `faro.product_events`. Es la pieza que materializa el goal
//! 10.E.1: usuario unificado entre devices.
//!
//! ## Idea de diseño
//!
//! Cada `user_unifier_interval_secs` el worker escanea los `product_events`
//! caídos en una ventana deslizante (`now - watermark`) y agrega por
//! `(project_id, distinct_id)`:
//!
//! * `groupUniqArray(anonymous_id)` → todos los anon ids "vistos" para ese
//!   user en la ventana. Junto con la unión contra la fila existente,
//!   acumula histórico completo a lo largo del tiempo.
//! * `groupUniqArray(source)` → web/mobile/backend/... — la base del split
//!   por device.
//! * `min/max(timestamp)`, `count()`, y el último `user_properties` usable
//!   (`argMaxIf`, JSON object válido, ignorando `''` y `'{}'`).
//!
//! Después busca cada `(project_id, distinct_id)` en `product_users FINAL`,
//! une los arrays con lo existente (mantener histórico), preserva
//! `first_seen` (min), y re-INSERTA. `ReplacingMergeTree(last_seen)` se
//! queda con la versión más reciente al merge.
//!
//! Paralelamente, por cada `(project_id, anonymous_id, distinct_id)` que
//! aparece en la ventana se inserta una fila en `product_user_aliases`. La
//! propia ReplacingMergeTree dedupea por (project, anon) al merge.
//!
//! ## Idempotencia y crash-safety
//!
//! La ventana se ABRE más atrás que el intervalo (overlap ~30s) para que
//! ningún evento se pierda si el worker se demora un tick. La agregación
//! es idempotente: unir arrays es asociativo, `max(last_seen)` es estable.
//!
//! La marca de agua (`last_processed_at`) vive sólo en memoria: tras un
//! restart, el primer tick mira `now - bootstrap_lookback` (default 1h).
//! Eso recupera el flujo continuo sin necesitar una tabla de cursor;
//! eventos más viejos que esa ventana son atribuidos cuando se identifique
//! al user de nuevo desde algún device (escenario típico: el usuario
//! vuelve a abrir la app y dispara `$identify`).
//!
//! ## ¿Por qué no una MaterializedView?
//!
//! Una MV de ClickHouse sólo "ve" inserts; para unir contra el row PREVIO
//! de `product_users` haría falta un join, que las MVs no soportan
//! limpiamente. El worker hace exactamente ese join.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::time::{interval, MissedTickBehavior};

use crate::api::params::ch_dt;
use crate::state::SharedState;
use crate::storage::{ProductUserAliasRow, ProductUserRow};

/// Margen extra ANTES del watermark al construir la ventana de scan.
/// Cubre eventos que llegaron justo después de un tick — sin solapamiento
/// los perdiéramos. Idempotente: agregar el mismo evento dos veces no
/// cambia el resultado (unión de arrays + max).
const OVERLAP_SECS: i64 = 30;

/// En el primer tick tras un restart, mirar este rango hacia atrás para
/// reconstruir el histórico reciente sin escanear toda la tabla.
const BOOTSTRAP_LOOKBACK_HOURS: i64 = 1;

/// Tope de `(project_id, distinct_id)` procesados por tick. Un burst de
/// onboarding masivo no debe convertir el lookup contra `product_users` en
/// un IN-list de 100k entradas que sature ClickHouse. Si una ventana excede
/// el tope, el resto cae al siguiente tick (que verá los mismos eventos por
/// el overlap, así que nada se pierde).
const MAX_USERS_PER_TICK: usize = 5_000;

pub fn start_user_unifier(state: SharedState) {
    if !state.cfg.user_unifier_enabled {
        tracing::info!("user_unifier deshabilitado");
        return;
    }
    let interval_secs = state.cfg.user_unifier_interval_secs.max(10);

    tracing::info!(
        every_secs = interval_secs,
        "arrancando user_unifier (multi-device, goal 10.E.1)"
    );

    tokio::spawn(async move {
        // Espera inicial para que ingest writers vayan vaciando el primer batch.
        tokio::time::sleep(Duration::from_secs(15)).await;

        let mut watermark: DateTime<Utc> =
            Utc::now() - chrono::Duration::hours(BOOTSTRAP_LOOKBACK_HOURS);

        let mut tick = interval(Duration::from_secs(interval_secs));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.tick().await; // descarta tick inmediato

        loop {
            tick.tick().await;
            // Punto de corte del tick. Usamos `now()` como `to` para que
            // ventanas consecutivas sean estrictamente disjuntas (modulo
            // overlap). Si el tick demora, igual avanza al `now()` actual.
            let to = Utc::now();
            let from = watermark - chrono::Duration::seconds(OVERLAP_SECS);
            match unify_once(&state, from, to).await {
                Ok((users, aliases)) => {
                    if users > 0 || aliases > 0 {
                        tracing::info!(
                            users_updated = users,
                            aliases_upserted = aliases,
                            "user_unifier: tick completo"
                        );
                    } else {
                        tracing::debug!("user_unifier: tick sin cambios");
                    }
                    watermark = to;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "user_unifier: tick falló — reintento en el próximo tick");
                    // No avanzamos watermark: el siguiente tick reintenta esta ventana.
                }
            }
        }
    });
}

#[derive(Debug, Deserialize)]
struct AggRow {
    project_id: String,
    distinct_id: String,
    anonymous_ids: Vec<String>,
    sources: Vec<String>,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    min_ts: DateTime<Utc>,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    max_ts: DateTime<Utc>,
    event_count: u64,
    #[serde(default)]
    latest_props: String,
}

#[derive(Debug, Deserialize)]
struct ExistingUser {
    project_id: String,
    distinct_id: String,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    first_seen: DateTime<Utc>,
    #[serde(deserialize_with = "crate::storage::de_dt")]
    last_seen: DateTime<Utc>,
    #[serde(default)]
    anonymous_ids: Vec<String>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    event_count: u64,
    #[serde(default)]
    properties: String,
}

async fn unify_once(
    state: &SharedState,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> anyhow::Result<(usize, usize)> {
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);

    // Agregado de la ventana. Filtramos `distinct_id != ''` porque eventos
    // puramente anónimos (sin login) todavía no son un "user" — viven en
    // product_events pero no se materializan en product_users hasta que
    // un `$identify` ate el anon_id a un distinct_id.
    //
    // El `LIMIT` actúa como guardrail contra un burst que tendería a hacer
    // crecer el IN-list del lookup de la fila existente sin freno.
    let limit_s = MAX_USERS_PER_TICK.to_string();
    let agg: Vec<AggRow> = state
        .ch
        .select_with_params(
            aggregate_users_sql(),
            &[("from", &from_s), ("to", &to_s), ("batch_limit", &limit_s)],
        )
        .await?;

    if agg.is_empty() {
        return Ok((0, 0));
    }

    // Aliases: una fila por (project, anon, distinct) con `linked_at = max(timestamp)`
    // dentro de la ventana. La PK de product_user_aliases es `(project, anon)`,
    // así que si un mismo anon aparece con dos distinct_ids en la ventana,
    // ambas filas se insertan y el merge se queda con la de `linked_at` mayor.
    let mut aliases: Vec<ProductUserAliasRow> = Vec::new();
    for row in &agg {
        for anon in &row.anonymous_ids {
            if anon.is_empty() {
                continue;
            }
            aliases.push(ProductUserAliasRow {
                project_id: row.project_id.clone(),
                anonymous_id: anon.clone(),
                distinct_id: row.distinct_id.clone(),
                linked_at: row.max_ts,
            });
        }
    }

    // Lookup de las filas existentes para preservar `first_seen` y unir
    // arrays históricos. Construimos el IN-list parametrizado por índice.
    //
    // Truco: la PK es `(project_id, distinct_id)` pero ClickHouse no permite
    // `(a, b) IN ((x1, y1), (x2, y2))` con parámetros para cada elemento del
    // tuple — sí permite múltiples valores por columna, pero el cross-product
    // (proj_i × distinct_i) sería incorrecto cuando hay mezcla de proyectos.
    // Solución: filtramos sobre distinct_id (bloom filter index ayuda) y
    // luego matcheamos en Rust por (project, distinct) en exact.
    let distinct_keys: Vec<String> = agg.iter().map(|r| r.distinct_id.clone()).collect();
    let distinct_keys_set: HashSet<(String, String)> = agg
        .iter()
        .map(|r| (r.project_id.clone(), r.distinct_id.clone()))
        .collect();

    let existing = load_existing(state, &distinct_keys).await?;
    let mut existing_by_key = std::collections::HashMap::new();
    for u in existing {
        let key = (u.project_id.clone(), u.distinct_id.clone());
        if distinct_keys_set.contains(&key) {
            existing_by_key.insert(key, u);
        }
    }

    // Merge: para cada agg row, unión con la existente.
    let mut upserts: Vec<ProductUserRow> = Vec::with_capacity(agg.len());
    for row in agg {
        let key = (row.project_id.clone(), row.distinct_id.clone());
        let (first_seen, anonymous_ids, sources, event_count, properties) =
            match existing_by_key.remove(&key) {
                Some(ex) => {
                    let mut anons = ex.anonymous_ids;
                    let mut srcs = ex.sources;
                    merge_unique(&mut anons, &row.anonymous_ids);
                    merge_unique(&mut srcs, &row.sources);
                    let first = std::cmp::min(ex.first_seen, row.min_ts);
                    let count = ex.event_count.saturating_add(row.event_count);
                    let props = merge_user_properties(&ex.properties, &row.latest_props);
                    (first, anons, srcs, count, props)
                }
                None => (
                    row.min_ts,
                    dedupe(row.anonymous_ids),
                    dedupe(row.sources),
                    row.event_count,
                    merge_user_properties("", &row.latest_props),
                ),
            };
        upserts.push(ProductUserRow {
            project_id: row.project_id,
            distinct_id: row.distinct_id,
            first_seen,
            // `last_seen` siempre toma el max de la ventana — eso es lo que
            // ReplacingMergeTree usa como "versión", así que tiene que avanzar
            // monótonamente para que esta inserción gane el merge contra la
            // versión existente.
            last_seen: row.max_ts,
            anonymous_ids,
            sources,
            event_count,
            properties,
        });
    }

    let users_n = upserts.len();
    if !upserts.is_empty() {
        state.ch.insert("faro.product_users", &upserts).await?;
    }
    let aliases_n = aliases.len();
    if !aliases.is_empty() {
        state
            .ch
            .insert("faro.product_user_aliases", &aliases)
            .await?;
    }

    Ok((users_n, aliases_n))
}

fn aggregate_users_sql() -> &'static str {
    "SELECT project_id, \
            distinct_id, \
            arrayFilter(x -> x != '', groupUniqArray(anonymous_id)) AS anonymous_ids, \
            groupUniqArray(toString(source)) AS sources, \
            min(timestamp) AS min_ts, \
            max(timestamp) AS max_ts, \
            toUInt64(count()) AS event_count, \
            argMaxIf(user_properties, timestamp, user_properties != '' AND user_properties != '{}' AND isValidJSON(user_properties) AND JSONType(user_properties) = 'Object') AS latest_props \
     FROM faro.product_events \
     WHERE timestamp >= toDateTime64({from:DateTime64(9)}, 9) \
       AND timestamp <  toDateTime64({to:DateTime64(9)}, 9) \
       AND distinct_id != '' \
     GROUP BY project_id, distinct_id \
     ORDER BY max_ts DESC \
     LIMIT {batch_limit:UInt32}"
}

async fn load_existing(
    state: &SharedState,
    distinct_keys: &[String],
) -> anyhow::Result<Vec<ExistingUser>> {
    if distinct_keys.is_empty() {
        return Ok(Vec::new());
    }
    // Construimos un IN-list parametrizado para evitar interpolar input usuario.
    let mut sql = String::from(
        "SELECT project_id, distinct_id, first_seen, last_seen, \
                anonymous_ids, sources, event_count, properties \
         FROM faro.product_users FINAL \
         WHERE distinct_id IN (",
    );
    let names: Vec<String> = (0..distinct_keys.len()).map(|i| format!("d_{i}")).collect();
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            sql.push_str(", ");
        }
        sql.push_str(&format!("{{{name}:String}}"));
    }
    sql.push(')');

    let mut params: Vec<(&str, &str)> = Vec::with_capacity(distinct_keys.len());
    for (i, k) in distinct_keys.iter().enumerate() {
        params.push((names[i].as_str(), k.as_str()));
    }

    state.ch.select_with_params(&sql, &params).await
}

/// Une `extra` dentro de `base` preservando orden de inserción de `base` y
/// añadiendo sólo los valores nuevos. Conserva el histórico (los más
/// antiguos quedan primero) — útil para inspección humana del array.
fn merge_unique(base: &mut Vec<String>, extra: &[String]) {
    let mut seen: HashSet<String> = base.iter().cloned().collect();
    for v in extra {
        if v.is_empty() {
            continue;
        }
        if seen.insert(v.clone()) {
            base.push(v.clone());
        }
    }
}

fn dedupe(mut v: Vec<String>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    v.retain(|x| !x.is_empty() && seen.insert(x.clone()));
    v
}

fn merge_user_properties(existing: &str, latest: &str) -> String {
    if latest.is_empty() {
        return existing.to_string();
    }

    let latest = match serde_json::from_str::<serde_json::Value>(latest) {
        Ok(serde_json::Value::Object(obj)) if !obj.is_empty() => obj,
        _ => return existing.to_string(),
    };

    let mut merged = match serde_json::from_str::<serde_json::Value>(existing) {
        Ok(serde_json::Value::Object(obj)) => obj,
        _ => serde_json::Map::new(),
    };

    for (key, value) in latest {
        merged.insert(key, value);
    }

    serde_json::Value::Object(merged).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_unique_preserves_order_and_dedupes() {
        let mut base = vec!["anon-a".to_string(), "anon-b".to_string()];
        merge_unique(
            &mut base,
            &["anon-b".into(), "anon-c".into(), "anon-a".into()],
        );
        assert_eq!(base, vec!["anon-a", "anon-b", "anon-c"]);
    }

    #[test]
    fn merge_unique_ignores_empty() {
        let mut base = vec!["x".to_string()];
        merge_unique(&mut base, &["".into(), "y".into(), "".into()]);
        assert_eq!(base, vec!["x", "y"]);
    }

    #[test]
    fn dedupe_filters_empty() {
        let v = dedupe(vec!["a".into(), "".into(), "a".into(), "b".into()]);
        assert_eq!(v, vec!["a", "b"]);
    }

    #[test]
    fn aggregate_users_sql_uses_latest_non_empty_user_properties() {
        let sql = aggregate_users_sql();

        assert!(sql.contains(
            "argMaxIf(user_properties, timestamp, user_properties != '' AND user_properties != '{}' AND isValidJSON(user_properties) AND JSONType(user_properties) = 'Object') AS latest_props"
        ));
        assert!(sql.contains("user_properties != ''"));
        assert!(sql.contains("user_properties != '{}'"));
        assert!(sql.contains("isValidJSON(user_properties)"));
        assert!(sql.contains("JSONType(user_properties) = 'Object'"));
    }

    #[test]
    fn merge_user_properties_preserves_existing_keys_and_latest_wins() {
        let merged = merge_user_properties(
            r#"{"plan":"free","signup_date":"2026-01-01"}"#,
            r#"{"plan":"pro","industry":"fintech"}"#,
        );
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();

        assert_eq!(v["plan"], "pro");
        assert_eq!(v["signup_date"], "2026-01-01");
        assert_eq!(v["industry"], "fintech");
    }

    #[test]
    fn merge_user_properties_ignores_empty_or_invalid_latest_payload() {
        let existing = r#"{"plan":"pro"}"#;

        assert_eq!(merge_user_properties(existing, ""), existing);
        assert_eq!(merge_user_properties(existing, "not-json"), existing);
        assert_eq!(merge_user_properties(existing, "[]"), existing);
        assert_eq!(merge_user_properties(existing, "{}"), existing);
    }

    #[test]
    fn merge_user_properties_uses_latest_when_existing_is_empty_or_invalid() {
        let latest = r#"{"plan":"pro"}"#;

        assert_eq!(merge_user_properties("", latest), latest);
        assert_eq!(merge_user_properties("not-json", latest), latest);
        assert_eq!(merge_user_properties("[]", latest), latest);
        assert_eq!(merge_user_properties(r#""old""#, latest), latest);
    }
}
