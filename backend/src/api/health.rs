//! Endpoints de health.
//!
//! Tres niveles, alineados con la convención de Kubernetes / orchestrators
//! modernos:
//!
//! - `GET /healthz` — **Liveness**. Devuelve 200 siempre que el proceso pueda
//!   responder HTTP. Sirve para detectar deadlocks / panics que dejan el
//!   listener vivo pero el handler colgado. NO verifica dependencias externas
//!   (ClickHouse, Redis) — un fallo de CH no es razón para matar el proceso,
//!   sólo para sacarlo del routing.
//! - `GET /readyz` — **Readiness**. Ping a ClickHouse (hard) y opcionalmente a
//!   Redis (soft — ver más abajo). Devuelve 200 si las hard checks pasan, 503
//!   si alguna falla. El body JSON incluye latencia por dep para diagnóstico.
//! - `GET /metrics` — Prometheus exposition format. Wired en `main.rs` por
//!   simplicidad (cerca de la inicialización del recorder).
//!
//! **Por qué Redis es soft hoy**: la config tiene `redis_url` pero ningún path
//! del backend lo usa todavía (es un slot reservado para una capa futura de
//! buffer durable / locking distribuido). Marcar Redis como hard check ahora
//! significaría que tirar Redis para mantenimiento saca a Faro del routing
//! aunque el backend siga funcional al 100%. Cuando algún path crítico
//! empiece a depender de Redis, promoverlo a hard check (sumar su error al
//! status code 503).

use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::json;

use crate::state::SharedState;
use crate::versions::HealthResponse;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/healthz", axum::routing::get(healthz))
        .route("/readyz", axum::routing::get(readyz))
}

/// Body de `/healthz`. Re-usa [`HealthResponse`] (ya devolvía version + protocol)
/// para no romper SDKs que lo consumen al arranque para detectar mismatches.
async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse::current())
}

/// Resultado de un check individual a una dependencia.
#[derive(Debug, Serialize)]
struct DepCheck {
    /// `ok` | `error` | `skipped` (Redis cuando no está configurado).
    status: &'static str,
    latency_ms: u64,
    /// Mensaje de error si `status = error`. Truncado a 200 chars para no
    /// inflar el body si la dep escupe páginas.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn readyz(State(state): State<SharedState>) -> impl IntoResponse {
    let ch = check_clickhouse(&state).await;
    let redis = check_redis(state.cfg.redis_url.as_deref()).await;

    // CH es hard; Redis hoy es soft. Ver doc del módulo para el porqué.
    let hard_ok = ch.status == "ok";
    let overall_status = if hard_ok { "ready" } else { "degraded" };
    let http_status = if hard_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let body = json!({
        "status": overall_status,
        "checks": {
            "clickhouse": ch,
            "redis": redis,
        }
    });
    (http_status, Json(body))
}

async fn check_clickhouse(state: &SharedState) -> DepCheck {
    let start = Instant::now();
    // SELECT 1 es lo mínimo que valida que la conexión + auth + parser SQL
    // funcionan. wait_until_ready usa lo mismo en startup.
    match state.ch.query_raw("SELECT 1").await {
        Ok(_) => DepCheck {
            status: "ok",
            latency_ms: start.elapsed().as_millis() as u64,
            error: None,
        },
        Err(e) => DepCheck {
            status: "error",
            latency_ms: start.elapsed().as_millis() as u64,
            error: Some(truncate_err(&e.to_string())),
        },
    }
}

async fn check_redis(redis_url: Option<&str>) -> DepCheck {
    let Some(url) = redis_url else {
        return DepCheck {
            status: "skipped",
            latency_ms: 0,
            error: None,
        };
    };
    let start = Instant::now();
    match ping_redis(url).await {
        Ok(()) => DepCheck {
            status: "ok",
            latency_ms: start.elapsed().as_millis() as u64,
            error: None,
        },
        Err(e) => DepCheck {
            status: "error",
            latency_ms: start.elapsed().as_millis() as u64,
            error: Some(truncate_err(&e.to_string())),
        },
    }
}

/// Ping nativo a Redis vía RESP — evita agregar el crate `redis` (60-80KB
/// + tiempo de compilación) sólo para un PING. Si Faro algún día usa Redis
/// de verdad, ese consumidor traerá su propio cliente y este check puede
/// reutilizarlo en lugar de abrir una conexión nueva.
///
/// Protocolo: enviamos `*1\r\n$4\r\nPING\r\n` y esperamos `+PONG\r\n` (o
/// `-NOAUTH ...` si la base requiere AUTH — eso también lo contamos como
/// "respondió" porque el daemon está vivo).
async fn ping_redis(url: &str) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    // Parse `redis://[user[:pass]@]host[:port][/db]`. Sólo necesitamos host:port.
    let parsed = url::Url::parse(url).map_err(|e| format!("redis_url inválida: {e}"))?;
    if !matches!(parsed.scheme(), "redis" | "rediss") {
        return Err(format!("redis_url scheme inesperado: {}", parsed.scheme()));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "redis_url sin host".to_string())?;
    let port = parsed.port().unwrap_or(6379);

    // 3s es generoso para una red interna; en producción Caddy/CH viven en la
    // misma docker network y RTT es <1ms.
    let connect_fut = TcpStream::connect((host, port));
    let mut stream = timeout(Duration::from_secs(3), connect_fut)
        .await
        .map_err(|_| "timeout conectando a redis".to_string())?
        .map_err(|e| format!("connect: {e}"))?;

    stream
        .write_all(b"*1\r\n$4\r\nPING\r\n")
        .await
        .map_err(|e| format!("write: {e}"))?;

    // PONG simple response: `+PONG\r\n` (7 bytes). Leemos hasta 64 para
    // tolerar `+PONG\r\n` o `-NOAUTH ...` sin parsear RESP completo.
    let mut buf = [0u8; 64];
    let n = timeout(Duration::from_secs(3), stream.read(&mut buf))
        .await
        .map_err(|_| "timeout leyendo respuesta de redis".to_string())?
        .map_err(|e| format!("read: {e}"))?;

    if n == 0 {
        return Err("redis cerró conexión sin responder".into());
    }
    let resp = &buf[..n];
    if resp.starts_with(b"+PONG") {
        return Ok(());
    }
    // Si la base requiere auth, PING sin AUTH devuelve `-NOAUTH Authentication required.`
    // Para readyz nos basta con que el daemon responda — auth es otra cosa.
    if resp.starts_with(b"-NOAUTH") {
        return Ok(());
    }
    Err(format!(
        "respuesta inesperada de redis: {}",
        String::from_utf8_lossy(resp).trim()
    ))
}

fn truncate_err(s: &str) -> String {
    const MAX: usize = 200;
    if s.len() <= MAX {
        s.to_string()
    } else {
        // Cuidado con cortar a mitad de char UTF-8.
        let mut end = MAX;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}
