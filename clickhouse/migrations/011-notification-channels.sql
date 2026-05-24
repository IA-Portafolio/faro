-- Migración 009 — Canales de notificación configurables (2.10 del audit).
--
-- La tabla `faro.integrations` que ya existe es **singleton-por-kind** (una sola
-- fila por tipo de integración, p.ej. un único Telegram global). Eso era suficiente
-- mientras sólo había Telegram, pero ya no escala: queremos múltiples webhooks,
-- múltiples destinos PagerDuty/OpsGenie/Slack/Discord, varias direcciones de email,
-- cada uno con su propia config y referenciable individualmente desde las reglas
-- de alerta vía `notification_targets`.
--
-- Diseño:
--   - PK por `id` (slug human-readable, e.g. `ops-pagerduty`, `team-slack`).
--   - `kind` selecciona qué Notifier construir en runtime
--     (`webhook` | `slack` | `discord` | `pagerduty` | `opsgenie` |
--      `email_resend` | `telegram`).
--   - `config` es JSON con la forma específica de cada kind — el backend valida al
--     deserializar.
--   - ReplacingMergeTree(version) para upserts idempotentes; `deleted` para
--     tombstones lógicos (un delete pone deleted=1 + bump version).
--
-- Compat: las reglas de alerta existentes con targets `tg://...` o `https://...`
-- siguen funcionando — el dispatcher resuelve esos formatos directamente sin
-- consultar esta tabla. El formato nuevo `channel://<id>` resuelve aquí.

CREATE TABLE IF NOT EXISTS faro.notification_channels
(
    id           String,
    name         String                 DEFAULT '',
    kind         LowCardinality(String),
    enabled      UInt8                  DEFAULT 1,
    -- JSON con la config concreta del notifier. El schema vive del lado de Rust
    -- (cada `notify::*::Config` lo deserializa con serde).
    config       String                 DEFAULT '',
    created_at   DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_at   DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_by   String                 DEFAULT '',
    deleted      UInt8                  DEFAULT 0,
    version      UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY id;
