//! Integration tests del worker `alert_evaluator` contra un ClickHouse real.
//!
//! Cubren los caminos críticos:
//!   * fire — la regla cruza el umbral → se inserta un incidente `firing` y la
//!     entrada queda en `active`.
//!   * dedup — segunda evaluación con la misma condición de disparo NO crea un
//!     incidente nuevo (clave: una regla mal evaluada = spam de avisos).
//!   * resolve — cuando la condición deja de cumplirse para una regla activa,
//!     se inserta una fila con `status='resolved'` y se quita del `active`.
//!   * cada operador (gt/gte/lt/lte/eq).
//!   * loop spawneado: con `tokio::time::pause()`/`advance()` se valida que tras
//!     el primer reload-tick + tick de evaluación la regla queda evaluada
//!     end-to-end (cubre la coordinación de los `interval(...)` internos).
//!
//! Aislamiento: cada test genera un `project_id` y un `rule_id` propios, y todas
//! las queries (de la regla y de las aserciones) filtran por esos IDs, así dos
//! tests en paralelo no se pisan en `faro.alert_rules` ni en `faro.alert_incidents`.

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use common::test_config;
use faro::state::AppState;
use faro::storage::{AlertIncidentRow, AlertRuleRow, AttrMap, Client, LogRow};
use faro::workers::alert_evaluator::evaluate_rule;
use serde::Deserialize;
use uuid::Uuid;

/// State mínimo (sin spawnear listeners HTTP ni workers) — `evaluate_rule` solo
/// usa `state.ch`, `state.notification_channels` (vacía aquí) y nada más.
async fn minimal_state() -> Arc<AppState> {
    let cfg = test_config();
    let ch = Client::new(&cfg).await.expect("CH client");
    ch.wait_until_ready().await.expect("CH ready");
    Arc::new(AppState::new(cfg, ch))
}

/// Construye una regla `error_count > threshold` sobre logs del proyecto dado,
/// con un `id` único para aislar conteos entre tests. La SELECT es una subquery
/// escalar wrapeada por `toFloat64(...)` exactamente como lo hace
/// `evaluate_rule` con la query del usuario.
fn make_count_rule(project: &str, threshold: f64) -> AlertRuleRow {
    // `:window_seconds` lo reemplaza `evaluate_rule` antes de ejecutar.
    let query = format!(
        "(SELECT count() FROM faro.logs \
          WHERE project_id = '{project}' \
            AND severity_number >= 17 \
            AND timestamp > now() - INTERVAL :window_seconds SECOND)"
    );
    let now = Utc::now();
    AlertRuleRow {
        id: Uuid::new_v4(),
        project_id: project.into(),
        name: format!("test-{}", &project[..8.min(project.len())]),
        description: String::new(),
        source: "logs".into(),
        query,
        condition: "gt".into(),
        threshold,
        window_seconds: 60,
        interval_seconds: 60,
        severity: "warn".into(),
        notification_targets: Vec::new(), // sin targets → dispatch es no-op
        enabled: 1,
        created_at: now,
        updated_at: now,
        deleted: 0,
        version: 1,
    }
}

fn err_log(secs_ago: i64, project: &str, body: &str) -> LogRow {
    let now = Utc::now();
    let ts = now - ChronoDuration::seconds(secs_ago);
    LogRow {
        timestamp: ts,
        observed_timestamp: ts,
        project_id: project.into(),
        service_name: "test-svc".into(),
        severity_text: "ERROR".into(),
        severity_number: LogRow::severity_from_text("ERROR"),
        body: body.into(),
        trace_id: String::new(),
        span_id: String::new(),
        scope_name: String::new(),
        resource_attributes: AttrMap::new(),
        attributes: AttrMap::new(),
    }
}

async fn count_incidents(ch: &Client, rule_id: Uuid) -> u64 {
    #[derive(Deserialize)]
    struct Cnt {
        count: u64,
    }
    let sql = "SELECT toUInt64(count()) AS count \
               FROM faro.alert_incidents \
               WHERE rule_id = {rid:UUID}";
    let row: Option<Cnt> = ch
        .select_one_with_params(sql, &[("rid", &rule_id.to_string())])
        .await
        .expect("count incidents");
    row.map(|c| c.count).unwrap_or(0)
}

async fn latest_incident(ch: &Client, rule_id: Uuid) -> Option<AlertIncidentRow> {
    let sql = "SELECT id, project_id, rule_id, rule_name, started_at, resolved_at, \
                       value, threshold, severity, status, note, version \
               FROM faro.alert_incidents FINAL \
               WHERE rule_id = {rid:UUID} \
               ORDER BY started_at DESC LIMIT 1";
    let rows: Vec<AlertIncidentRow> = ch
        .select_with_params(sql, &[("rid", &rule_id.to_string())])
        .await
        .expect("select incident");
    rows.into_iter().next()
}

#[tokio::test]
async fn fires_incident_when_above_threshold_and_dedupes_on_second_call() {
    let state = minimal_state().await;
    let project = format!("alert-fire-{}", Uuid::new_v4().simple());

    // Cinco ERROR logs dentro de la ventana de 60s.
    let rows: Vec<LogRow> = (0..5).map(|i| err_log(i * 5, &project, "boom")).collect();
    state
        .ch
        .insert("faro.logs", &rows)
        .await
        .expect("insert logs");

    // Threshold = 3 → con 5 errores recientes la condición cruza.
    let rule = make_count_rule(&project, 3.0);
    let rule_id = rule.id;
    let mut active: HashMap<Uuid, AlertIncidentRow> = HashMap::new();

    // 1) Primera evaluación: tiene que disparar.
    evaluate_rule(state.clone(), rule.clone(), &mut active).await;

    assert_eq!(
        active.len(),
        1,
        "el incidente debe quedar en el mapa active"
    );
    let first = active
        .get(&rule_id)
        .expect("active tiene el incidente")
        .clone();
    assert_eq!(first.status, "firing");
    assert!(first.value >= 5.0, "value reportado = {}", first.value);
    assert_eq!(first.threshold, 3.0);
    assert!(first.resolved_at.is_none());
    assert_eq!(
        count_incidents(&state.ch, rule_id).await,
        1,
        "una sola fila"
    );

    // 2) Segunda evaluación con la misma condición: dedup.
    //    El bug que esto previene: una regla "ruidosa" generando un incidente
    //    nuevo en cada tick, spammeando avisos. La fuente de verdad es el mapa
    //    `active` — si tiene la regla, NO se inserta otro incidente.
    evaluate_rule(state.clone(), rule.clone(), &mut active).await;

    assert_eq!(
        active.len(),
        1,
        "dedup en memoria: sigue habiendo 1 entrada"
    );
    let still = active.get(&rule_id).expect("dedup").clone();
    assert_eq!(
        still.id, first.id,
        "el incidente activo debe preservar su id (no se crea uno nuevo)"
    );
    assert_eq!(
        count_incidents(&state.ch, rule_id).await,
        1,
        "dedup persistido: faro.alert_incidents tampoco creció"
    );
}

#[tokio::test]
async fn resolves_when_condition_clears() {
    let state = minimal_state().await;
    let project = format!("alert-resolve-{}", Uuid::new_v4().simple());

    let rows: Vec<LogRow> = (0..3).map(|i| err_log(i * 5, &project, "x")).collect();
    state
        .ch
        .insert("faro.logs", &rows)
        .await
        .expect("insert logs");

    let mut rule = make_count_rule(&project, 1.0); // 3 > 1 → fires
    let rule_id = rule.id;
    let mut active = HashMap::new();
    evaluate_rule(state.clone(), rule.clone(), &mut active).await;
    assert_eq!(active.len(), 1, "primera evaluación: firing");

    // Subimos el umbral por encima del valor actual — la condición ya no se
    // cumple, así que tiene que resolver el incidente activo. (En prod la
    // condición cambia porque cambian los datos; mover el threshold logra el
    // mismo efecto con datos fijos, evitando depender de `tokio::time` para
    // empujar logs fuera de la ventana.)
    rule.threshold = 1_000.0;
    evaluate_rule(state.clone(), rule.clone(), &mut active).await;

    assert!(active.is_empty(), "tras resolver, active queda vacío");
    let latest = latest_incident(&state.ch, rule_id)
        .await
        .expect("debería existir el incidente resuelto");
    assert_eq!(latest.status, "resolved");
    assert!(
        latest.resolved_at.is_some(),
        "resolved_at debe estar seteado"
    );
    assert!(
        latest.version >= 2,
        "version bumpea al resolver (v={})",
        latest.version
    );
}

#[tokio::test]
async fn does_not_fire_when_below_threshold() {
    let state = minimal_state().await;
    let project = format!("alert-quiet-{}", Uuid::new_v4().simple());

    // Solo 1 error.
    state
        .ch
        .insert("faro.logs", &[err_log(2, &project, "x")])
        .await
        .expect("insert");

    let rule = make_count_rule(&project, 10.0); // 1 < 10 → no debe disparar
    let rule_id = rule.id;
    let mut active = HashMap::new();
    evaluate_rule(state.clone(), rule.clone(), &mut active).await;

    assert!(active.is_empty(), "sin disparar, active queda vacío");
    assert_eq!(
        count_incidents(&state.ch, rule_id).await,
        0,
        "no se insertó nada"
    );
}

#[tokio::test]
async fn condition_operators_all_handled() {
    let state = minimal_state().await;
    let project = format!("alert-ops-{}", Uuid::new_v4().simple());

    // 4 errores. Probamos cada operador con un threshold tal que la condición
    // se cumple según su semántica.
    let rows: Vec<LogRow> = (0..4).map(|i| err_log(i, &project, "x")).collect();
    state
        .ch
        .insert("faro.logs", &rows)
        .await
        .expect("insert logs");

    let cases: &[(&str, f64, bool)] = &[
        ("gt", 3.0, true),  // 4 > 3
        ("gt", 4.0, false), // 4 > 4 → falso
        ("gte", 4.0, true), // 4 >= 4
        ("lt", 5.0, true),  // 4 < 5
        ("lt", 4.0, false), // 4 < 4 → falso
        ("lte", 4.0, true), // 4 <= 4
        ("eq", 4.0, true),  // 4 == 4
        ("eq", 5.0, false), // 4 != 5
    ];

    for (op, thr, should_fire) in cases {
        let mut rule = make_count_rule(&project, *thr);
        rule.condition = (*op).to_string();
        let rid = rule.id;
        let mut active = HashMap::new();
        evaluate_rule(state.clone(), rule, &mut active).await;
        assert_eq!(
            active.len(),
            if *should_fire { 1 } else { 0 },
            "operador `{op}` con threshold {thr} (esperado fire={should_fire})"
        );
        let count = count_incidents(&state.ch, rid).await;
        assert_eq!(
            count,
            if *should_fire { 1 } else { 0 },
            "operador `{op}` con threshold {thr}: incidentes persistidos"
        );
    }
}

#[tokio::test]
async fn unknown_condition_is_a_no_op_not_a_panic() {
    let state = minimal_state().await;
    let project = format!("alert-bad-op-{}", Uuid::new_v4().simple());

    state
        .ch
        .insert("faro.logs", &[err_log(1, &project, "x")])
        .await
        .expect("insert");

    let mut rule = make_count_rule(&project, 0.0);
    rule.condition = "between".into(); // no soportado
    let rid = rule.id;
    let mut active = HashMap::new();
    // No tiene que entrar en pánico ni disparar nada.
    evaluate_rule(state.clone(), rule, &mut active).await;
    assert!(active.is_empty());
    assert_eq!(count_incidents(&state.ch, rid).await, 0);
}

#[tokio::test]
async fn bad_sql_query_does_not_crash_evaluator() {
    let state = minimal_state().await;
    let project = format!("alert-bad-sql-{}", Uuid::new_v4().simple());
    let mut rule = make_count_rule(&project, 0.0);
    rule.query = "thisIsNotValidClickHouseSQL()".into();
    let rid = rule.id;
    let mut active = HashMap::new();
    // El evaluator loguea el warning y vuelve — no debe panic ni insertar.
    evaluate_rule(state.clone(), rule, &mut active).await;
    assert!(active.is_empty());
    assert_eq!(count_incidents(&state.ch, rid).await, 0);
}

/// Ejercita el LOOP spawneado por `start_alert_evaluator` para validar la
/// coordinación de los dos `interval(...)` (reload cada 15s, tick cada 1s).
///
/// Pausamos el clock de tokio DESPUÉS de inicializar el `Client` de reqwest
/// (su builder/rustls hacen cosas que asumen wall-clock), y luego usamos
/// `tokio::time::advance(...)` para saltar los intervalos sin esperar real.
/// El loop usa `std::time::Instant::now()` (no `tokio::time::Instant`) para
/// la siguiente evaluación por regla, así que sólo los `interval(...)` quedan
/// gobernados por el clock pausado — suficiente para que la primera
/// evaluación dispare. Para las queries de aserción re-`resume()` el clock,
/// porque los timeouts de reqwest hacia CH sí miran el clock pausado y
/// cualquier I/O tarda más que el advance disponible.
#[tokio::test]
async fn spawned_loop_evaluates_after_advance() {
    let state = minimal_state().await;
    let project = format!("alert-loop-{}", Uuid::new_v4().simple());

    let rows: Vec<LogRow> = (0..5).map(|i| err_log(i, &project, "x")).collect();
    state
        .ch
        .insert("faro.logs", &rows)
        .await
        .expect("insert logs");

    let rule = make_count_rule(&project, 1.0);
    let rule_id = rule.id;
    state
        .ch
        .insert("faro.alert_rules", &[rule])
        .await
        .expect("insert rule");

    faro::workers::start_alert_evaluator(state.clone());

    tokio::time::pause();
    // Más allá del primer reload-tick (15s) y de algunos tick de eval (1s c/u).
    // `advance` cede el control para que el runtime polee la tarea spawneada.
    for _ in 0..20 {
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
    }
    tokio::time::resume();

    // El INSERT a CH del incidente sucedió en el spawn — esperamos en wall-clock
    // a que la fila esté persistida. 40 × 50ms = 2s tope, suficiente con
    // bastante margen para un INSERT JSONEachRow sincrónico.
    let mut found = 0u64;
    for _ in 0..40 {
        found = count_incidents(&state.ch, rule_id).await;
        if found > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        found >= 1,
        "el loop spawneado tuvo que insertar al menos 1 incidente tras advance"
    );
}
