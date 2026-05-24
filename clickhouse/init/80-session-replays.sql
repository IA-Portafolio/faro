-- Session replays: chunks de eventos rrweb enviados desde el SDK browser.
-- Cada fila es un *chunk* (~5s o ~50 eventos) de una sesión, no la sesión entera.
-- El player reconstruye la sesión leyendo las filas con el mismo session_id en orden de `seq`.
--
-- Volumen estimado: una sesión de 5 minutos con buffer de 5s genera ~60 chunks; el
-- snapshot inicial (1er chunk) pesa más, los incrementales son pequeños. ClickHouse
-- comprime `events` con ZSTD(3) — típicamente 5-10x sobre el JSON crudo.
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
    -- Array JSON con los rrweb events del chunk. Se almacena tal cual lo manda
    -- el cliente; la compresión la hace ClickHouse vía el codec de columna, así
    -- evitamos un round-trip de inflate/deflate aplicación↔storage.
    events        String                              CODEC(ZSTD(3)),
    user_id       String                              DEFAULT '',
    page_url      String                              DEFAULT '',
    user_agent    String                              DEFAULT '',
    INDEX idx_session session_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
-- Ordering por (session_id, seq) hace que un GET de la sesión completa lea
-- las filas contiguas — el caso de uso dominante en el player.
PARTITION BY toDate(timestamp)
ORDER BY (session_id, seq, timestamp)
TTL toDateTime(timestamp) + INTERVAL 7 DAY
SETTINGS index_granularity = 8192;
