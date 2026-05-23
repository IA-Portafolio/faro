use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

mod api;
mod auth;
mod config;
mod error;
mod fingerprint;
mod ingest;
mod notify;
mod openapi;
mod projects;
mod state;
mod storage;
mod stream;
mod versions;
mod workers;

use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,faro=debug")))
        .with_target(true)
        .init();

    let cfg = Config::from_env()?;
    tracing::info!(api = %cfg.api_addr, otlp = %cfg.otlp_addr, "arrancando faro");

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

    // Bootstrap del admin del dashboard.
    if let Err(e) = auth::bootstrap_admin_if_empty(&state).await {
        tracing::warn!(error = %e, "falló el bootstrap de admin");
    }

    // Workers en segundo plano.
    let bus = state.live_bus.clone();
    workers::start_ingest_writers(state.clone());
    workers::start_monitor_runner(state.clone());
    workers::start_alert_evaluator(state.clone());
    workers::start_error_indexer(state.clone(), bus);

    // Dos listeners para poder exponer OTLP y la API del dashboard de forma independiente.
    let api_router = api::router(state.clone());
    let otlp_router = ingest::otlp_router(state.clone());

    let api_addr = cfg.api_addr.clone();
    let otlp_addr = cfg.otlp_addr.clone();

    let api_task = tokio::spawn(serve("api", api_addr, api_router));
    let otlp_task = tokio::spawn(serve("otlp", otlp_addr, otlp_router));

    tokio::select! {
        _ = signal::ctrl_c() => tracing::info!("ctrl-c received, shutting down"),
        r = api_task => { tracing::error!(?r, "el servidor api terminó"); }
        r = otlp_task => { tracing::error!(?r, "el servidor otlp terminó"); }
    }

    Ok(())
}

async fn serve(name: &'static str, addr: String, router: Router) -> Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(%name, %addr, "escuchando");
    let app = router
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );
    axum::serve(listener, app).await?;
    Ok(())
}
