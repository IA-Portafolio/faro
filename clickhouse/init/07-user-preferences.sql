-- Preferencias por usuario (tema UI, defaults de exploración). Una fila por usuario.
-- ReplacingMergeTree para permitir actualizaciones idempotentes con `version`.
-- `default_project` y `default_time_range` se aplican al hidratar la sesión si la
-- URL no trae un override (?project=… / ?range=…), de modo que un deep link
-- siempre gana frente al default del usuario.
CREATE TABLE IF NOT EXISTS faro.user_preferences
(
    user_id              UUID,
    theme                LowCardinality(String) DEFAULT 'system',  -- 'light' | 'dark' | 'system'
    default_project      String                 DEFAULT '',         -- slug o '' para "todos"
    default_time_range   LowCardinality(String) DEFAULT '1h',       -- '5m'|'15m'|'1h'|'6h'|'24h'|'7d'
    updated_at           DateTime64(3, 'UTC')   DEFAULT now64(3),
    version              UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY user_id;
