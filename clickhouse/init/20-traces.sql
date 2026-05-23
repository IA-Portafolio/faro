-- Distributed tracing: one row per span. Trace = collection of spans sharing trace_id.
CREATE TABLE IF NOT EXISTS faro.spans
(
    timestamp           DateTime64(9, 'UTC')                CODEC(Delta, ZSTD(1)),
    project_id          LowCardinality(String)              DEFAULT 'default',
    trace_id            String                              CODEC(ZSTD(1)),
    span_id             String                              CODEC(ZSTD(1)),
    parent_span_id      String                              CODEC(ZSTD(1)),
    trace_state         String,
    name                LowCardinality(String),
    kind                LowCardinality(String),
    service_name        LowCardinality(String),
    duration_ns         UInt64                              CODEC(T64, ZSTD(1)),
    status_code         LowCardinality(String),
    status_message      String,
    resource_attributes Map(LowCardinality(String), String) CODEC(ZSTD(1)),
    span_attributes     Map(LowCardinality(String), String) CODEC(ZSTD(1)),
    events_timestamps   Array(DateTime64(9, 'UTC'))         CODEC(ZSTD(1)),
    events_names        Array(LowCardinality(String)),
    events_attributes   Array(String)                       CODEC(ZSTD(1)),
    links_trace_ids     Array(String)                       CODEC(ZSTD(1)),
    links_span_ids      Array(String)                       CODEC(ZSTD(1)),
    INDEX idx_trace_id trace_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_name name TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_status status_code TYPE set(8) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (service_name, name, timestamp)
TTL toDateTime(timestamp) + INTERVAL 14 DAY
SETTINGS index_granularity = 8192;

-- Root-span index to quickly list traces (one row per trace).
CREATE TABLE IF NOT EXISTS faro.traces_index
(
    timestamp      DateTime64(3, 'UTC')   CODEC(Delta, ZSTD(1)),
    trace_id       String                 CODEC(ZSTD(1)),
    service_name   LowCardinality(String),
    root_name      LowCardinality(String),
    duration_ns    UInt64                 CODEC(T64, ZSTD(1)),
    status_code    LowCardinality(String),
    span_count     UInt32                 CODEC(T64, ZSTD(1))
)
ENGINE = ReplacingMergeTree(timestamp)
PARTITION BY toDate(timestamp)
ORDER BY (timestamp, trace_id)
TTL toDateTime(timestamp) + INTERVAL 14 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS faro.mv_traces_index TO faro.traces_index AS
SELECT
    timestamp,
    trace_id,
    service_name,
    name AS root_name,
    duration_ns,
    status_code,
    toUInt32(1) AS span_count
FROM faro.spans
WHERE parent_span_id = '' OR parent_span_id = '0000000000000000';
