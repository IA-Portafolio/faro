-- Tabla de session replays para almacenar eventos rrweb del SDK browser.
-- Ver clickhouse/init/80-session-replays.sql para la definición canónica.
CREATE TABLE IF NOT EXISTS faro.session_replays
(
    timestamp     DateTime64(3, 'UTC')                CODEC(Delta, ZSTD(1)),
    project_id    LowCardinality(String)              DEFAULT 'default',
    session_id    String                              CODEC(ZSTD(1)),
    service_name  LowCardinality(String),
    seq           UInt32                              CODEC(T64, ZSTD(1)),
    start_ts      DateTime64(3, 'UTC')                CODEC(Delta, ZSTD(1)),
    end_ts        DateTime64(3, 'UTC')                CODEC(Delta, ZSTD(1)),
    event_count   UInt32                              CODEC(T64, ZSTD(1)),
    events        String                              CODEC(ZSTD(3)),
    user_id       String                              DEFAULT '',
    page_url      String                              DEFAULT '',
    user_agent    String                              DEFAULT '',
    INDEX idx_session session_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (session_id, seq, timestamp)
TTL toDateTime(timestamp) + INTERVAL 7 DAY
SETTINGS index_granularity = 8192;
