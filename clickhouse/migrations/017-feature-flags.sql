-- Feature flags: definiciones por proyecto servidas a SDKs para evaluación local.
CREATE TABLE IF NOT EXISTS faro.feature_flags
(
    project_id          LowCardinality(String) DEFAULT 'default',
    key                 String,
    rollout_percentage  UInt8                  DEFAULT 0,
    conditions          String                 DEFAULT '',
    active              UInt8                  DEFAULT 1,
    updated_at          DateTime64(3, 'UTC')   DEFAULT now64(3),
    version             UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY (project_id, key);
