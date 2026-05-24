//! Integration tests del live tail SSE (`GET /api/v1/logs/live`).
//!
//! Cubren la propiedad clave del broadcast bus: N suscriptores reciben el mismo
//! evento, y un cliente lento (que no consume su recv) NO bloquea al productor
//! ni al resto de los suscriptores. La capacidad del canal (1024 en
//! `LiveBus::new`) acota el daño — un suscriptor que se atrasa pierde mensajes
//! viejos vía `BroadcastStreamRecvError::Lagged` y la conexión continúa.
//!
//! Stack: `TestApp` arranca el router en un puerto efímero contra el CH del
//! entorno (igual que el resto de integration tests). El ingest path va por
//! `POST /api/v1/ingest/logs` (auth con bearer del proyecto); el SSE va por
//! `GET /api/v1/logs/live` con cookie de sesión.

mod common;

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use chrono::Utc;
use common::TestApp;
use faro::storage::{AttrMap, LogRow};
use futures::StreamExt;

/// Parsea bloques SSE. Cada evento termina con `\n\n`; las líneas `data:` son
/// las que llevan el payload (las `event:` y los `:` comments —keep-alives— se
/// ignoran). Devuelve los `data:` extraídos. Diseñado para acumular
/// progresivamente: el caller pasa un buffer mutable y la función consume los
/// eventos completos, dejando el tail incompleto adentro para la próxima
/// llamada.
fn extract_data_events(buf: &mut Vec<u8>) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    loop {
        let Some(rel_end) = window_find(&buf[start..], b"\n\n") else {
            break;
        };
        let end = start + rel_end;
        let block = std::str::from_utf8(&buf[start..end]).unwrap_or("");
        for line in block.split('\n') {
            if let Some(rest) = line.strip_prefix("data:") {
                out.push(rest.trim_start().to_string());
            }
        }
        start = end + 2;
    }
    if start > 0 {
        buf.drain(0..start);
    }
    out
}

fn window_find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Conecta un cliente SSE a `/api/v1/logs/live?project=...` y devuelve la
/// respuesta. Verifica que el status sea 200 y el content-type SSE.
async fn open_sse(app: &TestApp, session: &str) -> reqwest::Response {
    let url = format!(
        "{}/api/v1/logs/live?project={}",
        app.api_url, app.project_slug
    );
    let resp = app
        .http
        .get(&url)
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .send()
        .await
        .expect("send SSE");
    assert!(
        resp.status().is_success(),
        "SSE connect status: {}, body: {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.starts_with("text/event-stream"), "content-type: {ct}");
    resp
}

/// Espera a que el bus tenga `expected` receivers activos. Cubre la latencia
/// entre `http.get(...)` y que el handler llegue a `live_bus.logs.subscribe()`.
async fn wait_subscribers(app: &TestApp, expected: usize) {
    for _ in 0..100 {
        if app.state.live_bus.logs.receiver_count() == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "esperando {expected} subscribers, hay {}",
        app.state.live_bus.logs.receiver_count()
    );
}

async fn post_log(app: &TestApp, message: &str) {
    let resp = app
        .http
        .post(format!("{}/api/v1/ingest/logs", app.api_url))
        .bearer_auth(&app.project_token)
        .json(&serde_json::json!({
            "service": "live-test",
            "logs": [
                { "level": "INFO", "message": message }
            ]
        }))
        .send()
        .await
        .expect("send ingest");
    assert!(
        resp.status().is_success(),
        "ingest status: {}",
        resp.status()
    );
}

/// Lee la respuesta SSE hasta que el predicate `done` devuelva true sobre los
/// `data:` parseados, o se acabe el deadline. Devuelve la lista de payloads
/// recibidos hasta ese momento.
async fn collect_events_until<F>(
    resp: reqwest::Response,
    deadline: Duration,
    mut done: F,
) -> Vec<String>
where
    F: FnMut(&[String]) -> bool,
{
    let mut events = Vec::<String>::new();
    let mut buf = Vec::<u8>::new();
    let mut stream = resp.bytes_stream();
    let result = tokio::time::timeout(deadline, async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("chunk read");
            buf.extend_from_slice(&chunk);
            let new = extract_data_events(&mut buf);
            events.extend(new);
            if done(&events) {
                return;
            }
        }
    })
    .await;
    let _ = result; // timeout es un caso esperado, devolvemos lo que haya
    events
}

fn test_log_row(project: &str, body: &str) -> LogRow {
    let now = Utc::now();
    LogRow {
        timestamp: now,
        observed_timestamp: now,
        project_id: project.into(),
        service_name: "burst".into(),
        severity_text: "INFO".into(),
        severity_number: LogRow::severity_from_text("INFO"),
        body: body.into(),
        trace_id: String::new(),
        span_id: String::new(),
        scope_name: String::new(),
        resource_attributes: AttrMap::new(),
        attributes: AttrMap::new(),
    }
}

#[tokio::test]
async fn two_clients_receive_the_same_logged_event() {
    let app = TestApp::spawn().await;
    let email = app.create_user("hunter2-test").await;
    let session = app.login_session(&email, "hunter2-test").await;

    let resp_a = open_sse(&app, &session).await;
    let resp_b = open_sse(&app, &session).await;
    wait_subscribers(&app, 2).await;

    let unique = format!("hello-{}", uuid::Uuid::new_v4().simple());
    let unique_for_check = unique.clone();
    post_log(&app, &unique).await;

    // Cada cliente colecta hasta ver su propio payload con la marca única.
    let needle = unique_for_check.clone();
    let (events_a, events_b) = tokio::join!(
        collect_events_until(resp_a, Duration::from_secs(3), move |events| events
            .iter()
            .any(|e| e.contains(&needle))),
        {
            let needle = unique_for_check.clone();
            collect_events_until(resp_b, Duration::from_secs(3), move |events| {
                events.iter().any(|e| e.contains(&needle))
            })
        },
    );

    assert!(
        events_a.iter().any(|e| e.contains(&unique)),
        "cliente A no recibió el evento. payloads: {events_a:?}"
    );
    assert!(
        events_b.iter().any(|e| e.contains(&unique)),
        "cliente B no recibió el evento. payloads: {events_b:?}"
    );

    // Sanity: el payload es un JSON parseable con los campos del LogRow.
    let payload_a = events_a.iter().find(|e| e.contains(&unique)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(payload_a).expect("payload JSON");
    assert_eq!(parsed["body"], unique);
    assert_eq!(parsed["service_name"], "live-test");
    assert_eq!(parsed["project_id"], app.project_slug);
}

#[tokio::test]
async fn slow_client_does_not_block_fast_or_the_producer() {
    let app = TestApp::spawn().await;
    let email = app.create_user("hunter2-test").await;
    let session = app.login_session(&email, "hunter2-test").await;

    // CLIENTE LENTO: abrimos la conexión pero NUNCA llamamos `bytes_stream`.
    // El response queda vivo en este scope, manteniendo el subscriber del bus
    // arriba; al no poll-earlo el queue interno del broadcast::Receiver se
    // llena y empieza a marcar Lagged, lo cual NO debería propagarse al resto.
    let slow_resp = open_sse(&app, &session).await;

    // CLIENTE RÁPIDO: lo lanzamos en un task que vacía el bytes_stream lo más
    // rápido que puede, acumulando eventos `data:` parseados.
    let fast_resp = open_sse(&app, &session).await;

    wait_subscribers(&app, 2).await;

    let events_seen = Arc::new(StdMutex::new(Vec::<String>::new()));
    let events_seen_w = events_seen.clone();
    let reader_handle = tokio::spawn(async move {
        let mut buf = Vec::<u8>::new();
        let mut stream = fast_resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) => break,
            };
            buf.extend_from_slice(&chunk);
            let new = extract_data_events(&mut buf);
            if !new.is_empty() {
                events_seen_w.lock().unwrap().extend(new);
            }
        }
    });

    // Producer: empuja 2_000 eventos por el bus directo (más rápido que pasar
    // por el ingest HTTP, y suficiente para superar la capacidad 1024 del
    // broadcast → el cliente lento entrará en Lagged). Mide cuánto tarda el
    // bucle: si el productor estuviese bloqueado por el slow, esto se iría a
    // varios segundos. broadcast::Sender::send es no-blocking por diseño;
    // medimos para verificarlo en el test.
    let bus = app.state.live_bus.logs.clone();
    let mut send_ok = 0u32;
    let send_start = std::time::Instant::now();
    for i in 0..2_000 {
        let row = test_log_row(&app.project_slug, &format!("burst-{i}"));
        if bus.send(row).is_ok() {
            send_ok += 1;
        }
    }
    let send_elapsed = send_start.elapsed();
    assert_eq!(
        send_ok, 2_000,
        "todos los sends deben tener éxito (broadcast es no-blocking)"
    );
    assert!(
        send_elapsed < Duration::from_secs(1),
        "el productor NO debe bloquearse cuando hay un suscriptor lento; tardó {send_elapsed:?}"
    );

    // Deja al cliente rápido tiempo para drenar lo que puede del ring buffer.
    // Tope realista: 2s para que la red local + axum entreguen los chunks SSE.
    let drain_deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < drain_deadline {
        if events_seen.lock().unwrap().len() >= 100 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let received = events_seen.lock().unwrap().len();
    // Esperamos al menos varios cientos — el cliente rápido drena
    // concurrentemente con el productor. No exigimos 2000 porque puede haber
    // Lagged también del lado del fast (el producer va sin yield), pero la
    // propiedad clave es: con un slow conectado, el fast SIGUE recibiendo
    // muchos eventos (no queda starved a 0).
    assert!(
        received >= 100,
        "cliente rápido debió recibir ≥100 eventos a pesar del slow; recibió {received}"
    );

    // Drop ordenado: cerrar el slow (libera su slot) y abortar el reader.
    drop(slow_resp);
    reader_handle.abort();

    // Tras cerrar el slow, el bus debería caer a 1 receptor activo (el fast,
    // aunque hayamos abortado el reader, el response al droppearse libera el
    // slot via RAII por el `SseSlot::Drop`). Damos margen porque el cleanup
    // del handler axum no es síncrono con el `drop`.
    for _ in 0..100 {
        if app.state.live_bus.logs.receiver_count() <= 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
