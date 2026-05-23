-- Métricas: mediciones numéricas. Una sola tabla con columna de tipo para mantener simple la ingesta.
CREATE TABLE IF NOT EXISTS faro.metrics
(
    timestamp           DateTime64(9, 'UTC')                CODEC(Delta, ZSTD(1)),
    project_id          LowCardinality(String)              DEFAULT 'default',
    metric_name         LowCardinality(String),
    metric_type         LowCardinality(String),
    metric_unit         LowCardinality(String),
    service_name        LowCardinality(String),
    value               Float64                             CODEC(Gorilla, ZSTD(1)),
    resource_attributes Map(LowCardinality(String), String) CODEC(ZSTD(1)),
    attributes          Map(LowCardinality(String), String) CODEC(ZSTD(1)),
    hist_count          UInt64    DEFAULT 0                 CODEC(T64, ZSTD(1)),
    hist_sum            Float64   DEFAULT 0                 CODEC(Gorilla, ZSTD(1)),
    hist_min            Float64   DEFAULT 0                 CODEC(Gorilla, ZSTD(1)),
    hist_max            Float64   DEFAULT 0                 CODEC(Gorilla, ZSTD(1)),
    hist_bucket_bounds  Array(Float64),
    hist_bucket_counts  Array(UInt64),
    INDEX idx_metric metric_name TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (service_name, metric_name, timestamp)
TTL toDateTime(timestamp) + INTERVAL 90 DAY
SETTINGS index_granularity = 8192;
