//! Integración de `POST /api/v1/funnels/compute` contra ClickHouse real.
//!
//! Antes este endpoint (cálculo central de conversión del 6º pilar) NO tenía
//! cobertura de integración: su único unit test reimplementaba el bucle de
//! acumulación y validaba su propia copia, no el handler. Este test inserta
//! `product_events` con un funnel de forma conocida y verifica los conteos por
//! paso, `total_entered` y las tasas de conversión que el handler devuelve.

mod common;

use chrono::{Duration, Utc};
use common::TestApp;
use faro::storage::ProductEventRow;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct FunnelStep {
    event: String,
    users: u64,
    conversion_from_start: f32,
    conversion_from_prev: f32,
}

#[derive(Debug, Deserialize)]
struct FunnelResult {
    steps: Vec<FunnelStep>,
    total_entered: u64,
    window_seconds: u32,
}

fn ev(secs_ago: i64, project: &str, distinct_id: &str, event_name: &str) -> ProductEventRow {
    ProductEventRow {
        timestamp: Utc::now() - Duration::seconds(secs_ago),
        project_id: project.into(),
        event_name: event_name.into(),
        distinct_id: distinct_id.into(),
        anonymous_id: String::new(),
        session_id: String::new(),
        properties: String::new(),
        user_properties: String::new(),
        context: String::new(),
        source: "web".into(),
        trace_id: String::new(),
        span_id: String::new(),
        event_id: Uuid::new_v4().to_string(),
    }
}

#[tokio::test]
async fn compute_returns_cumulative_step_counts_and_conversion() {
    let app = TestApp::spawn().await;
    let p = app.project_slug.clone();
    let email = app.create_user("funnel-pw-123").await;
    let session = app.login_session(&email, "funnel-pw-123").await;

    // Funnel de 3 pasos. Por usuario, los eventos van en orden temporal creciente
    // (windowFunnel ordena por timestamp): a < b < c.
    //   u1: a,b,c  → nivel 3
    //   u2: a,b    → nivel 2
    //   u3: a,b    → nivel 2
    //   u4: a      → nivel 1
    //   u5: a      → nivel 1
    // Esperado (cumulative-from-top): step_a=5, step_b=3, step_c=1; total_entered=5.
    let rows = vec![
        ev(300, &p, "u1", "step_a"),
        ev(299, &p, "u1", "step_b"),
        ev(298, &p, "u1", "step_c"),
        ev(290, &p, "u2", "step_a"),
        ev(289, &p, "u2", "step_b"),
        ev(280, &p, "u3", "step_a"),
        ev(279, &p, "u3", "step_b"),
        ev(270, &p, "u4", "step_a"),
        ev(260, &p, "u5", "step_a"),
    ];
    app.ch
        .insert("faro.product_events", &rows)
        .await
        .expect("insert product events");

    let resp = app
        .http
        .post(format!("{}/api/v1/funnels/compute", app.api_url))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .json(&serde_json::json!({
            "steps": ["step_a", "step_b", "step_c"],
            "project": p,
            "window_seconds": 86_400,
        }))
        .send()
        .await
        .expect("send compute");
    assert!(
        resp.status().is_success(),
        "compute status: {}",
        resp.status()
    );
    let result: FunnelResult = resp.json().await.expect("json");

    assert_eq!(result.window_seconds, 86_400);
    assert_eq!(result.total_entered, 5, "u1..u5 entraron al paso 1");
    assert_eq!(result.steps.len(), 3);

    let users: Vec<u64> = result.steps.iter().map(|s| s.users).collect();
    assert_eq!(
        users,
        vec![5, 3, 1],
        "conteos por paso (cumulative-from-top)"
    );

    assert_eq!(result.steps[0].event, "step_a");
    // Conversión desde el inicio: 5/5, 3/5, 1/5.
    assert!((result.steps[0].conversion_from_start - 1.0).abs() < 1e-5);
    assert!((result.steps[1].conversion_from_start - 0.6).abs() < 1e-5);
    assert!((result.steps[2].conversion_from_start - 0.2).abs() < 1e-5);
    // Conversión paso a paso: 1.0, 3/5, 1/3.
    assert!((result.steps[1].conversion_from_prev - 0.6).abs() < 1e-5);
    assert!((result.steps[2].conversion_from_prev - (1.0 / 3.0)).abs() < 1e-5);
}

#[tokio::test]
async fn compute_rejects_single_step() {
    let app = TestApp::spawn().await;
    let email = app.create_user("funnel-pw-456").await;
    let session = app.login_session(&email, "funnel-pw-456").await;

    let resp = app
        .http
        .post(format!("{}/api/v1/funnels/compute", app.api_url))
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .json(&serde_json::json!({ "steps": ["solo_uno"] }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}
