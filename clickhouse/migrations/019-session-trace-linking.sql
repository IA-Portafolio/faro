-- Goal 10.F.3: link product sessions to backend traces.
ALTER TABLE faro.product_sessions
    ADD COLUMN IF NOT EXISTS trace_ids Array(String) DEFAULT [] CODEC(ZSTD(1));

ALTER TABLE faro.product_sessions
    ADD COLUMN IF NOT EXISTS trace_count UInt32 DEFAULT 0 CODEC(T64, ZSTD(1));
