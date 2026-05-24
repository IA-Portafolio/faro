//! Canales de notificación configurables — análogo a `integrations.rs` pero
//! multi-instancia. Cada canal tiene un `id` único, un `kind` (selecciona el
//! plugin `Notifier`) y un blob `config` JSON.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use parking_lot::RwLock;
use tokio::time::{interval, MissedTickBehavior};

use crate::state::SharedState;
use crate::storage::{Client, NotificationChannelRow};

/// Snapshot inmutable de un canal — el cache lo entrega clonado para que el
/// caller no retenga el lock del RwLock mientras hace I/O.
#[derive(Clone, Debug)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub config: String,
}

#[derive(Clone, Default)]
pub struct NotificationChannelsCache {
    inner: Arc<RwLock<HashMap<String, Channel>>>,
}

impl NotificationChannelsCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Devuelve sólo si está habilitado (no deleted, enabled=1). Los disabled
    /// no se cachean — esto evita resolver `channel://x` a un canal que el
    /// admin acaba de desactivar.
    pub fn get(&self, id: &str) -> Option<Channel> {
        self.inner.read().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Channel> {
        let mut out: Vec<_> = self.inner.read().values().cloned().collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    pub async fn reload(&self, ch: &Client) -> Result<()> {
        let rows: Vec<NotificationChannelRow> = ch
            .select(
                "SELECT id, name, kind, enabled, config, \
                        created_at, updated_at, updated_by, deleted, version \
                 FROM faro.notification_channels FINAL",
            )
            .await?;
        let mut map = HashMap::new();
        for r in rows {
            if r.deleted == 1 || r.enabled == 0 {
                continue;
            }
            map.insert(
                r.id.clone(),
                Channel {
                    id: r.id,
                    name: r.name,
                    kind: r.kind,
                    config: r.config,
                },
            );
        }
        *self.inner.write() = map;
        Ok(())
    }
}

pub fn spawn_refresh(state: SharedState) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(15));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = state.notification_channels.reload(&state.ch).await {
                tracing::warn!(error = %e, "falló reload de notification_channels");
            }
        }
    });
}

/// Lee la fila completa (incluye campos no-cacheados) directo de DB.
/// Útil para la API que muestra audit metadata (`updated_at`, `updated_by`).
pub async fn read_one(ch: &Client, id: &str) -> Result<Option<NotificationChannelRow>> {
    ch.select_one_with_params::<NotificationChannelRow>(
        "SELECT id, name, kind, enabled, config, \
                created_at, updated_at, updated_by, deleted, version \
         FROM faro.notification_channels FINAL WHERE id = {id:String} AND deleted = 0 LIMIT 1",
        &[("id", id)],
    )
    .await
}

pub async fn list_all(ch: &Client) -> Result<Vec<NotificationChannelRow>> {
    ch.select::<NotificationChannelRow>(
        "SELECT id, name, kind, enabled, config, \
                created_at, updated_at, updated_by, deleted, version \
         FROM faro.notification_channels FINAL WHERE deleted = 0 ORDER BY id",
    )
    .await
}

/// Upsert. La caller construye la `NotificationChannelRow` con los campos
/// editables; aquí seteamos `updated_at/updated_by/version` y persistimos.
pub async fn upsert(
    ch: &Client,
    mut row: NotificationChannelRow,
    actor_email: &str,
) -> Result<NotificationChannelRow> {
    let now = Utc::now();
    row.updated_at = now;
    row.updated_by = actor_email.to_string();
    row.version = now.timestamp_millis() as u64;
    // Para nuevos canales, `created_at` viene como Utc::now() por default del serde
    // pero si el caller lo dejó del default (epoch o now()), lo respetamos. Cuando
    // se está editando uno existente, el caller debe pasar el `created_at` original.
    ch.insert("faro.notification_channels", &[row.clone()])
        .await?;
    Ok(row)
}

/// Tombstone lógico: marca deleted=1 con bump de version. ReplacingMergeTree
/// con FINAL hará que las queries no vean el canal después del merge.
pub async fn soft_delete(ch: &Client, id: &str, actor_email: &str) -> Result<()> {
    let existing = read_one(ch, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("canal '{id}' no existe"))?;
    let now = Utc::now();
    let row = NotificationChannelRow {
        deleted: 1,
        enabled: 0,
        updated_at: now,
        updated_by: actor_email.to_string(),
        version: now.timestamp_millis() as u64,
        ..existing
    };
    ch.insert("faro.notification_channels", &[row]).await?;
    Ok(())
}
