//! Configuración de integraciones externas almacenada en ClickHouse y cacheada
//! en memoria. Cada `kind` ('telegram', etc.) tiene una única fila; el campo
//! `config` es JSON específico de la integración.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::time::{interval, MissedTickBehavior};

use crate::state::SharedState;
use crate::storage::{Client, IntegrationRow};

pub const KIND_TELEGRAM: &str = "telegram";

/// Config concreta de la integración de Telegram. Persistida como JSON en
/// `faro.integrations.config`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TelegramConfig {
    #[serde(default)]
    pub bot_token: String,
    /// Chat ID por defecto sugerido en la UI (opcional, no se aplica
    /// automáticamente — sigue siendo cada regla la que define sus destinos).
    #[serde(default)]
    pub default_chat_id: String,
}

impl TelegramConfig {
    pub fn is_configured(&self) -> bool {
        !self.bot_token.trim().is_empty()
    }
}

/// Snapshot in-memory de las integraciones. Se refresca cada 15 s y también
/// cuando un endpoint las modifica.
#[derive(Clone, Default)]
pub struct IntegrationsSnapshot {
    pub telegram: Option<TelegramConfig>,
}

#[derive(Clone, Default)]
pub struct IntegrationsCache {
    inner: Arc<RwLock<IntegrationsSnapshot>>,
}

impl IntegrationsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn telegram(&self) -> Option<TelegramConfig> {
        self.inner.read().telegram.clone()
    }

    pub async fn reload(&self, ch: &Client) -> Result<()> {
        let rows: Vec<IntegrationRow> = ch
            .select(
                "SELECT kind, enabled, config, updated_at, updated_by, version \
                 FROM faro.integrations FINAL",
            )
            .await?;
        let mut snap = IntegrationsSnapshot::default();
        for row in rows {
            if row.enabled == 0 {
                continue;
            }
            if row.kind == KIND_TELEGRAM {
                match serde_json::from_str::<TelegramConfig>(&row.config) {
                    Ok(cfg) => snap.telegram = Some(cfg),
                    Err(e) => tracing::warn!(error = %e, "telegram config inválida en DB"),
                }
            }
        }
        *self.inner.write() = snap;
        Ok(())
    }
}

/// Tarea en segundo plano que refresca el cache periódicamente para captar
/// cambios hechos por otra réplica del backend o vía SQL manual.
pub fn spawn_refresh(state: SharedState) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(15));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = state.integrations.reload(&state.ch).await {
                tracing::warn!(error = %e, "falló el reload de integraciones");
            }
        }
    });
}

/// Upserta la integración de Telegram. `version` se actualiza con el timestamp
/// para que ReplacingMergeTree desambigüe correctamente.
pub async fn upsert_telegram(
    ch: &Client,
    cfg: &TelegramConfig,
    enabled: bool,
    actor_email: &str,
) -> Result<IntegrationRow> {
    let now = Utc::now();
    let row = IntegrationRow {
        kind: KIND_TELEGRAM.into(),
        enabled: if enabled { 1 } else { 0 },
        config: serde_json::to_string(cfg)?,
        updated_at: now,
        updated_by: actor_email.to_string(),
        version: now.timestamp_millis() as u64,
    };
    ch.insert("faro.integrations", &[row.clone()]).await?;
    Ok(row)
}
