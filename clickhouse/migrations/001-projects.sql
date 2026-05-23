-- Migración manual: añade soporte de proyectos a una instancia existente.
-- Idempotente: usa IF NOT EXISTS / IF NOT EXISTS también en columnas.

CREATE TABLE IF NOT EXISTS faro.projects
(
    id             UUID,
    slug           LowCardinality(String),
    name           String,
    description    String                 DEFAULT '',
    ingest_token   String,
    created_at     DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_at     DateTime64(3, 'UTC')   DEFAULT now64(3),
    deleted        UInt8                  DEFAULT 0,
    version        UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY id;

ALTER TABLE faro.logs                ADD COLUMN IF NOT EXISTS project_id LowCardinality(String) DEFAULT 'default' AFTER observed_timestamp;
ALTER TABLE faro.spans               ADD COLUMN IF NOT EXISTS project_id LowCardinality(String) DEFAULT 'default' AFTER timestamp;
ALTER TABLE faro.metrics             ADD COLUMN IF NOT EXISTS project_id LowCardinality(String) DEFAULT 'default' AFTER timestamp;
ALTER TABLE faro.error_events        ADD COLUMN IF NOT EXISTS project_id LowCardinality(String) DEFAULT 'default' AFTER timestamp;
ALTER TABLE faro.error_issue_status  ADD COLUMN IF NOT EXISTS project_id LowCardinality(String) DEFAULT 'default' FIRST;
ALTER TABLE faro.api_monitors        ADD COLUMN IF NOT EXISTS project_id LowCardinality(String) DEFAULT 'default' AFTER id;
ALTER TABLE faro.monitor_results     ADD COLUMN IF NOT EXISTS project_id LowCardinality(String) DEFAULT 'default' AFTER monitor_id;
ALTER TABLE faro.alert_rules         ADD COLUMN IF NOT EXISTS project_id LowCardinality(String) DEFAULT 'default' AFTER id;
ALTER TABLE faro.alert_incidents     ADD COLUMN IF NOT EXISTS project_id LowCardinality(String) DEFAULT 'default' AFTER id;

-- Skip index para acelerar filtros por proyecto.
ALTER TABLE faro.logs    ADD INDEX IF NOT EXISTS idx_project project_id TYPE bloom_filter(0.01) GRANULARITY 4;
ALTER TABLE faro.spans   ADD INDEX IF NOT EXISTS idx_project project_id TYPE bloom_filter(0.01) GRANULARITY 4;
ALTER TABLE faro.metrics ADD INDEX IF NOT EXISTS idx_project project_id TYPE bloom_filter(0.01) GRANULARITY 4;
