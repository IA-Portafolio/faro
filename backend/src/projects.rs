//! Project token cache: tokens → project slug lookup. The cache is refreshed
//! from `faro.projects` periodically so newly created tokens propagate without
//! a backend restart.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::Deserialize;
use tokio::time::interval;

use crate::storage::{Client, ProjectRow};

#[derive(Default)]
struct CacheInner {
    by_token: std::collections::HashMap<String, String>, // token -> slug
    slugs: std::collections::HashSet<String>,
}

#[derive(Clone)]
pub struct ProjectCache {
    inner: Arc<RwLock<CacheInner>>,
}

impl ProjectCache {
    pub fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(CacheInner::default())) }
    }

    pub fn lookup(&self, token: &str) -> Option<String> {
        if token.is_empty() {
            return None;
        }
        self.inner.read().by_token.get(token).cloned()
    }

    pub fn known_slug(&self, slug: &str) -> bool {
        self.inner.read().slugs.contains(slug)
    }

    pub async fn reload(&self, ch: &Client) -> anyhow::Result<usize> {
        #[derive(Deserialize)]
        struct Row {
            slug: String,
            ingest_token: String,
        }
        let rows: Vec<Row> = ch
            .select(
                "SELECT slug, ingest_token FROM faro.projects FINAL WHERE deleted = 0",
            )
            .await?;
        let mut by_token = std::collections::HashMap::with_capacity(rows.len());
        let mut slugs = std::collections::HashSet::with_capacity(rows.len());
        for r in &rows {
            slugs.insert(r.slug.clone());
            if !r.ingest_token.is_empty() {
                by_token.insert(r.ingest_token.clone(), r.slug.clone());
            }
        }
        let n = rows.len();
        let mut guard = self.inner.write();
        guard.by_token = by_token;
        guard.slugs = slugs;
        Ok(n)
    }
}

/// Tarea en segundo plano que refresca la caché cada 15 segundos.
pub fn spawn_refresh(state: crate::state::SharedState) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(15));
        loop {
            tick.tick().await;
            if let Err(e) = state.projects.reload(&state.ch).await {
                tracing::warn!(error = %e, "falló el reload de la caché de proyectos");
            }
        }
    });
}

/// Crea un proyecto por defecto a partir de variables de entorno si no existe ningún
/// proyecto. Permite que un despliegue nuevo empiece a ingestar sin tener que llamar
/// manualmente a la API primero.
pub async fn bootstrap_if_empty(state: &crate::state::SharedState) -> anyhow::Result<()> {
    let count: Option<CountRow> = state
        .ch
        .select_one("SELECT toUInt64(count()) AS count FROM faro.projects FINAL WHERE deleted = 0")
        .await?;
    if count.map(|c| c.count).unwrap_or(0) > 0 {
        return Ok(());
    }

    let slug = std::env::var("FARO_BOOTSTRAP_PROJECT_SLUG").unwrap_or_else(|_| "default".into());
    let token = std::env::var("FARO_BOOTSTRAP_INGEST_TOKEN").ok();
    let Some(token) = token else {
        tracing::warn!(
            "no projects exist and FARO_BOOTSTRAP_INGEST_TOKEN not set; \
             create a project via POST /api/v1/projects to enable ingestion"
        );
        return Ok(());
    };

    let row = ProjectRow {
        id: uuid::Uuid::new_v4(),
        slug: slug.clone(),
        name: std::env::var("FARO_BOOTSTRAP_PROJECT_NAME").unwrap_or_else(|_| "Default".into()),
        description: "Auto-created from FARO_BOOTSTRAP_* env vars".into(),
        ingest_token: token,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        deleted: 0,
        version: 1,
    };
    state.ch.insert("faro.projects", &[row]).await?;
    tracing::info!(%slug, "proyecto de bootstrap creado");
    Ok(())
}

#[derive(Deserialize)]
struct CountRow {
    count: u64,
}
