-- Migración 008 — Workers de mantenimiento (2.8 del audit).
--
-- 1. Pre-agregaciones (3 MVs):
--    - errors_hourly: count de errores por hora/proyecto/servicio/severidad.
--    - spans_latency_hourly: count + quantilesTDigest p50/p95/p99 por hora/servicio/operación.
--    - monitor_uptime_daily: éxitos/fallos/quantiles de latencia por día/monitor.
--
-- 2. services_seen: AggregatingMergeTree de last_seen por (project, service) alimentada
--    por 3 MVs paralelas desde logs+spans+metrics. La consume el detector de stale.
--
-- 3. error_clusters: tabla ReplacingMergeTree donde el compactador de fingerprints mantiene
--    el mapping fingerprint→cluster_id con la signature MinHash del representante.
--
-- 4. service_stale_events: log audit de transiciones active→stale (útil para alertas / UI).
--
-- Todas las tablas son IF NOT EXISTS / idempotentes — esta migración se puede correr en
-- cualquier instalación existente o nueva.

-- ===========================================================================
-- 1.a errors_hourly  (count de errores por hora)
-- ===========================================================================
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
-- severity_number >= 17 = ERROR/FATAL (mismo umbral que error_indexer.rs)
WHERE severity_number >= 17
GROUP BY hour, project_id, service_name, severity_text;

-- ===========================================================================
-- 1.b spans_latency_hourly  (p50/p95/p99 por servicio/operación/hora)
-- ===========================================================================
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
    -- Pasamos a milisegundos para que los dashboards lo consuman directo.
    quantilesTDigestState(0.5, 0.95, 0.99)(toFloat64(duration_ns) / 1000000.0) AS duration_quantiles
FROM faro.spans
GROUP BY hour, project_id, service_name, span_name;

-- ===========================================================================
-- 1.c monitor_uptime_daily  (uptime/quantiles por monitor/día)
-- ===========================================================================
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
    -- `1 - success` con CAST evita underflow cuando success ya es UInt8 = 0/1
    sumState(toUInt64(if(success = 0, 1, 0))) AS failed_checks,
    quantilesTDigestState(0.5, 0.95, 0.99)(toFloat64(duration_ms)) AS duration_quantiles
FROM faro.monitor_results
GROUP BY day, project_id, monitor_id;

-- ===========================================================================
-- 2. services_seen  (alimenta al detector de stale)
-- ===========================================================================
CREATE TABLE IF NOT EXISTS faro.services_seen
(
    project_id   LowCardinality(String),
    service_name LowCardinality(String),
    last_seen_at AggregateFunction(max, DateTime64(9, 'UTC'))
)
ENGINE = AggregatingMergeTree
ORDER BY (project_id, service_name);

-- Tres MVs paralelas — una por tabla de origen. Cada una agrega su last_seen y el
-- merge engine los une en background, así que el SELECT final con `maxMerge(...)`
-- devuelve el máximo real cruzando logs+spans+metrics.
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

-- ===========================================================================
-- 3. error_clusters  (mapping fingerprint → cluster_id + MinHash del representante)
-- ===========================================================================
--
-- El compactador de fingerprints lee `error_events`, calcula MinHash de cada fingerprint
-- nuevo, y lo asigna a un cluster existente (Jaccard >= umbral) o crea uno nuevo.
--
-- Convención: los REPRESENTANTES de un cluster tienen `fingerprint = cluster_id`. Los
-- demás miembros tienen `cluster_id != fingerprint`. Esto evita una tabla aparte de
-- representantes y permite un SELECT trivial para listar clusters compatibles:
--
--   SELECT fingerprint, minhash, representative_message
--   FROM faro.error_clusters FINAL
--   WHERE project_id = ? AND service_name = ? AND exception_type = ?
--     AND fingerprint = cluster_id
--     AND last_seen_at > now() - INTERVAL 30 DAY
--
-- ReplacingMergeTree por (fingerprint) con version monotónica para que cada update
-- (last_seen_at, member_count) deje sólo la fila más nueva tras merge + FINAL.
CREATE TABLE IF NOT EXISTS faro.error_clusters
(
    fingerprint            String,
    cluster_id             String,
    project_id             LowCardinality(String) DEFAULT 'default',
    service_name           LowCardinality(String),
    exception_type         LowCardinality(String),
    -- MinHash signature. K=128 enteros sin signo, comparables posicionalmente:
    -- similitud Jaccard estimada = (# posiciones iguales) / K.
    minhash                Array(UInt64)          CODEC(ZSTD(3)),
    -- Sólo se rellena para representantes (fingerprint == cluster_id). Para members va vacío.
    representative_message String                 CODEC(ZSTD(3)),
    representative_stack   String                 CODEC(ZSTD(3)),
    member_count           UInt64                 DEFAULT 1,
    first_seen_at          DateTime64(3, 'UTC')   DEFAULT now64(3),
    last_seen_at           DateTime64(3, 'UTC')   DEFAULT now64(3),
    version                UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY (fingerprint);

-- ===========================================================================
-- 4. service_stale_events  (audit de transiciones active → stale)
-- ===========================================================================
--
-- Cada vez que el detector de stale ve un servicio que estaba activo y ahora lleva
-- >stale_threshold sin tráfico, escribe acá. Útil para:
--   - mostrar timeline "este servicio dejó de reportar el X"
--   - reglas de alerta tipo "servicio Y crítico se fue stale"
--   - distinguir un servicio que nunca existió de uno que dejó de reportar
--
-- No es ReplacingMergeTree porque queremos TODOS los eventos (un servicio puede
-- ir stale → active → stale → active a lo largo del tiempo).
CREATE TABLE IF NOT EXISTS faro.service_stale_events
(
    timestamp     DateTime64(3, 'UTC')   CODEC(Delta, ZSTD(1)),
    project_id    LowCardinality(String),
    service_name  LowCardinality(String),
    event         LowCardinality(String),  -- 'stale' | 'recovered'
    last_seen_at  DateTime64(3, 'UTC'),
    silence_hours Float64
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (project_id, service_name, timestamp)
TTL toDateTime(timestamp) + INTERVAL 365 DAY;
