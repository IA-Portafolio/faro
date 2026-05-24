-- 2FA TOTP (RFC 6238) opcional por usuario:
--   * `totp_secret`  : secreto crudo (Base32) cifrado-en-reposo NO se aplica todavía;
--                      como mitigación, el campo se mantiene vacío salvo cuando 2FA está
--                      enabled. Si se compromete la DB el atacante igual necesita el
--                      password para login, y los recovery_codes están hasheados.
--   * `totp_enabled` : 0 / 1.
-- Idempotente — ClickHouse acepta `ADD COLUMN IF NOT EXISTS` desde 22.x.
ALTER TABLE faro.users
    ADD COLUMN IF NOT EXISTS totp_secret  String DEFAULT '',
    ADD COLUMN IF NOT EXISTS totp_enabled UInt8  DEFAULT 0;

-- Recovery codes — 10 códigos one-shot generados al habilitar 2FA. Sólo se guardan
-- SHA-256 hashes; el plaintext se muestra al user una sola vez. Cada uso consume
-- una fila (marca `used_at`), nunca se reactiva.
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

-- Login challenges — token de un solo uso que el backend emite tras pasar
-- email+password cuando 2FA está habilitado, y que el cliente devuelve junto al
-- código TOTP para completar el login. Vida corta (5 min). Sólo se guarda el SHA-256
-- del token; el plaintext sólo existe en la respuesta HTTP.
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
