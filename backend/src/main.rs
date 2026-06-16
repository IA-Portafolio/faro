//! Binario del servidor Faro: arranca y orquesta todo el proceso.
//!
//! Inicializa logging/telemetría, carga la `Config` del entorno, conecta a
//! ClickHouse, lanza los workers en segundo plano y sirve dos superficies: la API
//! del dashboard (`:8080`) y los listeners de ingesta OTLP HTTP (`:4318`) y gRPC
//! (`:4317`). Gestiona el apagado ordenado ante SIGTERM. La lógica reutilizable
//! vive en la crate `faro` (`lib.rs`); esto es solo el `fn main`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::response::IntoResponse;
use axum::Router;
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use faro::{
    api, auth, config, feature_flags, ingest, integrations, notification_channels, observability,
    projects, state, storage, telemetry, workers,
};

use config::Config;
use state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,faro=debug")),
        )
        .with_target(true)
        .init();

    // Self-observability opcional. El guard se mantiene vivo hasta que
    // `main` termina, lo cual asegura un flush ordenado al recibir SIGTERM.
    let _otel_guard = match telemetry::init_otel() {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(error = %e, "fallo al iniciar self-observability — continuando sin OTel");
            None
        }
    };

    let cfg = Config::from_env()?;
    tracing::info!(api = %cfg.api_addr, otlp = %cfg.otlp_addr, "arrancando faro");

    // Recorder Prometheus global. Tiene que instalarse antes de que cualquier
    // `metrics::counter!` se ejecute, o esas llamadas son no-ops.
    let (prom_layer, prom_handle) = observability::install();

    let storage = storage::Client::new(&cfg).await?;
    storage.wait_until_ready().await?;

    let state = Arc::new(AppState::new(cfg.clone(), storage));

    // Coordinador de apagado ordenado: SIGTERM/SIGINT ponen este watch en `true`.
    // Los servidores HTTP cierran con graceful shutdown (terminan las requests en
    // vuelo) y los ingest writers hacen un flush final del buffer, en vez de morir
    // por SIGKILL perdiendo en silencio la telemetría en vuelo en cada deploy.
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);

    // Bootstrap + caché de proyectos.
    if let Err(e) = projects::bootstrap_if_empty(&state).await {
        tracing::warn!(error = %e, "falló el bootstrap de proyectos");
    }
    if let Err(e) = state.projects.reload(&state.ch).await {
        tracing::warn!(error = %e, "falló la carga inicial de la caché de proyectos");
    }
    projects::spawn_refresh(state.clone());

    // Carga inicial + refresh periódico de integraciones (Telegram, etc.).
    if let Err(e) = state.integrations.reload(&state.ch).await {
        tracing::warn!(error = %e, "falló la carga inicial de integraciones");
    }
    integrations::spawn_refresh(state.clone());

    // Canales de notificación configurables (webhook/PagerDuty/OpsGenie/...).
    if let Err(e) = state.notification_channels.reload(&state.ch).await {
        tracing::warn!(error = %e, "falló la carga inicial de notification_channels");
    }
    notification_channels::spawn_refresh(state.clone());

    // Feature flags activas por proyecto para que los SDKs hagan evaluación local.
    if let Err(e) = state.feature_flags.reload(&state.ch).await {
        tracing::warn!(error = %e, "falló la carga inicial de feature flags");
    }
    feature_flags::spawn_refresh(state.clone());

    // Bootstrap del admin del dashboard.
    if let Err(e) = auth::bootstrap_admin_if_empty(&state).await {
        tracing::warn!(error = %e, "falló el bootstrap de admin");
    }

    // Workers en segundo plano.
    let bus = state.live_bus.clone();
    // Gauge de ocupación de los canales de ingesta — leading indicator de
    // saturación antes de que se descarten records. Arranca antes que el writer
    // (sólo lee capacity() de los senders, no toca los receivers).
    observability::spawn_channel_depth_sampler(state.clone());
    workers::start_ingest_writers(state.clone(), shutdown_tx.subscribe());
    workers::start_monitor_runner(state.clone());
    workers::start_alert_evaluator(state.clone());
    workers::start_anomaly_detector(state.clone());
    workers::start_feature_rollback_detector(state.clone());
    workers::start_error_indexer(state.clone(), bus);
    workers::start_fingerprint_compactor(state.clone());
    workers::start_stale_detector(state.clone());
    workers::start_user_unifier(state.clone());
    workers::start_session_aggregator(state.clone());

    // Tres listeners independientes: API del dashboard, OTLP/HTTP (4318) y OTLP/gRPC (4317).
    // El listener gRPC es necesario porque los SDKs oficiales de OpenTelemetry usan
    // gRPC+protobuf por defecto y no caen por sí solos al endpoint HTTP/JSON.
    //
    // `/metrics` se sirve sólo desde el listener de API — Prometheus scrapea allí.
    // El layer Prometheus mide ambos routers HTTP (API y OTLP/HTTP) para tener
    // `faro_http_request_duration_seconds` por endpoint y status.
    //
    // El handler exige `Authorization: Bearer <FARO_METRICS_TOKEN>`. Si el token
    // no está configurado devuelve 401 (fail-closed). El path ya está exento de
    // `require_session_mw` en `auth::is_public_path`.
    let metrics_token = cfg.metrics_token.clone();
    let api_router = api::router(state.clone()).route(
        "/metrics",
        axum::routing::get(move |headers: axum::http::HeaderMap| {
            let handle = prom_handle.clone();
            let token = metrics_token.clone();
            async move {
                use subtle::ConstantTimeEq as _;
                let unauthorized =
                    (axum::http::StatusCode::UNAUTHORIZED, "unauthorized\n").into_response();
                match token.as_deref() {
                    None => return unauthorized,
                    Some(expected) => {
                        let got = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.strip_prefix("Bearer "))
                            .map(str::trim)
                            .unwrap_or("");
                        if got.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() != 1 {
                            return unauthorized;
                        }
                    }
                }
                (
                    axum::http::StatusCode::OK,
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/plain; version=0.0.4",
                    )],
                    handle.render(),
                )
                    .into_response()
            }
        }),
    );
    let otlp_router = ingest::otlp_router(state.clone());

    let api_addr = cfg.api_addr.clone();
    let otlp_addr = cfg.otlp_addr.clone();
    let otlp_grpc_addr: std::net::SocketAddr = cfg.otlp_grpc_addr.parse().map_err(|e| {
        anyhow::anyhow!("FARO_OTLP_GRPC_ADDR inválido ({}): {e}", cfg.otlp_grpc_addr)
    })?;

    // CORS del listener `api`: NO se aplica a nivel de servidor. Cada sub-router
    // (dashboard, docs, ingest) lleva su propio CorsLayer dentro de `api::router`
    // porque conviven políticas distintas: el dashboard restringe a
    // `FARO_DASHBOARD_ORIGINS` con credenciales, mientras que `/api/v1/ingest/*`
    // debe aceptar cualquier origen (browser/RUM SDK en dominios de clientes). Un
    // CorsLayer server-wide cortocircuitaría el preflight OPTIONS de ingesta antes
    // de llegar al layer permisivo y devolvería 200 sin `Access-Control-Allow-Origin`.
    let mut api_task = tokio::spawn(serve(
        "api",
        api_addr,
        api_router,
        Some(prom_layer.clone()),
        None,
        shutdown_tx.subscribe(),
    ));
    // El listener OTLP/HTTP sí es 100% ingesta, así que el CORS permisivo va
    // server-wide sin conflicto.
    let mut otlp_task = tokio::spawn(serve(
        "otlp",
        otlp_addr,
        otlp_router,
        Some(prom_layer),
        Some(api::ingest_cors()),
        shutdown_tx.subscribe(),
    ));
    let otlp_grpc_state = state.clone();
    let mut otlp_grpc_task =
        tokio::spawn(
            async move { ingest::otlp_grpc::serve(otlp_grpc_state, otlp_grpc_addr).await },
        );

    // Espera la señal de apagado (SIGTERM/SIGINT) o que un servidor muera por su
    // cuenta (en cuyo caso igual iniciamos el apagado ordenado del resto).
    tokio::select! {
        _ = shutdown_signal() => {
            tracing::info!("señal de apagado recibida (SIGTERM/SIGINT), drenando…");
        }
        r = &mut api_task => tracing::error!(?r, "el servidor api terminó inesperadamente"),
        r = &mut otlp_task => tracing::error!(?r, "el servidor otlp terminó inesperadamente"),
        r = &mut otlp_grpc_task => tracing::error!(?r, "el servidor otlp/grpc terminó inesperadamente"),
    }

    // Propaga el apagado: los servidores HTTP cierran graceful (terminan las
    // requests en vuelo y `serve` retorna) y los ingest writers vacían su buffer.
    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        let _ = api_task.await;
        let _ = otlp_task.await;
    })
    .await;
    // gRPC no tiene drain propio (sus rows ya pasaron a los canales de ingesta);
    // lo abortamos tras señalar el apagado al resto.
    otlp_grpc_task.abort();
    // Margen final para que los ingest writers terminen su último flush.
    tokio::time::sleep(Duration::from_secs(2)).await;
    tracing::info!("apagado completado");
    Ok(())
}

/// Future que resuelve al recibir SIGINT (Ctrl-C) o SIGTERM (el que envían
/// `docker stop` y el redeploy). Antes sólo se escuchaba `ctrl_c` (SIGINT): en
/// cada deploy el contenedor ignoraba el SIGTERM, colgaba ~10s y moría por
/// SIGKILL sin drenar nada.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "no se pudo instalar el handler de SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

async fn serve(
    name: &'static str,
    addr: String,
    router: Router,
    prom: Option<axum_prometheus::PrometheusMetricLayer<'static>>,
    cors: Option<CorsLayer>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(%name, %addr, "escuchando");
    let mut app = router.layer(TraceLayer::new_for_http());
    // CORS opcional a nivel de servidor: el listener `api` lo deja en `None` y
    // aplica CORS por sub-router (ver `api::router`); el OTLP/HTTP usa el CORS
    // permisivo server-wide. `ingest_cors`/`dashboard_cors` viven en `api`.
    if let Some(cors) = cors {
        app = app.layer(cors);
    }
    if let Some(prom) = prom {
        app = app.layer(prom);
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(wait_for_shutdown(shutdown))
        .await?;
    tracing::info!(%name, "listener cerrado (graceful)");
    Ok(())
}

/// Resuelve cuando el coordinador de apagado pone el watch en `true`.
async fn wait_for_shutdown(mut rx: tokio::sync::watch::Receiver<bool>) {
    while !*rx.borrow_and_update() {
        if rx.changed().await.is_err() {
            break;
        }
    }
}
