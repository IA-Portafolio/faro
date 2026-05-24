-- Configuración de integraciones externas (Telegram, Slack, etc.).
-- Una fila por integración. `config` es JSON serializado por el backend para
-- evitar tener que migrar el esquema cada vez que añadimos un campo a una
-- integración concreta.
CREATE TABLE IF NOT EXISTS faro.integrations
(
    kind         LowCardinality(String),               -- 'telegram', 'slack', ...
    enabled      UInt8                  DEFAULT 1,
    config       String                 DEFAULT '{}',  -- JSON
    updated_at   DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_by   String                 DEFAULT '',
    version      UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY kind;
