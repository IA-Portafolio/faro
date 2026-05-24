# Session-Level Properties Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add session-level event counts, pageview counts, bounce/engaged flags, conversion flag, and quality score to `product_sessions`.

**Architecture:** Extend the ClickHouse session table additively and keep `session_aggregator` as the single writer of derived session properties. Keep `page_count` for compatibility, but write it as `pageview_count` going forward.

**Tech Stack:** Rust, Tokio integration tests, ClickHouse SQL migrations/init schema, Docker Compose integration test stack.

---

### Task 1: Write Failing Session Property Test

**Files:**
- Modify: `backend/tests/workers_session_aggregator.rs`

- [ ] **Step 1: Extend test row shape**

Update `SessionOut` to include:

```rust
event_count: u32,
pageview_count: u32,
is_bounce: u8,
is_engaged: u8,
converted: u8,
quality_score: f32,
```

Update the `sessions()` query to select those columns from `product_sessions FINAL`.

- [ ] **Step 2: Add assertions to existing tests**

In `explicit_session_id_is_trusted`, assert:

```rust
assert_eq!(out[0].event_count, 2);
assert_eq!(out[0].pageview_count, 1);
assert_eq!(out[0].page_count, 1);
assert_eq!(out[0].is_bounce, 0);
assert_eq!(out[0].is_engaged, 1);
assert_eq!(out[0].converted, 0);
assert!(out[0].quality_score > 35.0);
assert!(out[0].quality_score <= 70.0);
```

In the synthetic split test, assert the first session has `event_count = 2`,
`pageview_count = 1`, `is_engaged = 1`, `is_bounce = 0`; assert the second has
`event_count = 1`, `pageview_count = 0`, `is_bounce = 1`, `is_engaged = 0`.

- [ ] **Step 3: Add conversion quality test**

Add a test that inserts one `$pageview`, one custom event, and one
`checkout_completed` event in the same explicit SDK session. Assert:

```rust
assert_eq!(out[0].event_count, 3);
assert_eq!(out[0].pageview_count, 1);
assert_eq!(out[0].converted, 1);
assert_eq!(out[0].is_bounce, 0);
assert_eq!(out[0].is_engaged, 1);
assert!(out[0].quality_score >= 40.0);
assert!(out[0].quality_score <= 100.0);
```

- [ ] **Step 4: Run failing test**

Run:

```bash
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc '/usr/local/cargo/bin/cargo test --test workers_session_aggregator -- --nocapture'
```

Expected: fail because the new columns are not in `ProductSessionRow`/ClickHouse yet.

### Task 2: Extend ClickHouse Schema

**Files:**
- Modify: `clickhouse/init/86-product-events-aux.sql`
- Create: `clickhouse/migrations/018-session-level-properties.sql`

- [ ] **Step 1: Add fresh-install columns**

Add these columns to `product_sessions` after `duration_seconds`:

```sql
    event_count      UInt32                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    pageview_count   UInt32                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    is_bounce        UInt8                  DEFAULT 0,
    is_engaged       UInt8                  DEFAULT 0,
    converted        UInt8                  DEFAULT 0,
    quality_score    Float32                DEFAULT 0 CODEC(ZSTD(1)),
```

- [ ] **Step 2: Add additive migration**

Create `clickhouse/migrations/018-session-level-properties.sql` with:

```sql
-- Goal 10.F.2: session-level properties.
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
```

### Task 3: Extend Rust Model and Aggregator

**Files:**
- Modify: `backend/src/storage/models.rs`
- Modify: `backend/src/workers/session_aggregator.rs`

- [ ] **Step 1: Add model fields**

Add to `ProductSessionRow` after `duration_seconds`:

```rust
#[serde(default)]
pub event_count: u32,
#[serde(default)]
pub pageview_count: u32,
#[serde(default)]
pub is_bounce: u8,
#[serde(default)]
pub is_engaged: u8,
#[serde(default)]
pub converted: u8,
#[serde(default)]
pub quality_score: f32,
```

- [ ] **Step 2: Add aggregator fields**

Add matching fields to `AggSessionRow` and map them into `ProductSessionRow`.

- [ ] **Step 3: Update aggregation SQL**

For both grouped branches, compute:

```sql
toUInt32(count()) AS event_count,
toUInt32(countIf(event_name IN ('$pageview', 'page_view', '$screen', 'screen_view'))) AS pageview_count,
pageview_count AS page_count,
toUInt8(event_count <= 1) AS is_bounce,
toUInt8(event_count > 1) AS is_engaged,
toUInt8(countIf(event_name IN ('$conversion', 'checkout_completed', 'purchase', 'signup_completed', 'trial_started')) > 0) AS converted,
toFloat32(
    least(event_count / 10.0, 1.0) * 35.0
    + least(dateDiff('second', min(timestamp), max(timestamp)) / 300.0, 1.0) * 35.0
    + if(converted = 1, 30.0, 0.0)
) AS quality_score
```

Ensure subqueries carry `event_name` through so count predicates work.

### Task 4: Verify

**Files:**
- No new source files.

- [ ] **Step 1: Apply schema and run targeted test**

Run the same bootstrap pattern used for worker tests:

```bash
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc '<apply init/migrations>; /usr/local/cargo/bin/cargo test --test workers_session_aggregator -- --nocapture'
```

Expected: 4 tests pass.

- [ ] **Step 2: Format touched Rust files**

Run:

```bash
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc '/usr/local/cargo/bin/rustup component add rustfmt >/dev/null && /usr/local/cargo/bin/rustfmt --edition 2021 --check src/workers/session_aggregator.rs tests/workers_session_aggregator.rs src/storage/models.rs'
```

Expected: pass.

- [ ] **Step 3: Report commit status**

If touched files are tracked cleanly, commit:

```bash
git add backend/src/workers/session_aggregator.rs backend/src/storage/models.rs backend/tests/workers_session_aggregator.rs clickhouse/init/86-product-events-aux.sql clickhouse/migrations/018-session-level-properties.sql docs/superpowers/plans/2026-05-24-session-level-properties.md
git commit -m "feat: add session-level properties"
```

If files are untracked pre-existing workspace files, do not commit; report that clearly.

### Review Notes

- `product_sessions` now has additive columns for event/pageview counts,
  bounce/engaged flags, conversion, and bounded `quality_score`.
- `session_aggregator` writes `page_count = pageview_count` for compatibility
  while exposing the explicit `event_count` and `pageview_count` fields.
- `GET /api/v1/sessions` returns the session-level properties so callers do not
  need to scan raw `product_events`.
- `/sessions` renders aggregate engaged/bounce/quality cards plus per-session
  quality score and type labels.
- `backend/tests/workers_session_aggregator.rs` asserts counts,
  bounce/engaged behavior, conversion detection, and quality score bounds. The
  test fixture adds missing additive columns before running so older local
  ClickHouse schemas still exercise the new behavior.
