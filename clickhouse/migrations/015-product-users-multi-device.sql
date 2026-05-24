-- Goal 10.E.1: unificar el usuario entre devices.
--
-- Hasta acá `product_users` sólo conocía `(project, distinct_id)` con
-- first_seen/last_seen. Para responder "todos los events de user_42 en
-- cualquier device" hace falta:
--
--   1) Saber qué `anonymous_id`s se fusionaron en cada `distinct_id`
--      (el SDK web manda anon-A, luego identify→user_42; el SDK mobile
--      manda anon-B, luego identify→user_42 — los dos anon ids son el
--      mismo usuario).
--   2) Saber por qué `source`s (web/mobile/backend/...) hemos visto al
--      usuario, para el split por device en el dashboard.
--   3) Lookup reverso `anonymous_id → distinct_id` para reattribuir
--      eventos pre-login que llegaron antes del identify.
--
-- Implementación:
--
-- * Extendemos `product_users` con `anonymous_ids`/`sources`/`event_count`.
--   El worker `user_unifier` re-inserta el row con la unión de anon_ids
--   y `last_seen` actualizado; `ReplacingMergeTree(last_seen)` se queda
--   con la versión más nueva (rationale igual que 014-product-aux-tables).
--   Las columnas nuevas tienen DEFAULT vacío para que el worker pueda
--   arrancar contra filas pre-existentes sin necesidad de migrar datos
--   (la primera vez que toque cada user, completa los campos).
--
-- * Nueva tabla `product_user_aliases` para el lookup reverso. PK por
--   `(project_id, anonymous_id)` — una versión por anon. Si el mismo
--   anon termina identificado contra dos distinct_ids distintos (caso
--   típico: compartir un device), `linked_at` decide cuál gana y el
--   resto del histórico queda audit-able si se consulta sin FINAL.

ALTER TABLE faro.product_users
    ADD COLUMN IF NOT EXISTS anonymous_ids Array(String) DEFAULT [];

ALTER TABLE faro.product_users
    ADD COLUMN IF NOT EXISTS sources Array(LowCardinality(String)) DEFAULT [];

ALTER TABLE faro.product_users
    ADD COLUMN IF NOT EXISTS event_count UInt64 DEFAULT 0;

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
