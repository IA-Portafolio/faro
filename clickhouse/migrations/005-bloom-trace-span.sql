-- Migración: skip indexes (bloom_filter) sobre `trace_id` y `span_id` en las
-- tablas donde faltaban. Búsqueda exacta tipo `WHERE trace_id = 'abc...'` o
-- `WHERE span_id = 'def...'` sin bloom escanea todas las granules del part —
-- típicamente la partition completa (un día). Con bloom el planner descarta
-- ~99% de las granules antes de leer las columnas reales.
--
-- Idempotente: `ADD INDEX IF NOT EXISTS`. El índice se aplica automáticamente
-- a los parts nuevos. Para backfill sobre data histórica correr a mano:
--   ALTER TABLE faro.logs         MATERIALIZE INDEX idx_span;
--   ALTER TABLE faro.spans        MATERIALIZE INDEX idx_span_id;
--   ALTER TABLE faro.error_events MATERIALIZE INDEX idx_trace;
--   ALTER TABLE faro.error_events MATERIALIZE INDEX idx_span;
-- Es una mutación pesada (reescribe parts), conviene fuera de horas pico.
-- Sin MATERIALIZE igual gana porque el TTL rota la data: 30 d en logs/errors,
-- 14 d en spans → en pocas semanas el 100% de la data ya tiene el índice.

ALTER TABLE faro.logs         ADD INDEX IF NOT EXISTS idx_span  span_id  TYPE bloom_filter(0.01) GRANULARITY 4;
ALTER TABLE faro.spans        ADD INDEX IF NOT EXISTS idx_span_id span_id TYPE bloom_filter(0.01) GRANULARITY 4;
ALTER TABLE faro.error_events ADD INDEX IF NOT EXISTS idx_trace trace_id TYPE bloom_filter(0.01) GRANULARITY 4;
ALTER TABLE faro.error_events ADD INDEX IF NOT EXISTS idx_span  span_id  TYPE bloom_filter(0.01) GRANULARITY 4;
