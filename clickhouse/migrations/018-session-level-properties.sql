-- Goal 10.F.2: session-level properties.
-- Additive columns so existing installations keep reading old session rows.
ALTER TABLE faro.product_sessions
    ADD COLUMN IF NOT EXISTS event_count UInt32 DEFAULT 0 CODEC(T64, ZSTD(1));

ALTER TABLE faro.product_sessions
    ADD COLUMN IF NOT EXISTS pageview_count UInt32 DEFAULT 0 CODEC(T64, ZSTD(1));

ALTER TABLE faro.product_sessions
    ADD COLUMN IF NOT EXISTS is_bounce UInt8 DEFAULT 0;

ALTER TABLE faro.product_sessions
    ADD COLUMN IF NOT EXISTS is_engaged UInt8 DEFAULT 0;

ALTER TABLE faro.product_sessions
    ADD COLUMN IF NOT EXISTS converted UInt8 DEFAULT 0;

ALTER TABLE faro.product_sessions
    ADD COLUMN IF NOT EXISTS quality_score Float32 DEFAULT 0 CODEC(ZSTD(1));
