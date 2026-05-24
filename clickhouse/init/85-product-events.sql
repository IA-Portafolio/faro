-- Product events: 6º pilar de Faro. Lo que Mixpanel/Amplitude/PostHog hacen pero
-- unificado con el resto de observabilidad. La diferencia clave es trace_id/span_id:
-- linkeás un evento de producto (checkout_completed) con el span del backend que lo procesó.
--
-- Decisiones explícitas:
--  * distinct_id + anonymous_id separados: necesario para fusionar sesiones pre/post-login
--    (patrón PostHog). El anonymous_id viene de cookie/device antes del login; tras el login
--    se asocia al distinct_id estable.
--  * properties / user_properties / context como String JSON, NO Map(String, String):
--    cardinalidad infinita en una Map mata ClickHouse. Se consulta con JSONExtractString().
--  * trace_id / span_id opcionales (DEFAULT ''): diferencial único de Faro respecto a
--    competidores. Vacíos cuando el evento es 100% client-side sin contexto de servidor.
--  * TTL 365 días (vs 30d de logs): análisis de cohortes requiere histórico largo.
--  * PROJECTION by_event: el ORDER BY primario es (project_id, timestamp, event_name) para
--    rangos temporales por proyecto; la projection optimiza "evento X a lo largo del tiempo".
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
