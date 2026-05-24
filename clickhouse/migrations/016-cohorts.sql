-- Cohorts: segmentación de usuarios sobre `faro.product_events`.
-- Ver clickhouse/init/87-cohorts.sql para rationale.
CREATE TABLE IF NOT EXISTS faro.cohorts
(
    id            UUID,
    project_id    LowCardinality(String) DEFAULT 'default',
    name          String,
    description   String                 DEFAULT '',
    definition    String,
    created_at    DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_at    DateTime64(3, 'UTC')   DEFAULT now64(3),
    created_by    String                 DEFAULT '',
    deleted       UInt8                  DEFAULT 0,
    version       UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY id;
