//! Negative tests del parser OTLP/HTTP+JSON. Confirma que el server degrada
//! con gracia ante payloads malformados:
//!
//!   - JSON sintácticamente inválido → 400 (axum::Json + serde)
//!   - Campos required ausentes (trace_id / start_time_unix_nano) → 400
//!   - Tipos incorrectos (severity_number > 255, body raw string) → 400
//!   - Variantes "vacías pero legales" (resourceLogs=[]) → 200 sin filas
//!   - Variantes "raras pero soportadas" (severity=99, timestamp="abc",
//!     atributos con tipos mezclados) → 200 con normalización documentada
//!
//! En NINGÚN caso esperamos 5xx, panic, ni filas corruptas en CH.

mod common;

use common::TestApp;
use reqwest::StatusCode;
use serde_json::json;

async fn post_logs(app: &TestApp, body: serde_json::Value) -> (StatusCode, String) {
    let resp = app
        .http
        .post(format!("{}/v1/logs", app.otlp_url))
        .bearer_auth(&app.project_token)
        .json(&body)
        .send()
        .await
        .expect("send");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    (status, text)
}

async fn post_logs_raw(app: &TestApp, raw: &'static str) -> (StatusCode, String) {
    let resp = app
        .http
        .post(format!("{}/v1/logs", app.otlp_url))
        .bearer_auth(&app.project_token)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(raw)
        .send()
        .await
        .expect("send");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    (status, text)
}

/// Status code que el server puede usar para rechazar un body OTLP malformado.
/// axum 0.7 distingue:
///   - 400 Bad Request  → JSON sintácticamente inválido
///   - 422 Unprocessable Entity → JSON válido pero no matchea el shape esperado
/// Cualquiera de los dos es "rechazo controlado"; lo opuesto sería 500 / panic.
fn is_client_rejection(s: StatusCode) -> bool {
    s == StatusCode::BAD_REQUEST || s == StatusCode::UNPROCESSABLE_ENTITY
}

async fn post_traces(app: &TestApp, body: serde_json::Value) -> (StatusCode, String) {
    let resp = app
        .http
        .post(format!("{}/v1/traces", app.otlp_url))
        .bearer_auth(&app.project_token)
        .json(&body)
        .send()
        .await
        .expect("send");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    (status, text)
}

fn ts_nano(secs_ago: i64) -> String {
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    (now - secs_ago * 1_000_000_000).to_string()
}

// ---------- Sintaxis / shape ----------

#[tokio::test]
async fn malformed_json_returns_400() {
    let app = TestApp::spawn().await;
    let (status, body) = post_logs_raw(&app, "{ not json").await;
    assert!(is_client_rejection(status), "status={status} body={body}");
    assert!(!status.is_server_error());
}

#[tokio::test]
async fn empty_object_is_accepted_as_zero_records() {
    // `{}` es válido OTLP: resource_logs default = []. No es un error de
    // contrato, es un batch vacío. Esperamos 200 (partialSuccess).
    let app = TestApp::spawn().await;
    let (status, _) = post_logs(&app, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.count_in("logs").await, 0);
}

#[tokio::test]
async fn empty_resource_logs_array_is_accepted() {
    let app = TestApp::spawn().await;
    let (status, _) = post_logs(&app, json!({ "resourceLogs": [] })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.count_in("logs").await, 0);
}

// ---------- Campos required ausentes ----------

#[tokio::test]
async fn span_missing_required_trace_id_returns_400() {
    let app = TestApp::spawn().await;
    let payload = json!({
        "resourceSpans": [{
            "scopeSpans": [{
                "spans": [{
                    // sin traceId
                    "spanId": "bbbbbbbbbbbbbbbb",
                    "name": "x",
                    "startTimeUnixNano": ts_nano(1),
                    "endTimeUnixNano": ts_nano(0),
                }]
            }]
        }]
    });
    let (status, body) = post_traces(&app, payload).await;
    assert!(is_client_rejection(status), "status={status} body={body}");
}

#[tokio::test]
async fn span_missing_required_start_time_returns_400() {
    let app = TestApp::spawn().await;
    let payload = json!({
        "resourceSpans": [{
            "scopeSpans": [{
                "spans": [{
                    "traceId": "a".repeat(32),
                    "spanId": "b".repeat(16),
                    "name": "x",
                    // sin startTimeUnixNano
                    "endTimeUnixNano": ts_nano(0),
                }]
            }]
        }]
    });
    let (status, body) = post_traces(&app, payload).await;
    assert!(is_client_rejection(status), "status={status} body={body}");
}

// ---------- Severity number fuera de rango ----------

#[tokio::test]
async fn severity_number_above_u8_returns_400() {
    // OTLP spec dice 0-24 pero el wire deserializa como UInt8 (0-255). 999 ni
    // siquiera entra en u8 → serde rechaza con 400. Mejor que aceptar basura.
    let app = TestApp::spawn().await;
    let payload = json!({
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "severityNumber": 999,
                    "body": { "stringValue": "x" },
                }]
            }]
        }]
    });
    let (status, body) = post_logs(&app, payload).await;
    assert!(is_client_rejection(status), "status={status} body={body}");
}

#[tokio::test]
async fn severity_number_in_u8_range_but_outside_otlp_range_is_accepted_gracefully() {
    // 99 está dentro de u8 pero fuera del rango OTLP (0-24). El parser no lo
    // rechaza — lo aceptamos como "degradación graceful": la fila se guarda
    // tal cual y el dashboard mostrará un valor numérico raro. NO crashea.
    let app = TestApp::spawn().await;
    let payload = json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [{ "key": "service.name", "value": { "stringValue": "neg" } }]
            },
            "scopeLogs": [{
                "logRecords": [{
                    "timeUnixNano": ts_nano(0),
                    "severityNumber": 99,
                    "body": { "stringValue": "x" },
                }]
            }]
        }]
    });
    let (status, _) = post_logs(&app, payload).await;
    assert_eq!(status, StatusCode::OK);
    let arrived = app
        .wait_for(40, || async { app.count_in("logs").await >= 1 })
        .await;
    assert!(arrived);
    #[derive(serde::Deserialize)]
    struct R {
        severity_number: u8,
    }
    let rows: Vec<R> = app
        .ch
        .select_with_params(
            "SELECT severity_number FROM faro.logs WHERE project_id = {p:String} LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .unwrap();
    assert_eq!(rows[0].severity_number, 99);
}

// ---------- Timestamps raros ----------

#[tokio::test]
async fn timestamp_non_numeric_string_is_accepted_without_crash() {
    // `StringOrU64::deserialize` cae a 0 si no parsea. El parser acepta el
    // batch sin tirarlo entero (mejor que rechazar por un timestamp raro de
    // un SDK roto). NO verificamos persistencia en CH porque la TTL de 30
    // días sobre `faro.logs` borra cualquier fila con timestamp=epoch
    // inmediatamente — eso es responsabilidad del schema, no del parser.
    let app = TestApp::spawn().await;
    let payload = json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [{ "key": "service.name", "value": { "stringValue": "neg" } }]
            },
            "scopeLogs": [{
                "logRecords": [{
                    "timeUnixNano": "definitely not a number",
                    "body": { "stringValue": "msg" },
                }]
            }]
        }]
    });
    let (status, body) = post_logs(&app, payload).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
}

#[tokio::test]
async fn timestamp_negative_number_is_accepted_without_crash() {
    // `StringOrU64::deserialize` hace `n.as_u64().unwrap_or(0)` — un número
    // JSON negativo no entra en u64, así que cae a 0 sin error. El batch se
    // acepta (200) en lugar de tirarlo entero. Misma nota de TTL que el caso
    // de string no-numérico: la fila persistida puede ser barrida por la TTL
    // sobre `timestamp` ~ 1970, así que no la verificamos en CH.
    let app = TestApp::spawn().await;
    let payload = json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [{ "key": "service.name", "value": { "stringValue": "neg" } }]
            },
            "scopeLogs": [{
                "logRecords": [{
                    "timeUnixNano": -1,
                    "body": { "stringValue": "x" },
                }]
            }]
        }]
    });
    let (status, body) = post_logs(&app, payload).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
}

// ---------- Body / attributes con tipos extraños ----------

#[tokio::test]
async fn body_as_raw_string_is_rejected() {
    // El spec dice body = AnyValue (objeto con stringValue/intValue/...).
    // Mandar un string raw rompe la forma → 400.
    let app = TestApp::spawn().await;
    let payload = json!({
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "body": "hello world raw",
                }]
            }]
        }]
    });
    let (status, body) = post_logs(&app, payload).await;
    assert!(is_client_rejection(status), "status={status} body={body}");
}

#[tokio::test]
async fn body_as_empty_anyvalue_object_is_accepted_with_empty_body() {
    // Objeto AnyValue sin ningún campo => to_string_value() = "". No crashea,
    // se guarda con body vacío.
    let app = TestApp::spawn().await;
    let payload = json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [{ "key": "service.name", "value": { "stringValue": "neg" } }]
            },
            "scopeLogs": [{
                "logRecords": [{
                    "timeUnixNano": ts_nano(0),
                    "body": {},
                }]
            }]
        }]
    });
    let (status, _) = post_logs(&app, payload).await;
    assert_eq!(status, StatusCode::OK);
    let arrived = app
        .wait_for(40, || async { app.count_in("logs").await >= 1 })
        .await;
    assert!(arrived);
    #[derive(serde::Deserialize)]
    struct R {
        body: String,
    }
    let rows: Vec<R> = app
        .ch
        .select_with_params(
            "SELECT body FROM faro.logs WHERE project_id = {p:String} LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .unwrap();
    assert_eq!(rows[0].body, "");
}

#[tokio::test]
async fn attribute_value_as_raw_string_is_rejected() {
    // Cada attribute.value debe ser AnyValue (objeto). Raw string → 400.
    let app = TestApp::spawn().await;
    let payload = json!({
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "body": { "stringValue": "x" },
                    "attributes": [{ "key": "k", "value": "should be object" }],
                }]
            }]
        }]
    });
    let (status, body) = post_logs(&app, payload).await;
    assert!(is_client_rejection(status), "status={status} body={body}");
}

#[tokio::test]
async fn resource_attributes_with_mixed_types_are_all_stringified() {
    // Mezcla string + int + bool + double + array + kvlist. El parser los
    // convierte todos a String vía AnyValue::to_string_value. No crashea.
    let app = TestApp::spawn().await;
    let payload = json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [
                    { "key": "service.name", "value": { "stringValue": "neg" } },
                    { "key": "k.int",    "value": { "intValue": "42" } },
                    { "key": "k.double", "value": { "doubleValue": 3.25 } },
                    { "key": "k.bool",   "value": { "boolValue": true } },
                    { "key": "k.arr",    "value": { "arrayValue": { "values": [
                        { "stringValue": "a" }, { "intValue": "1" }
                    ]}}},
                ]
            },
            "scopeLogs": [{
                "logRecords": [{
                    "timeUnixNano": ts_nano(0),
                    "body": { "stringValue": "mix" },
                }]
            }]
        }]
    });
    let (status, _) = post_logs(&app, payload).await;
    assert_eq!(status, StatusCode::OK);
    let arrived = app
        .wait_for(40, || async { app.count_in("logs").await >= 1 })
        .await;
    assert!(arrived);
    #[derive(serde::Deserialize)]
    struct R {
        resource_attributes: std::collections::BTreeMap<String, String>,
    }
    let rows: Vec<R> = app
        .ch
        .select_with_params(
            "SELECT resource_attributes FROM faro.logs WHERE project_id = {p:String} LIMIT 1",
            &[("p", &app.project_slug)],
        )
        .await
        .unwrap();
    let attrs = &rows[0].resource_attributes;
    assert_eq!(attrs.get("k.int").map(String::as_str), Some("42"));
    assert_eq!(attrs.get("k.bool").map(String::as_str), Some("true"));
    assert_eq!(attrs.get("k.double").map(String::as_str), Some("3.25"));
    assert!(attrs
        .get("k.arr")
        .map(|s| s.starts_with('['))
        .unwrap_or(false));
}

// ---------- Final sanity: ninguna respuesta fue 5xx ni panicked el server ----------

#[tokio::test]
async fn server_remains_healthy_after_negative_volley() {
    // Dispara una ráfaga corta de payloads malformados mezclados, luego
    // verifica que el server sigue aceptando un payload válido. Si algún
    // panic se hubiera comido el writer task, este último insert no llegaría.
    let app = TestApp::spawn().await;
    let bad_payloads = [
        json!({}),
        json!({ "resourceLogs": [] }),
        json!({ "resourceLogs": [{ "scopeLogs": [{ "logRecords": [{ "body": "raw" }]}]}]}),
        json!({ "resourceLogs": [{ "scopeLogs": [{ "logRecords": [
            { "severityNumber": 99, "body": { "stringValue": "x" } }
        ]}]}]}),
    ];
    for p in bad_payloads {
        let (status, body) = post_logs(&app, p).await;
        assert!(
            !status.is_server_error(),
            "ráfaga negativa generó 5xx: {status} body={body:.200}"
        );
    }

    // Tras la ráfaga, un payload válido sigue funcionando end-to-end.
    let good = json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [{ "key": "service.name", "value": { "stringValue": "healthy" } }]
            },
            "scopeLogs": [{
                "logRecords": [{
                    "timeUnixNano": ts_nano(0),
                    "body": { "stringValue": "still alive" },
                }]
            }]
        }]
    });
    let (status, _) = post_logs(&app, good).await;
    assert_eq!(status, StatusCode::OK);
    let arrived = app
        .wait_for(40, || async { app.count_in("logs").await >= 1 })
        .await;
    assert!(
        arrived,
        "el server quedó incapacitado tras la ráfaga negativa"
    );
}
