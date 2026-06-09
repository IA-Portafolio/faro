# Session Trace Linking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Link product sessions to backend traces by storing session trace ids and exposing a session traces API.

**Architecture:** `session_aggregator` materializes unique `trace_ids` into `product_sessions` so synthetic sessions can be linked even when raw events have empty `session_id`. A protected API endpoint reads those ids and summarizes matching spans from `faro.spans`.

**Tech Stack:** Rust, Axum, Tokio integration tests, ClickHouse SQL migrations/init schema.

---

### Task 1: Worker Test for Trace IDs

**Files:**

- Modify: `backend/tests/workers_session_aggregator.rs`

- [x] **Step 1: Extend session test output**

Add to `SessionOut`:

```rust
trace_ids: Vec<String>,
trace_count: u32,
```

Update the `sessions()` query to select `trace_ids, trace_count`.

- [x] **Step 2: Add trace ids to seeded events**

Add a helper:

```rust
fn event_with_trace(
    project_id: &str,
    seconds_ago: i64,
    event_name: &str,
    distinct_id: &str,
    anonymous_id: &str,
    session_id: &str,
    trace_id: &str,
) -> ProductEventRow
```

Make `event()` call `event_with_trace(..., "")`.

- [x] **Step 3: Add assertions**

In explicit SDK session test, seed two events with the same trace id and one with
empty trace id. Assert `trace_ids = ["trace-sdk-a"]` and `trace_count = 1`.

In synthetic session test, seed the first two events with different trace ids and
assert the first derived session has `trace_count = 2`; assert the second has
`trace_count = 0`.

- [x] **Step 4: Run failing test**

Run:

```bash
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc '/usr/local/cargo/bin/cargo test --test workers_session_aggregator -- --nocapture'
```

Expected: fail because `trace_ids`/`trace_count` do not exist yet.

### Task 2: Schema and Model

**Files:**

- Modify: `clickhouse/init/86-product-events-aux.sql`
- Create: `clickhouse/migrations/019-session-trace-linking.sql`
- Modify: `backend/src/storage/models.rs`

- [x] **Step 1: Add schema columns**

Add to `product_sessions`:

```sql
trace_ids        Array(String)          DEFAULT [] CODEC(ZSTD(1)),
trace_count      UInt32                 DEFAULT 0 CODEC(T64, ZSTD(1)),
```

- [x] **Step 2: Add migration**

Create:

```sql
-- Goal 10.F.3: link product sessions to backend traces.
ALTER TABLE faro.product_sessions
    ADD COLUMN IF NOT EXISTS trace_ids Array(String) DEFAULT [] CODEC(ZSTD(1));

ALTER TABLE faro.product_sessions
    ADD COLUMN IF NOT EXISTS trace_count UInt32 DEFAULT 0 CODEC(T64, ZSTD(1));
```

- [x] **Step 3: Add Rust fields**

Add to `ProductSessionRow`:

```rust
#[serde(default)]
pub trace_ids: Vec<String>,
#[serde(default)]
pub trace_count: u32,
```

### Task 3: Aggregator Trace Materialization

**Files:**

- Modify: `backend/src/workers/session_aggregator.rs`

- [x] **Step 1: Add row fields**

Add `trace_ids: Vec<String>` and `trace_count: u32` to `AggSessionRow`, and map
them into `ProductSessionRow`.

- [x] **Step 2: Carry trace id in SQL**

Ensure both session branches include `trace_id` in the event subqueries and
compute:

```sql
groupUniqArrayIf(trace_id, trace_id != '') AS trace_ids,
toUInt32(length(trace_ids)) AS trace_count
```

Include `trace_ids, trace_count` in the outer SELECT.

### Task 4: Session Traces API

**Files:**

- Create: `backend/src/api/sessions.rs`
- Modify: `backend/src/api/mod.rs`
- Create: `backend/tests/api_session_traces.rs`

- [x] **Step 1: Write endpoint test**

Create a test that inserts one `product_sessions` row with two trace ids and
three `spans` rows across those traces. Login, call:

```text
GET /api/v1/sessions/<session_id>/traces?project=<project>
```

Assert it returns two `TraceSummary` rows, grouped by trace id, with span counts
and ERROR status visible for the failing trace.

Also test missing session returns 404 and empty `trace_ids` returns `[]`.

- [x] **Step 2: Implement `sessions.rs`**

Add router:

```rust
Router::new().route("/sessions/:session_id/traces", get(session_traces))
```

Handler:

1. Require non-empty `project` query param.
2. Read `trace_ids` from `faro.product_sessions FINAL`.
3. Return 404 if absent.
4. Return empty vec if no trace ids.
5. Query `faro.spans` with an IN-list of bound params, grouped by `trace_id`,
   returning `TraceSummary`.

- [x] **Step 3: Mount router**

Add `pub mod sessions;` and `.merge(sessions::router())` in `backend/src/api/mod.rs`.

### Task 5: Verification

**Files:**

- No new source files.

- [x] **Step 1: Apply schema and run targeted tests**

Run:

```bash
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc '<apply init/migrations>; /usr/local/cargo/bin/cargo test --test workers_session_aggregator --test api_session_traces -- --nocapture'
```

Expected: all targeted tests pass.

- [x] **Step 2: Format touched Rust files**

Run:

```bash
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc '/usr/local/cargo/bin/rustup component add rustfmt >/dev/null && /usr/local/cargo/bin/rustfmt --edition 2021 --check src/workers/session_aggregator.rs src/storage/models.rs src/api/mod.rs src/api/sessions.rs tests/workers_session_aggregator.rs tests/api_session_traces.rs'
```

Expected: pass.

- [x] **Step 3: Report commit status**

Commit only if the touched implementation files are tracked cleanly. If key
files are untracked pre-existing workspace files, do not commit and report that.

### Review Notes

- `session_aggregator` materializes unique non-empty `trace_id` values for both
  SDK-provided and synthetic sessions.
- `GET /api/v1/sessions/:session_id/traces?project=<project>` returns 404 when
  the session row is absent, `[]` when `trace_ids` is empty, and `TraceSummary[]`
  grouped from `faro.spans` when traces exist.
- `GET /api/v1/sessions` also exposes `trace_count`, allowing the session list
  to show whether a session has backend traces without fetching the trace list.
- The frontend adds `sessionTracesHref`, `fetchProductSessionTraces`, links from
  `/sessions`, and `/sessions/:session_id/traces?project=<project>` as the
  navigable trace summary view.
- Worker coverage now asserts `trace_ids` and `trace_count`; endpoint coverage
  asserts resolved summaries, empty sessions, and unknown sessions.
