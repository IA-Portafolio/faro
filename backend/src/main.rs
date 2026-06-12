//! Binario del servidor Faro: arranca y orquesta todo el proceso.
//!
//! Inicializa logging/telemetría, carga la `Config` del entorno, conecta a
//! ClickHouse, lanza los workers en segundo plano y sirve dos superficies: la API
//! del dashboard (`:8080`) y los listeners de ingesta OTLP HTTP (`:4318`) y gRPC
//! (`:4317`). Gestiona el apagado ordenado ante SIGTERM. La lógica reutilizable
//! vive en la crate `faro` (`lib.rs`); esto es solo el `fn main`.

use std::sync::Arc;

use anyhow::Result;
use axum::http::HeaderValue;
use axum::response::IntoResponse;
use axum::Router;
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::cors::{Any, CorsLayer};
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
    workers::start_ingest_writers(state.clone());
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

    let api_task = tokio::spawn(serve(
        "api",
        api_addr,
        api_router,
        Some(prom_layer.clone()),
        dashboard_cors(&cfg.dashboard_origins),
    ));
    let otlp_task = tokio::spawn(serve(
        "otlp",
        otlp_addr,
        otlp_router,
        Some(prom_layer),
        ingest_cors(),
    ));
    let otlp_grpc_state = state.clone();
    let otlp_grpc_task =
        tokio::spawn(
            async move { ingest::otlp_grpc::serve(otlp_grpc_state, otlp_grpc_addr).await },
        );

    tokio::select! {
        _ = signal::ctrl_c() => tracing::info!("ctrl-c received, shutting down"),
        r = api_task => { tracing::error!(?r, "el servidor api terminó"); }
        r = otlp_task => { tracing::error!(?r, "el servidor otlp terminó"); }
        r = otlp_grpc_task => { tracing::error!(?r, "el servidor otlp/grpc terminó"); }
    }

    Ok(())
}

fn dashboard_cors(origins: &[String]) -> CorsLayer {
    if origins.is_empty() {
        // Dev: sólo orígenes localhost típicos.  Sin credenciales — el browser
        // no envía cookies a orígenes no listados explícitamente.
        let dev: Vec<HeaderValue> = [
            "http://localhost:5173",
            "http://localhost:3000",
            "http://localhost:8080",
            "http://127.0.0.1:5173",
            "http://127.0.0.1:3000",
            "http://127.0.0.1:8080",
        ]
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();
        CorsLayer::new()
            .allow_origin(dev)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let vals: Vec<HeaderValue> = origins.iter().filter_map(|o| o.parse().ok()).collect();
        CorsLayer::new()
            .allow_origin(vals)
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_credentials(true)
    }
}

fn ingest_cors() -> CorsLayer {
    // Los SDKs de telemetría web se originan desde cualquier dominio del cliente;
    // no usan cookies — usan API keys en headers. `Any` es correcto aquí.
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

async fn serve(
    name: &'static str,
    addr: String,
    router: Router,
    prom: Option<axum_prometheus::PrometheusMetricLayer<'static>>,
    cors: CorsLayer,
) -> Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(%name, %addr, "escuchando");
    let mut app = router.layer(TraceLayer::new_for_http()).layer(cors);
    if let Some(prom) = prom {
        app = app.layer(prom);
    }
    axum::serve(listener, app).await?;
    Ok(())
}
