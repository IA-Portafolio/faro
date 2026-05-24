-- Migración idempotente: tabla de preferencias por usuario.
-- El backend persiste aquí el tema preferido y, en el futuro, cualquier
-- otra preferencia de UI por usuario.
CREATE TABLE IF NOT EXISTS faro.user_preferences
(
    user_id      UUID,
    theme        LowCardinality(String) DEFAULT 'system',  -- 'light' | 'dark' | 'system'
    updated_at   DateTime64(3, 'UTC')   DEFAULT now64(3),
    version      UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY user_id;
