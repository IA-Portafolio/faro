-- API monitors: synthetic HTTP checks executed on a schedule.
CREATE TABLE IF NOT EXISTS faro.api_monitors
(
    id                   UUID,
    project_id           LowCardinality(String)            DEFAULT 'default',
    name                 String,
    method               LowCardinality(String),
    url                  String,
    headers              Map(String, String),
    body                 String                 DEFAULT '',
    interval_seconds     UInt32                 DEFAULT 60,
    timeout_seconds      UInt32                 DEFAULT 30,
    expected_status_min  UInt16                 DEFAULT 200,
    expected_status_max  UInt16                 DEFAULT 299,
    expected_body_regex  String                 DEFAULT '',
    enabled              UInt8                  DEFAULT 1,
    created_at           DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_at           DateTime64(3, 'UTC')   DEFAULT now64(3),
    deleted              UInt8                  DEFAULT 0,
    version              UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY id;

CREATE TABLE IF NOT EXISTS faro.monitor_results
(
    monitor_id          UUID,
    project_id          LowCardinality(String) DEFAULT 'default',
    timestamp           DateTime64(3, 'UTC')   CODEC(Delta, ZSTD(1)),
    success             UInt8                  CODEC(T64, ZSTD(1)),
    status_code         UInt16                 CODEC(T64, ZSTD(1)),
    duration_ms         UInt32                 CODEC(T64, ZSTD(1)),
    error_message       String                 CODEC(ZSTD(3)),
    response_size       UInt32                 CODEC(T64, ZSTD(1)),
    INDEX idx_monitor monitor_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (monitor_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 60 DAY;
