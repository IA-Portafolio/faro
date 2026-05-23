-- Proyectos: agrupación lógica. Cada proyecto tiene su propio token de ingesta; la
-- ingesta se rechaza si el token no coincide con un proyecto activo.
CREATE TABLE IF NOT EXISTS faro.projects
(
    id             UUID,
    slug           LowCardinality(String),
    name           String,
    description    String                 DEFAULT '',
    ingest_token   String,
    created_at     DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_at     DateTime64(3, 'UTC')   DEFAULT now64(3),
    deleted        UInt8                  DEFAULT 0,
    version        UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY id;
