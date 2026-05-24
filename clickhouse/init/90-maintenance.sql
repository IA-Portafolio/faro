-- Tablas y MVs para los workers de mantenimiento (ver docs/adr o migration 008).
-- Este init corre en DB fresca; la migración 008-maintenance-workers.sql es la
-- ruta para actualizar instalaciones existentes. Mantener ambos en sync.

-- ----------- Pre-agregaciones -----------

CREATE TABLE IF NOT EXISTS faro.errors_hourly
(
    hour          DateTime               CODEC(Delta, ZSTD(1)),
    project_id    LowCardinality(String),
    service_name  LowCardinality(String),
    severity_text LowCardinality(String),
    count         AggregateFunction(count, UInt64)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(hour)
ORDER BY (hour, project_id, service_name, severity_text)
TTL toDateTime(hour) + INTERVAL 365 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS faro.mv_errors_hourly TO faro.errors_hourly AS
SELECT
    toStartOfHour(timestamp) AS hour,
    project_id,
    service_name,
    severity_text,
    countState() AS count
FROM faro.logs
WHERE severity_number >= 17
GROUP BY hour, project_id, service_name, severity_text;

CREATE TABLE IF NOT EXISTS faro.spans_latency_hourly
(
    hour               DateTime               CODEC(Delta, ZSTD(1)),
    project_id         LowCardinality(String),
    service_name       LowCardinality(String),
    span_name          LowCardinality(String),
    span_count         AggregateFunction(count, UInt64),
    error_count        AggregateFunction(sumIf, UInt64, UInt8),
    duration_quantiles AggregateFunction(quantilesTDigest(0.5, 0.95, 0.99), Float64)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(hour)
ORDER BY (hour, project_id, service_name, span_name)
TTL toDateTime(hour) + INTERVAL 90 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS faro.mv_spans_latency_hourly TO faro.spans_latency_hourly AS
SELECT
    toStartOfHour(timestamp) AS hour,
    project_id,
    service_name,
    name AS span_name,
    countState() AS span_count,
    sumIfState(toUInt64(1), status_code = 'ERROR') AS error_count,
    quantilesTDigestState(0.5, 0.95, 0.99)(toFloat64(duration_ns) / 1000000.0) AS duration_quantiles
FROM faro.spans
GROUP BY hour, project_id, service_name, span_name;

CREATE TABLE IF NOT EXISTS faro.monitor_uptime_daily
(
    day                Date                   CODEC(Delta, ZSTD(1)),
    project_id         LowCardinality(String),
    monitor_id         UUID,
    total_checks       AggregateFunction(count, UInt64),
    successful_checks  AggregateFunction(sum, UInt64),
    failed_checks      AggregateFunction(sum, UInt64),
    duration_quantiles AggregateFunction(quantilesTDigest(0.5, 0.95, 0.99), Float64)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(day)
ORDER BY (day, project_id, monitor_id)
TTL day + INTERVAL 365 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS faro.mv_monitor_uptime_daily TO faro.monitor_uptime_daily AS
SELECT
    toDate(timestamp) AS day,
    project_id,
    monitor_id,
    countState() AS total_checks,
    sumState(toUInt64(success)) AS successful_checks,
    sumState(toUInt64(if(success = 0, 1, 0))) AS failed_checks,
    quantilesTDigestState(0.5, 0.95, 0.99)(toFloat64(duration_ms)) AS duration_quantiles
FROM faro.monitor_results
GROUP BY day, project_id, monitor_id;

-- ----------- services_seen + stale events -----------

CREATE TABLE IF NOT EXISTS faro.services_seen
(
    project_id   LowCardinality(String),
    service_name LowCardinality(String),
    last_seen_at AggregateFunction(max, DateTime64(9, 'UTC'))
)
ENGINE = AggregatingMergeTree
ORDER BY (project_id, service_name);

CREATE MATERIALIZED VIEW IF NOT EXISTS faro.mv_services_seen_logs TO faro.services_seen AS
SELECT project_id, service_name, maxState(timestamp) AS last_seen_at
FROM faro.logs
GROUP BY project_id, service_name;

CREATE MATERIALIZED VIEW IF NOT EXISTS faro.mv_services_seen_spans TO faro.services_seen AS
SELECT project_id, service_name, maxState(timestamp) AS last_seen_at
FROM faro.spans
GROUP BY project_id, service_name;

CREATE MATERIALIZED VIEW IF NOT EXISTS faro.mv_services_seen_metrics TO faro.services_seen AS
SELECT project_id, service_name, maxState(timestamp) AS last_seen_at
FROM faro.metrics
GROUP BY project_id, service_name;

CREATE TABLE IF NOT EXISTS faro.service_stale_events
(
    timestamp     DateTime64(3, 'UTC')   CODEC(Delta, ZSTD(1)),
    project_id    LowCardinality(String),
    service_name  LowCardinality(String),
    event         LowCardinality(String),
    last_seen_at  DateTime64(3, 'UTC'),
    silence_hours Float64
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (project_id, service_name, timestamp)
TTL toDateTime(timestamp) + INTERVAL 365 DAY;

-- ----------- error_clusters (compactador MinHash) -----------

CREATE TABLE IF NOT EXISTS faro.error_clusters
(
    fingerprint            String,
    cluster_id             String,
    project_id             LowCardinality(String) DEFAULT 'default',
    service_name           LowCardinality(String),
    exception_type         LowCardinality(String),
    minhash                Array(UInt64)          CODEC(ZSTD(3)),
    representative_message String                 CODEC(ZSTD(3)),
    representative_stack   String                 CODEC(ZSTD(3)),
    member_count           UInt64                 DEFAULT 1,
    first_seen_at          DateTime64(3, 'UTC')   DEFAULT now64(3),
    last_seen_at           DateTime64(3, 'UTC')   DEFAULT now64(3),
    version                UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY (fingerprint);

-- ----------- notification_channels (canales configurables) -----------
-- Ver migration 009-notification-channels.sql para el rationale.
CREATE TABLE IF NOT EXISTS faro.notification_channels
(
    id           String,
    name         String                 DEFAULT '',
    kind         LowCardinality(String),
    enabled      UInt8                  DEFAULT 1,
    config       String                 DEFAULT '',
    created_at   DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_at   DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_by   String                 DEFAULT '',
    deleted      UInt8                  DEFAULT 0,
    version      UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY id;
