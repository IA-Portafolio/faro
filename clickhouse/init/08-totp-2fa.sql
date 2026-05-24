-- Esquema 2FA TOTP. Init paralelo a clickhouse/migrations/007-totp-2fa.sql para
-- deploys nuevos que no corran migrations.

ALTER TABLE faro.users
    ADD COLUMN IF NOT EXISTS totp_secret  String DEFAULT '',
    ADD COLUMN IF NOT EXISTS totp_enabled UInt8  DEFAULT 0;

CREATE TABLE IF NOT EXISTS faro.user_recovery_codes
(
    user_id    UUID,
    code_hash  String,
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    used_at    Nullable(DateTime64(3, 'UTC')),
    version    UInt64               DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY (user_id, code_hash);

CREATE TABLE IF NOT EXISTS faro.user_login_challenges
(
    token_hash  String,
    user_id     UUID,
    user_email  LowCardinality(String),
    user_name   String,
    user_role   LowCardinality(String),
    created_at  DateTime64(3, 'UTC') DEFAULT now64(3),
    expires_at  DateTime64(3, 'UTC'),
    consumed    UInt8                DEFAULT 0,
    version     UInt64               DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY token_hash
TTL toDateTime(expires_at) + INTERVAL 1 DAY;
