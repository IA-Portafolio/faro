-- Tablas auxiliares y MVs del 6º pilar (product events).
-- Ver clickhouse/init/86-product-events-aux.sql para la definición canónica y rationale.

CREATE TABLE IF NOT EXISTS faro.product_users
(
    project_id   LowCardinality(String) DEFAULT 'default',
    distinct_id  String                 CODEC(ZSTD(1)),
    first_seen   DateTime64(9, 'UTC')   CODEC(Delta, ZSTD(1)),
    last_seen    DateTime64(9, 'UTC')   CODEC(Delta, ZSTD(1)),
    properties   String                 DEFAULT '' CODEC(ZSTD(3)),
    INDEX idx_distinct distinct_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(last_seen)
ORDER BY (project_id, distinct_id)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS faro.product_sessions
(
    project_id       LowCardinality(String) DEFAULT 'default',
    session_id       String                 CODEC(ZSTD(1)),
    distinct_id      String                 CODEC(ZSTD(1)),
    started_at       DateTime64(9, 'UTC')   CODEC(Delta, ZSTD(1)),
    ended_at         DateTime64(9, 'UTC')   CODEC(Delta, ZSTD(1)),
    page_count       UInt32                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    duration_seconds UInt32                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    source           LowCardinality(String) DEFAULT 'web',
    INDEX idx_session session_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_distinct distinct_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(ended_at)
ORDER BY (project_id, session_id)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS faro.product_events_per_day
(
    day        Date                   CODEC(Delta, ZSTD(1)),
    project_id LowCardinality(String),
    event_name LowCardinality(String),
    count      AggregateFunction(count, UInt64)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(day)
ORDER BY (day, project_id, event_name)
TTL day + INTERVAL 365 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS faro.mv_product_events_per_day
TO faro.product_events_per_day AS
SELECT
    toDate(timestamp) AS day,
    project_id,
    event_name,
    countState() AS count
FROM faro.product_events
GROUP BY day, project_id, event_name;

CREATE TABLE IF NOT EXISTS faro.product_unique_users_per_day
(
    day          Date                   CODEC(Delta, ZSTD(1)),
    project_id   LowCardinality(String),
    unique_users AggregateFunction(uniqExact, String)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(day)
ORDER BY (day, project_id)
TTL day + INTERVAL 365 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS faro.mv_product_unique_users_per_day
TO faro.product_unique_users_per_day AS
SELECT
    toDate(timestamp) AS day,
    project_id,
    uniqExactState(distinct_id) AS unique_users
FROM faro.product_events
GROUP BY day, project_id;
