# Session Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish 10.F.1 by making session aggregation include anonymous-only product events while preserving SDK-provided session ids and 30-minute inactivity splitting.

**Architecture:** Keep the existing `session_aggregator` worker and `product_sessions` schema. Extract the aggregation SQL into a small helper, expose a one-shot aggregation function for integration tests, and change the SQL to derive an effective actor id from `distinct_id` or `anonymous_id`.

**Tech Stack:** Rust, Tokio, Axum test fixture, ClickHouse, `cargo test`.

---

## File Structure

- Modify `backend/src/workers/session_aggregator.rs`
  - Add a `session_aggregation_sql()` helper returning the ClickHouse query.
  - Make one-shot aggregation callable from integration tests.
  - Change both SQL branches to use `actor_id = if(distinct_id != '', distinct_id, anonymous_id)`.
- Modify `backend/tests/common/mod.rs`
  - Add the missing `Config` fields for user unifier and session aggregator test defaults.
- Create `backend/tests/workers_session_aggregator.rs`
  - Seed `faro.product_events` directly.
  - Call the one-shot aggregator.
  - Assert rows in `faro.product_sessions FINAL`.

## Task 1: Fix Backend Test Fixture Config

**Files:**

- Modify: `backend/tests/common/mod.rs`

- [ ] **Step 1: Run the current backend test compile**

Run:

```bash
cd backend
cargo test --test workers_session_aggregator --no-run
```

Expected: this may fail because the test file does not exist yet. If it compiles existing tests instead, continue. The important check comes after adding the new test file.

- [ ] **Step 2: Add missing test config fields**

In `backend/tests/common/mod.rs`, inside `test_config() -> Config`, after `stale_threshold_hours: 24,`, add:

```rust
        user_unifier_enabled: false,
        user_unifier_interval_secs: 60,
        session_aggregator_enabled: false,
        session_aggregator_interval_secs: 300,
        session_aggregator_gap_minutes: 30,
        session_aggregator_lookback_minutes: 360,
```

- [ ] **Step 3: Verify fixture compiles with existing tests**

Run:

```bash
cd backend
cargo test --test auth_session --no-run
```

Expected: compile succeeds. If it fails with a missing `Config` field, add that exact field to `test_config()` with the default from `backend/src/config.rs`.

- [ ] **Step 4: Commit**

```bash
git add backend/tests/common/mod.rs
git commit -m "test: refresh backend fixture config"
```

## Task 2: Add Failing Session Aggregator Integration Tests

**Files:**

- Create: `backend/tests/workers_session_aggregator.rs`

- [ ] **Step 1: Create the failing test file**

Create `backend/tests/workers_session_aggregator.rs` with:

```rust
mod common;

use chrono::{Duration, Utc};
use common::TestApp;
use faro::storage::ProductEventRow;
use faro::workers::session_aggregator::aggregate_once;
use serde::Deserialize;
use uuid::Uuid;

fn event(
    project_id: &str,
    seconds_ago: i64,
    event_name: &str,
    distinct_id: &str,
    anonymous_id: &str,
    session_id: &str,
) -> ProductEventRow {
    ProductEventRow {
        timestamp: Utc::now() - Duration::seconds(seconds_ago),
        project_id: project_id.to_string(),
        event_name: event_name.to_string(),
        distinct_id: distinct_id.to_string(),
        anonymous_id: anonymous_id.to_string(),
        session_id: session_id.to_string(),
        properties: "{}".into(),
        user_properties: "{}".into(),
        context: "{}".into(),
        source: "web".into(),
        trace_id: String::new(),
        span_id: String::new(),
        event_id: Uuid::new_v4().to_string(),
    }
}

#[derive(Debug, Deserialize)]
struct SessionOut {
    session_id: String,
    distinct_id: String,
    page_count: u32,
    duration_seconds: u32,
}

async fn sessions(app: &TestApp) -> Vec<SessionOut> {
    app.ch
        .select_with_params(
            "SELECT session_id, distinct_id, page_count, duration_seconds \
             FROM faro.product_sessions FINAL \
             WHERE project_id = {project:String} \
             ORDER BY distinct_id, started_at, session_id",
            &[("project", &app.project_slug)],
        )
        .await
        .expect("select sessions")
}

#[tokio::test]
async fn explicit_session_id_is_trusted() {
    let app = TestApp::spawn().await;
    let rows = vec![
        event(&app.project_slug, 3600, "page_view", "user-1", "", "sdk-session"),
        event(&app.project_slug, 60, "checkout", "user-1", "", "sdk-session"),
    ];
    app.ch.insert("faro.product_events", &rows).await.expect("insert events");

    let from = Utc::now() - Duration::hours(2);
    let written = aggregate_once(&app.state, from, 30).await.expect("aggregate");
    assert_eq!(written, 1);

    let out = sessions(&app).await;
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].session_id, "sdk-session");
    assert_eq!(out[0].distinct_id, "user-1");
    assert_eq!(out[0].page_count, 2);
    assert!(out[0].duration_seconds >= 3500);
}

#[tokio::test]
async fn synthetic_sessions_split_only_after_gap_exceeds_timeout() {
    let app = TestApp::spawn().await;
    let now = Utc::now();
    let rows = vec![
        ProductEventRow {
            timestamp: now - Duration::minutes(61),
            project_id: app.project_slug.clone(),
            event_name: "page_view".into(),
            distinct_id: "user-gap".into(),
            anonymous_id: String::new(),
            session_id: String::new(),
            properties: "{}".into(),
            user_properties: "{}".into(),
            context: "{}".into(),
            source: "web".into(),
            trace_id: String::new(),
            span_id: String::new(),
            event_id: Uuid::new_v4().to_string(),
        },
        ProductEventRow {
            timestamp: now - Duration::minutes(31),
            project_id: app.project_slug.clone(),
            event_name: "dashboard_opened".into(),
            distinct_id: "user-gap".into(),
            anonymous_id: String::new(),
            session_id: String::new(),
            properties: "{}".into(),
            user_properties: "{}".into(),
            context: "{}".into(),
            source: "web".into(),
            trace_id: String::new(),
            span_id: String::new(),
            event_id: Uuid::new_v4().to_string(),
        },
        ProductEventRow {
            timestamp: now,
            project_id: app.project_slug.clone(),
            event_name: "checkout".into(),
            distinct_id: "user-gap".into(),
            anonymous_id: String::new(),
            session_id: String::new(),
            properties: "{}".into(),
            user_properties: "{}".into(),
            context: "{}".into(),
            source: "web".into(),
            trace_id: String::new(),
            span_id: String::new(),
            event_id: Uuid::new_v4().to_string(),
        },
    ];
    app.ch.insert("faro.product_events", &rows).await.expect("insert events");

    let from = now - Duration::hours(2);
    let written = aggregate_once(&app.state, from, 30).await.expect("aggregate");
    assert_eq!(written, 2);

    let out = sessions(&app).await;
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].distinct_id, "user-gap");
    assert_eq!(out[0].page_count, 2);
    assert_eq!(out[1].distinct_id, "user-gap");
    assert_eq!(out[1].page_count, 1);
}

#[tokio::test]
async fn anonymous_only_events_are_sessionized_and_empty_actor_events_are_ignored() {
    let app = TestApp::spawn().await;
    let rows = vec![
        event(&app.project_slug, 600, "page_view", "", "anon-1", ""),
        event(&app.project_slug, 300, "dashboard_opened", "", "anon-1", ""),
        event(&app.project_slug, 120, "orphan", "", "", ""),
    ];
    app.ch.insert("faro.product_events", &rows).await.expect("insert events");

    let from = Utc::now() - Duration::hours(1);
    let written = aggregate_once(&app.state, from, 30).await.expect("aggregate");
    assert_eq!(written, 1);

    let out = sessions(&app).await;
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].distinct_id, "anon-1");
    assert_eq!(out[0].page_count, 2);
}
```

- [ ] **Step 2: Run the new test to verify it fails**

Run:

```bash
cd backend
cargo test --test workers_session_aggregator -- --nocapture
```

Expected: FAIL at compile time because `aggregate_once` is private. If it compiles, expect the anonymous-only test to fail because current SQL filters `distinct_id != ''`.

- [ ] **Step 3: Commit the failing test**

```bash
git add backend/tests/workers_session_aggregator.rs
git commit -m "test: cover product session aggregation"
```

## Task 3: Make One-Shot Aggregation Testable

**Files:**

- Modify: `backend/src/workers/session_aggregator.rs`

- [ ] **Step 1: Expose the aggregation helper intentionally**

Change:

```rust
async fn aggregate_once(
    state: &SharedState,
    from: DateTime<Utc>,
    gap_minutes: u32,
) -> anyhow::Result<usize> {
```

to:

```rust
/// Runs one session aggregation pass. Public for integration tests and
/// maintenance jobs; the spawned worker calls the same function on each tick.
pub async fn aggregate_once(
    state: &SharedState,
    from: DateTime<Utc>,
    gap_minutes: u32,
) -> anyhow::Result<usize> {
```

- [ ] **Step 2: Extract the SQL into a helper**

Replace the inline `let sql = "...";` binding in `aggregate_once` with:

```rust
    let sql = session_aggregation_sql();
```

Then add this function above `aggregate_once`:

```rust
fn session_aggregation_sql() -> &'static str {
    "
        SELECT project_id, session_id, distinct_id, started_at, ended_at,
               page_count, duration_seconds, source
        FROM (
            SELECT
                project_id,
                session_id,
                actor_id AS distinct_id,
                min(timestamp) AS started_at,
                max(timestamp) AS ended_at,
                toUInt32(count()) AS page_count,
                toUInt32(dateDiff('second', min(timestamp), max(timestamp))) AS duration_seconds,
                any(toString(source)) AS source
            FROM (
                SELECT
                    project_id,
                    if(distinct_id != '', distinct_id, anonymous_id) AS actor_id,
                    session_id,
                    timestamp,
                    source
                FROM faro.product_events
                WHERE timestamp >= toDateTime64({from:DateTime64(9)}, 9)
                  AND session_id != ''
                  AND (distinct_id != '' OR anonymous_id != '')
            )
            GROUP BY project_id, actor_id, session_id

            UNION ALL

            SELECT
                project_id,
                concat('s-', lower(hex(cityHash64(
                    toString(project_id),
                    actor_id,
                    toUInt64(toUnixTimestamp(min(timestamp)))
                )))) AS session_id,
                actor_id AS distinct_id,
                min(timestamp) AS started_at,
                max(timestamp) AS ended_at,
                toUInt32(count()) AS page_count,
                toUInt32(dateDiff('second', min(timestamp), max(timestamp))) AS duration_seconds,
                any(toString(source)) AS source
            FROM (
                SELECT
                    project_id,
                    actor_id,
                    timestamp,
                    source,
                    sum(if(prev_ts = toDateTime64(0, 9)
                           OR dateDiff('second', prev_ts, timestamp) > {gap_secs:UInt32}, 1, 0))
                        OVER (PARTITION BY project_id, actor_id ORDER BY timestamp
                              ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS sess_idx
                FROM (
                    SELECT
                        project_id,
                        actor_id,
                        timestamp,
                        source,
                        lagInFrame(timestamp, 1, toDateTime64(0, 9))
                            OVER (PARTITION BY project_id, actor_id ORDER BY timestamp) AS prev_ts
                    FROM (
                        SELECT
                            project_id,
                            if(distinct_id != '', distinct_id, anonymous_id) AS actor_id,
                            timestamp,
                            source
                        FROM faro.product_events
                        WHERE timestamp >= toDateTime64({from:DateTime64(9)}, 9)
                          AND session_id = ''
                          AND (distinct_id != '' OR anonymous_id != '')
                    )
                )
            )
            GROUP BY project_id, actor_id, sess_idx
        )
    "
}
```

- [ ] **Step 3: Run formatter**

Run:

```bash
cd backend
cargo fmt
```

Expected: succeeds.

- [ ] **Step 4: Run the new test**

Run:

```bash
cd backend
cargo test --test workers_session_aggregator -- --nocapture
```

Expected: tests pass. If ClickHouse rejects aliases inside window functions, wrap the innermost actor derivation in another subquery exactly as shown above so `actor_id` is a real selected column before the window clauses.

- [ ] **Step 5: Commit**

```bash
git add backend/src/workers/session_aggregator.rs
git commit -m "fix: sessionize anonymous product events"
```

## Task 4: Verify Focused Backend Suite

**Files:**

- No code changes expected.

- [ ] **Step 1: Run session aggregator tests**

Run:

```bash
cd backend
cargo test --test workers_session_aggregator -- --nocapture
```

Expected: all tests pass.

- [ ] **Step 2: Run related worker/unit tests**

Run:

```bash
cd backend
cargo test session_aggregator
```

Expected: all matching tests pass.

- [ ] **Step 3: Run backend compile check for all tests**

Run:

```bash
cd backend
cargo test --all-targets --no-run
```

Expected: all test targets compile.

- [ ] **Step 4: Commit any verification-only fixes**

Only if previous steps required small compile fixes:

```bash
git add backend
git commit -m "test: stabilize session aggregation coverage"
```

## Self-Review

- Spec coverage: the plan covers trusted SDK `session_id`, synthetic 30-minute timeout sessions, anonymous-only events, empty actor exclusion, idempotent table behavior via `ReplacingMergeTree`, and no UI/schema migration.
- Red-flag scan: no unresolved filler text or unspecified edge handling remains.
- Type consistency: tests use existing `ProductEventRow`, `TestApp`, `SharedState`, and the planned `pub async fn aggregate_once` signature.
