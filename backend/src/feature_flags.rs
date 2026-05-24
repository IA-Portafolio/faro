//! Caché de feature flags activas por proyecto.
//!
//! Los SDKs descargan estas definiciones y evalúan localmente. Por eso las
//! conditions no deben contener secretos: son configuración de targeting, no
//! política de autorización.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::interval;

use crate::storage::{Client, FeatureFlagRow};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SdkFeatureFlag {
    pub key: String,
    pub rollout_percentage: u8,
    pub conditions: Value,
}

#[derive(Default)]
struct CacheInner {
    by_project: HashMap<String, Vec<SdkFeatureFlag>>,
}

#[derive(Clone, Default)]
pub struct FeatureFlagsCache {
    inner: Arc<RwLock<CacheInner>>,
}

impl FeatureFlagsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn flags_for_project(&self, project: &str) -> Vec<SdkFeatureFlag> {
        self.inner
            .read()
            .by_project
            .get(project)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn reload(&self, ch: &Client) -> anyhow::Result<usize> {
        let rows: Vec<FeatureFlagRow> = ch
            .select(
                "SELECT project_id, key, rollout_percentage, conditions, active, updated_at, version \
                 FROM faro.feature_flags FINAL WHERE active = 1",
            )
            .await?;

        let mut by_project: HashMap<String, Vec<SdkFeatureFlag>> = HashMap::new();
        for row in &rows {
            let conditions = parse_conditions(&row.conditions);
            by_project
                .entry(row.project_id.clone())
                .or_default()
                .push(SdkFeatureFlag {
                    key: row.key.clone(),
                    rollout_percentage: row.rollout_percentage.min(100),
                    conditions,
                });
        }

        let n = rows.len();
        self.inner.write().by_project = by_project;
        Ok(n)
    }
}

fn parse_conditions(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return serde_json::json!({});
    }
    match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "feature flag conditions JSON inválido; usando objeto vacío");
            serde_json::json!({})
        }
    }
}

pub fn spawn_refresh(state: crate::state::SharedState) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            if let Err(e) = state.feature_flags.reload(&state.ch).await {
                tracing::warn!(error = %e, "falló el reload de feature flags");
            }
        }
    });
}
