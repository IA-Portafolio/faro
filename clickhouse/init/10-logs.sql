-- Logs: registros de log estructurados. Columnas alineadas con OTel más campos ergonómicos.
CREATE TABLE IF NOT EXISTS faro.logs
(
    timestamp           DateTime64(9, 'UTC')                CODEC(Delta, ZSTD(1)),
    observed_timestamp  DateTime64(9, 'UTC')                CODEC(Delta, ZSTD(1)),
    project_id          LowCardinality(String)              DEFAULT 'default',
    service_name        LowCardinality(String),
    severity_text       LowCardinality(String),
    severity_number     UInt8                               CODEC(T64, ZSTD(1)),
    body                String                              CODEC(ZSTD(3)),
    trace_id            String                              CODEC(ZSTD(1)),
    span_id             String                              CODEC(ZSTD(1)),
    scope_name          LowCardinality(String),
    resource_attributes Map(LowCardinality(String), String) CODEC(ZSTD(1)),
    attributes          Map(LowCardinality(String), String) CODEC(ZSTD(1)),
    log_id              UUID                                DEFAULT generateUUIDv4(),
    INDEX idx_body body TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 4,
    INDEX idx_trace trace_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_project project_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_attrs_keys mapKeys(attributes) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_attrs_values mapValues(attributes) TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (service_name, severity_number, timestamp)
TTL toDateTime(timestamp) + INTERVAL 30 DAY
SETTINGS index_granularity = 8192;

-- Agregación por minuto para los sparklines e histogramas del dashboard.
CREATE TABLE IF NOT EXISTS faro.logs_stats
(
    minute        DateTime               CODEC(Delta, ZSTD(1)),
    service_name  LowCardinality(String),
    severity_text LowCardinality(String),
    count         AggregateFunction(count, UInt64)
)
ENGINE = AggregatingMergeTree
PARTITION BY toDate(minute)
ORDER BY (minute, service_name, severity_text)
TTL toDateTime(minute) + INTERVAL 90 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS faro.mv_logs_stats TO faro.logs_stats AS
SELECT
    toStartOfMinute(timestamp) AS minute,
    service_name,
    severity_text,
    countState() AS count
FROM faro.logs
GROUP BY minute, service_name, severity_text;
