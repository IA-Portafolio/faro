-- Reglas de alerta: umbrales declarativos evaluados periódicamente contra ClickHouse.
CREATE TABLE IF NOT EXISTS faro.alert_rules
(
    id                    UUID,
    project_id            LowCardinality(String) DEFAULT 'default',
    name                  String,
    description           String                 DEFAULT '',
    source                LowCardinality(String),               -- logs / spans / metrics / monitors
    query                 String,                               -- SELECT expression returning single Float64
    condition             LowCardinality(String),               -- gt / lt / eq / gte / lte
    threshold             Float64,
    window_seconds        UInt32                 DEFAULT 300,
    interval_seconds      UInt32                 DEFAULT 60,
    severity              LowCardinality(String) DEFAULT 'warn',
    notification_targets  Array(String),                        -- webhook URLs
    enabled               UInt8                  DEFAULT 1,
    created_at            DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_at            DateTime64(3, 'UTC')   DEFAULT now64(3),
    deleted               UInt8                  DEFAULT 0,
    version               UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY id;

CREATE TABLE IF NOT EXISTS faro.alert_incidents
(
    id           UUID,
    project_id   LowCardinality(String) DEFAULT 'default',
    rule_id      UUID,
    rule_name    String,
    started_at   DateTime64(3, 'UTC')   CODEC(Delta, ZSTD(1)),
    resolved_at  Nullable(DateTime64(3, 'UTC')),
    value        Float64                CODEC(Gorilla, ZSTD(1)),
    threshold    Float64                CODEC(Gorilla, ZSTD(1)),
    severity     LowCardinality(String),
    status       LowCardinality(String),                       -- firing / resolved
    note         String                 DEFAULT '',
    version      UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
PARTITION BY toDate(started_at)
ORDER BY (rule_id, started_at, id);
