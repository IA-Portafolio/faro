-- Tablas auxiliares y materialized views del 6º pilar (product events).
-- Ver clickhouse/init/85-product-events.sql para la tabla principal.
--
-- Reparto de responsabilidades:
--   * product_users     -> poblada por un worker (lookup + upsert) para preservar
--                          first_seen correctamente. ReplacingMergeTree(last_seen)
--                          dedupea por (project_id, distinct_id) quedándose con la
--                          versión de mayor last_seen.
--   * product_sessions  -> poblada por un worker que sesionaliza eventos por
--                          inactividad (típico 30 min). ReplacingMergeTree(ended_at)
--                          permite extender la sesión en flight reinsertando.
--   * MVs *_per_day     -> rellenadas automáticamente desde product_events para
--                          que las cards del dashboard y los cohorts respondan en O(1).

-- ----------- product_users -----------
-- Una fila estable por (project_id, distinct_id). Permite "¿cuántos usuarios únicos
-- vimos esta semana?" sin escanear product_events entero.
--
-- `anonymous_ids` / `sources` / `event_count` materializan la unificación
-- multi-device (goal 10.E.1): un mismo usuario que entra anónimo en web
-- (anon-A) y luego en mobile (anon-B), tras dos `identify` queda con
-- `anonymous_ids = [anon-A, anon-B]` y `sources = [web, mobile]`. El worker
-- `user_unifier` agrega y re-inserta; ReplacingMergeTree(last_seen) se queda
-- con la fila más reciente.
CREATE TABLE IF NOT EXISTS faro.product_users
(
    project_id     LowCardinality(String) DEFAULT 'default',
    distinct_id    String                 CODEC(ZSTD(1)),
    first_seen     DateTime64(9, 'UTC')   CODEC(Delta, ZSTD(1)),
    last_seen      DateTime64(9, 'UTC')   CODEC(Delta, ZSTD(1)),
    anonymous_ids  Array(String)          DEFAULT [] CODEC(ZSTD(1)),
    sources        Array(LowCardinality(String)) DEFAULT [],
    event_count    UInt64                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    properties     String                 DEFAULT '' CODEC(ZSTD(3)),
    INDEX idx_distinct distinct_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(last_seen)
ORDER BY (project_id, distinct_id)
SETTINGS index_granularity = 8192;

-- ----------- product_user_aliases -----------
-- Lookup reverso anonymous_id → distinct_id. Permite reattribuir eventos
-- pre-login (que llegaron sólo con anon_id) al usuario estable una vez que
-- éste se identifica desde algún device.
--
-- ReplacingMergeTree(linked_at): si el mismo anonymous_id se asocia a dos
-- distinct_ids distintos (device compartido), el más reciente gana. El
-- histórico queda audit-able consultando sin FINAL.
CREATE TABLE IF NOT EXISTS faro.product_user_aliases
(
    project_id    LowCardinality(String) DEFAULT 'default',
    anonymous_id  String                 CODEC(ZSTD(1)),
    distinct_id   String                 CODEC(ZSTD(1)),
    linked_at     DateTime64(9, 'UTC')   CODEC(Delta, ZSTD(1)),
    INDEX idx_distinct distinct_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(linked_at)
ORDER BY (project_id, anonymous_id)
SETTINGS index_granularity = 8192;

-- ----------- product_sessions -----------
-- Pre-agregada por un worker que cierra sesiones tras N minutos de inactividad.
-- ReplacingMergeTree(ended_at): mientras la sesión sigue abierta, el worker
-- re-inserta con ended_at actualizado y la versión más reciente gana.
CREATE TABLE IF NOT EXISTS faro.product_sessions
(
    project_id       LowCardinality(String) DEFAULT 'default',
    session_id       String                 CODEC(ZSTD(1)),
    distinct_id      String                 CODEC(ZSTD(1)),
    started_at       DateTime64(9, 'UTC')   CODEC(Delta, ZSTD(1)),
    ended_at         DateTime64(9, 'UTC')   CODEC(Delta, ZSTD(1)),
    page_count       UInt32                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    duration_seconds UInt32                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    event_count      UInt32                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    pageview_count   UInt32                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    is_bounce        UInt8                  DEFAULT 0,
    is_engaged       UInt8                  DEFAULT 0,
    converted        UInt8                  DEFAULT 0,
    quality_score    Float32                DEFAULT 0 CODEC(ZSTD(1)),
    trace_ids        Array(String)          DEFAULT [] CODEC(ZSTD(1)),
    trace_count      UInt32                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    source           LowCardinality(String) DEFAULT 'web',
    INDEX idx_session session_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_distinct distinct_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(ended_at)
ORDER BY (project_id, session_id)
SETTINGS index_granularity = 8192;

-- ----------- mv_product_events_per_day -----------
-- Cards instantáneas: total de eventos por (proyecto, nombre, día).
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

-- ----------- mv_product_unique_users_per_day -----------
-- Cohorts viables: uniqExact (no aproximación HLL) sobre distinct_id por día/proyecto.
-- uniqExactState es mergeable: cohorts semanales/mensuales se calculan con uniqExactMerge.
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
