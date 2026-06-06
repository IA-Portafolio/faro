//! Ingesta nativa de product events (`POST /api/v1/ingest/events`): valida el
//! path completo request → channel → writer → ClickHouse para los tipos
//! `track`, `identify`, `alias`; y los límites duros del handler (bearer,
//! batch size, event_name, properties size, SQL injection).

mod common;

use common::TestApp;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct EventOut {
    event_name: String,
    distinct_id: String,
    anonymous_id: String,
    properties: String,
    project_id: String,
}

async fn post_events(app: &TestApp, payload: serde_json::Value) -> reqwest::Response {
    app.http
        .post(format!("{}/api/v1/ingest/events", app.api_url))
        .bearer_auth(&app.project_token)
        .json(&payload)
        .send()
        .await
        .expect("send")
}

#[tokio::test]
async fn ingest_events_persists_track_to_clickhouse() {
    let app = TestApp::spawn().await;

    let payload = serde_json::json!({
        "service": "checkout",
        "batch": [{
            "type": "track",
            "event": "checkout_completed",
            "distinct_id": "user_42",
            "anonymous_id": "anon_abc",
            "properties": { "amount": 99.5, "currency": "USD" }
        }]
    });
    let resp = post_events(&app, payload).await;
    assert!(resp.status().is_success(), "status: {}", resp.status());
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["accepted"], 1);
    assert_eq!(body["project"], app.project_slug);

    let arrived = app
        .wait_for(120, || async { app.count_in("product_events").await >= 1 })
        .await;
    assert!(arrived, "el event no llegó a faro.product_events en 6 s");

    let rows: Vec<EventOut> = app
        .ch
        .select_with_params(
            "SELECT event_name, distinct_id, anonymous_id, properties, project_id \
             FROM faro.product_events WHERE project_id = {p:String} \
             ORDER BY timestamp DESC LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .expect("select");
    let row = rows.first().expect("una fila");
    assert_eq!(row.event_name, "checkout_completed");
    assert_eq!(row.distinct_id, "user_42");
    assert_eq!(row.anonymous_id, "anon_abc");
    assert_eq!(row.project_id, app.project_slug);
    assert!(row.properties.contains("\"amount\":99.5"));
}

#[tokio::test]
async fn ingest_events_identify_writes_canonical_name() {
    // `identify` se traduce a `event_name = "$identify"` y rellena
    // `user_properties` con los traits que mandó el SDK.
    let app = TestApp::spawn().await;

    let payload = serde_json::json!({
        "batch": [{
            "type": "identify",
            "distinct_id": "user_42",
            "user_properties": { "plan": "pro", "email": "a@b.com" }
        }]
    });
    let resp = post_events(&app, payload).await;
    assert!(resp.status().is_success(), "status: {}", resp.status());

    let arrived = app
        .wait_for(120, || async { app.count_in("product_events").await >= 1 })
        .await;
    assert!(arrived, "identify no llegó a faro.product_events");

    #[derive(Debug, Deserialize)]
    struct IdentRow {
        event_name: String,
        user_properties: String,
    }
    let rows: Vec<IdentRow> = app
        .ch
        .select_with_params(
            "SELECT event_name, user_properties FROM faro.product_events \
             WHERE project_id = {p:String} AND distinct_id = 'user_42' \
             ORDER BY timestamp DESC LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .expect("select");
    let row = rows.first().expect("una fila identify");
    assert_eq!(row.event_name, "$identify");
    assert!(row.user_properties.contains("\"plan\":\"pro\""));
}

#[tokio::test]
async fn ingest_events_identify_upserts_product_users_immediately() {
    // Fast-path: el handler hace best-effort INSERT a faro.product_users en
    // cuanto llega un $identify, así el dashboard ve al usuario sin esperar
    // los 60s del worker user_unifier. El row queda con event_count=0
    // (no autoritativo); el worker lo corrige más tarde.
    let app = TestApp::spawn().await;

    let payload = serde_json::json!({
        "batch": [{
            "type": "identify",
            "distinct_id": "fast_user_1",
            "anonymous_id": "anon_fast_1",
            "user_properties": { "plan": "pro" },
            "source": "web"
        }]
    });
    let resp = post_events(&app, payload).await;
    assert!(resp.status().is_success(), "status: {}", resp.status());

    #[derive(Debug, Deserialize)]
    struct UserRow {
        distinct_id: String,
        properties: String,
        sources: Vec<String>,
    }
    // El INSERT a product_users es síncrono dentro del handler — no esperamos
    // al flush del writer. Damos un margen mínimo por la latencia del round-trip.
    let arrived = app
        .wait_for(60, || async {
            let rows: Vec<UserRow> = app
                .ch
                .select_with_params(
                    "SELECT distinct_id, properties, sources FROM faro.product_users FINAL \
                     WHERE project_id = {p:String} AND distinct_id = 'fast_user_1'",
                    &[("p", &app.project_slug)],
                )
                .await
                .unwrap_or_default();
            !rows.is_empty()
        })
        .await;
    assert!(
        arrived,
        "identify no materializó product_users via fast-path"
    );

    let rows: Vec<UserRow> = app
        .ch
        .select_with_params(
            "SELECT distinct_id, properties, sources FROM faro.product_users FINAL \
             WHERE project_id = {p:String} AND distinct_id = 'fast_user_1' LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .expect("select product_users");
    let row = rows.first().expect("una fila product_users");
    assert_eq!(row.distinct_id, "fast_user_1");
    assert!(row.properties.contains("\"plan\":\"pro\""));
    assert!(
        row.sources.iter().any(|s| s == "web"),
        "sources debería incluir 'web', got: {:?}",
        row.sources
    );
}

#[tokio::test]
async fn ingest_events_rejects_missing_bearer() {
    let app = TestApp::spawn().await;
    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/events", app.api_url))
        .json(&serde_json::json!({ "batch": [] }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ingest_events_rejects_unknown_token() {
    let app = TestApp::spawn().await;
    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/events", app.api_url))
        .bearer_auth("not-a-real-token")
        .json(&serde_json::json!({
            "batch": [{ "type": "track", "event": "x", "distinct_id": "u" }]
        }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ingest_events_rejects_empty_event_name() {
    let app = TestApp::spawn().await;
    let payload = serde_json::json!({
        "batch": [{ "type": "track", "event": "", "distinct_id": "u" }]
    });
    let resp = post_events(&app, payload).await;
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let body = resp.text().await.unwrap_or_default();
    assert!(
        body.to_lowercase().contains("event_name"),
        "el mensaje 400 debe explicar 'event_name', got: {body}"
    );
}

#[tokio::test]
async fn ingest_events_rejects_payload_over_max_batch() {
    // El handler cortocircuita los batches > MAX_BATCH_EVENTS (100). Es la
    // protección contra clientes que mandan miles de eventos en un POST.
    let app = TestApp::spawn().await;
    let mut events = Vec::with_capacity(101);
    for i in 0..101 {
        events.push(serde_json::json!({
            "type": "track",
            "event": "bulk",
            "distinct_id": format!("u{i}")
        }));
    }
    let resp = post_events(&app, serde_json::json!({ "batch": events })).await;
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let body = resp.text().await.unwrap_or_default();
    assert!(
        body.to_lowercase().contains("batch"),
        "el 400 debería mencionar 'batch', got: {body}"
    );
}

#[tokio::test]
async fn ingest_events_rejects_sql_injection_in_event_name() {
    // La validación de event_name (alphanumeric + _-.$) rechaza espacios y
    // comillas — esto cierra el vector clásico de "inyectar SQL via event_name"
    // antes de que toque ClickHouse, aunque las queries van parametrizadas.
    let app = TestApp::spawn().await;
    let payload = serde_json::json!({
        "batch": [{
            "type": "track",
            "event": "x' OR 1=1--",
            "distinct_id": "u"
        }]
    });
    let resp = post_events(&app, payload).await;
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    // Verificación belt-and-suspenders: nada se escribió en CH para este proyecto.
    let count = app.count_in("product_events").await;
    assert_eq!(count, 0, "SQL injection no debería haber persistido nada");
}
