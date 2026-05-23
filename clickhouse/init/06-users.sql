-- Usuarios del dashboard + sesiones activas. Los tokens de ingesta (faro.projects) son independientes.
CREATE TABLE IF NOT EXISTS faro.users
(
    id             UUID,
    email          LowCardinality(String),
    password_hash  String,
    name           String                 DEFAULT '',
    role           LowCardinality(String) DEFAULT 'admin',
    created_at     DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_at     DateTime64(3, 'UTC')   DEFAULT now64(3),
    deleted        UInt8                  DEFAULT 0,
    version        UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY id;

CREATE TABLE IF NOT EXISTS faro.user_sessions
(
    token_hash     String,
    user_id        UUID,
    user_email     LowCardinality(String),
    user_name      String,
    user_role      LowCardinality(String),
    created_at     DateTime64(3, 'UTC')   DEFAULT now64(3),
    expires_at     DateTime64(3, 'UTC'),
    revoked        UInt8                  DEFAULT 0,
    version        UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY token_hash
TTL toDateTime(expires_at) + INTERVAL 30 DAY;
