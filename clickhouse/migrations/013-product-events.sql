-- Product events: 6º pilar de Faro (telemetría de uso de las apps de los clientes).
-- Ver clickhouse/init/85-product-events.sql para la definición canónica y rationale.
CREATE TABLE IF NOT EXISTS faro.product_events
(
    timestamp        DateTime64(9, 'UTC')   CODEC(Delta, ZSTD(1)),
    project_id       LowCardinality(String) DEFAULT 'default',
    event_name       LowCardinality(String),
    distinct_id      String                 CODEC(ZSTD(1)),
    anonymous_id     String                 DEFAULT '' CODEC(ZSTD(1)),
    session_id       String                 DEFAULT '' CODEC(ZSTD(1)),
    properties       String                 DEFAULT '' CODEC(ZSTD(3)),
    user_properties  String                 DEFAULT '' CODEC(ZSTD(3)),
    context          String                 DEFAULT '' CODEC(ZSTD(3)),
    source           LowCardinality(String) DEFAULT 'web',
    trace_id         String                 DEFAULT '' CODEC(ZSTD(1)),
    span_id          String                 DEFAULT '' CODEC(ZSTD(1)),
    event_id         UUID                   DEFAULT generateUUIDv4(),
    INDEX idx_event_name event_name TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_distinct distinct_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_anonymous anonymous_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_session session_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_trace trace_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_span span_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_project project_id TYPE bloom_filter(0.01) GRANULARITY 4,
    PROJECTION by_event
    (
        SELECT *
        ORDER BY (project_id, event_name, timestamp)
    )
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (project_id, timestamp, event_name)
TTL toDateTime(timestamp) + INTERVAL 365 DAY
SETTINGS index_granularity = 8192;
