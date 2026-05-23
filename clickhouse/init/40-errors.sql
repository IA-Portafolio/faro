-- Errors: individual events captured from logs/spans with exception data.
CREATE TABLE IF NOT EXISTS faro.error_events
(
    timestamp         DateTime64(9, 'UTC')                CODEC(Delta, ZSTD(1)),
    project_id        LowCardinality(String)              DEFAULT 'default',
    fingerprint       String                              CODEC(ZSTD(1)),
    service_name      LowCardinality(String),
    severity_text     LowCardinality(String),
    message           String                              CODEC(ZSTD(3)),
    exception_type    LowCardinality(String),
    exception_message String                              CODEC(ZSTD(3)),
    stack_trace       String                              CODEC(ZSTD(3)),
    trace_id          String                              CODEC(ZSTD(1)),
    span_id           String                              CODEC(ZSTD(1)),
    log_id            UUID                                DEFAULT generateUUIDv4(),
    attributes        Map(LowCardinality(String), String) CODEC(ZSTD(1)),
    INDEX idx_fingerprint fingerprint TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (service_name, fingerprint, timestamp)
TTL toDateTime(timestamp) + INTERVAL 30 DAY
SETTINGS index_granularity = 8192;

-- Issue status (resolved / ignored / unresolved). Counts are computed from error_events on read.
CREATE TABLE IF NOT EXISTS faro.error_issue_status
(
    project_id   LowCardinality(String) DEFAULT 'default',
    service_name LowCardinality(String),
    fingerprint  String,
    status       LowCardinality(String) DEFAULT 'unresolved',
    assignee     String                 DEFAULT '',
    note         String                 DEFAULT '',
    updated_at   DateTime64(3, 'UTC')   DEFAULT now64(3),
    version      UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY (project_id, service_name, fingerprint);
