-- Migración idempotente: tabla de integraciones externas para instancias existentes.
-- El backend persiste aquí la config de Telegram (token + chat_id por defecto)
-- y cualquier futura integración con el mismo patrón kind/config-JSON.
CREATE TABLE IF NOT EXISTS faro.integrations
(
    kind         LowCardinality(String),
    enabled      UInt8                  DEFAULT 1,
    config       String                 DEFAULT '{}',
    updated_at   DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_by   String                 DEFAULT '',
    version      UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY kind;
