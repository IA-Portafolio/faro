-- Cohorts: definiciones de segmentación de usuarios persistidas.
--
-- Diseño:
--   * `definition` es un JSON serializado con la regla declarativa
--     (event_name, comparador, count, last_days, filtros sobre properties).
--     Se guarda como String JSON en vez de columnas tipadas porque el set
--     de tipos de regla irá creciendo (sequence, frequency, recency…) y
--     normalizar a columnas implicaría una migración por cada nueva forma.
--   * El cohort se EVALÚA al vuelo contra `faro.product_events` cada vez
--     que la UI lo pide — no materializamos los miembros porque el cohort
--     decae con cada nuevo evento ingerido y mantener una membership table
--     fresca exige cron+invalidación. Para los goals D.x del backlog se
--     podrá añadir una snapshot opcional.
--   * Soft-delete + ReplacingMergeTree(version) — mismo patrón que
--     alert_rules / monitors / channels; el dashboard hace `FINAL`.
CREATE TABLE IF NOT EXISTS faro.cohorts
(
    id            UUID,
    project_id    LowCardinality(String) DEFAULT 'default',
    name          String,
    description   String                 DEFAULT '',
    -- JSON con la regla. Esquema actual:
    --   { "event": String,
    --     "op": "==" | ">=" | ">" | "<=" | "<",
    --     "count": UInt32,
    --     "last_days": UInt32,
    --     "filters": [ { "key": String, "value": String }, ... ] }
    definition    String,
    created_at    DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_at    DateTime64(3, 'UTC')   DEFAULT now64(3),
    created_by    String                 DEFAULT '',
    deleted       UInt8                  DEFAULT 0,
    version       UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY id;
