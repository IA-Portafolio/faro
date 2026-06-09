# Latency Funnel Impact Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `GET /api/v1/insights/latency-funnel-impact`, correlating slow backend span p95 buckets with checkout funnel conversion drop.

**Architecture:** Extend the existing `backend/src/api/insights.rs` module created for revenue-impact. Add typed query/response structs, two ClickHouse queries (span p95 by bucket and funnel counts by bucket), merge buckets in Rust, and compute aggregate slow-vs-baseline conversion metrics. Cover math with unit tests and the route with a ClickHouse-backed integration test.

**Tech Stack:** Rust 2021, Axum 0.7, serde, chrono, ClickHouse HTTP JSONEachRow through `storage::Client`, existing `backend/tests/common` integration harness.

---

## File Structure

- Modify `backend/src/api/insights.rs`: add `/insights/latency-funnel-impact`, request/response structs, helper math, SQL, and unit tests.
- Create `backend/tests/latency_funnel_impact.rs`: seed spans/product events and verify route output.
- No changes to ClickHouse schema.
- No changes to `backend/src/api/mod.rs` are needed if 10.J.1 has already registered `insights::router()`.

## Task 1: Add Failing Unit Tests For Latency Funnel Helpers

**Files:**

- Modify: `backend/src/api/insights.rs`

- [ ] **Step 1: Add helper tests to the existing `#[cfg(test)] mod tests`**

Append these tests inside the existing test module in `backend/src/api/insights.rs`:

```rust
#[test]
fn funnel_conversion_rate_counts_completed_over_started() {
    let rate = funnel_conversion_rate(10, 7);

    assert!((rate - 0.7).abs() < f64::EPSILON);
}

#[test]
fn funnel_conversion_rate_is_zero_when_no_one_started() {
    assert_eq!(funnel_conversion_rate(0, 7), 0.0);
}

#[test]
fn conversion_drop_points_clamps_negative_drop() {
    assert_eq!(conversion_drop_points(0.50, 0.75), 0.0);
}

#[test]
fn conversion_drop_points_returns_percentage_points() {
    let points = conversion_drop_points(0.71, 0.59);

    assert!((points - 12.0).abs() < 0.0001);
}

#[test]
fn latency_funnel_summary_formats_threshold_and_drop() {
    let text = latency_funnel_summary("/api/checkout", 2_000, 12.0);

    assert_eq!(
        text,
        "Cuando /api/checkout p95 supera 2s, el funnel checkout cae 12 puntos."
    );
}
```

- [ ] **Step 2: Run the unit test command and verify RED**

Run in Docker because local `cargo` is not available in this workspace:

```powershell
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc "/usr/local/cargo/bin/cargo test api::insights::tests"
```

Expected: compile failure mentioning missing functions:

```text
cannot find function `funnel_conversion_rate`
cannot find function `conversion_drop_points`
cannot find function `latency_funnel_summary`
```

If unrelated existing unit-test compile errors appear first (currently `src/ingest/events.rs` has broken tests in this workspace), record that blocker and continue to the focused integration test later.

## Task 2: Implement Latency Funnel Helpers

**Files:**

- Modify: `backend/src/api/insights.rs`

- [ ] **Step 1: Add helper functions above the test module**

Add this code near the existing revenue helper functions:

```rust
fn funnel_conversion_rate(started: u64, completed: u64) -> f64 {
    if started == 0 {
        return 0.0;
    }
    completed as f64 / started as f64
}

fn conversion_drop_points(baseline_conversion_rate: f64, slow_conversion_rate: f64) -> f64 {
    (baseline_conversion_rate - slow_conversion_rate).max(0.0) * 100.0
}

fn latency_funnel_summary(span_name: &str, latency_threshold_ms: u32, drop_points: f64) -> String {
    let threshold = if latency_threshold_ms % 1_000 == 0 {
        format!("{}s", latency_threshold_ms / 1_000)
    } else {
        format!("{:.1}s", latency_threshold_ms as f64 / 1_000.0)
    };
    format!(
        "Cuando {span_name} p95 supera {threshold}, el funnel checkout cae {:.0} puntos.",
        drop_points
    )
}
```

- [ ] **Step 2: Run focused formatting and compile check**

Run:

```powershell
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc "/usr/local/cargo/bin/rustup component add rustfmt >/tmp/rustfmt-install.log && /usr/local/cargo/bin/rustfmt --edition 2021 --check src/api/insights.rs && /usr/local/cargo/bin/cargo check"
```

Expected: `cargo check` finishes successfully. Existing warnings in unrelated files are acceptable.

## Task 3: Add Failing Integration Test For Latency Funnel Impact

**Files:**

- Create: `backend/tests/latency_funnel_impact.rs`

- [ ] **Step 1: Create the route-level integration test**

Create `backend/tests/latency_funnel_impact.rs`:

```rust
mod common;

use chrono::{DateTime, Duration, TimeZone, Utc};
use common::TestApp;
use faro::storage::{AttrMap, ProductEventRow, SpanRow};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct LatencyFunnelImpactResult {
    span_name: String,
    service_name: String,
    funnel_from: String,
    funnel_to: String,
    bucket_minutes: u32,
    p95_threshold_ms: u32,
    slow_bucket_count: u32,
    baseline_bucket_count: u32,
    baseline_conversion_rate: f64,
    slow_conversion_rate: f64,
    conversion_drop_points: f64,
    summary: String,
    buckets: Vec<LatencyFunnelBucket>,
}

#[derive(Debug, Deserialize)]
struct LatencyFunnelBucket {
    bucket_start: String,
    p95_latency_ms: f64,
    funnel_started: u64,
    funnel_completed: u64,
    conversion_rate: f64,
    slow: bool,
}

fn span_at(ts: DateTime<Utc>, project: &str, duration_ms: u64) -> SpanRow {
    SpanRow {
        timestamp: ts,
        project_id: project.into(),
        trace_id: Uuid::new_v4().simple().to_string(),
        span_id: Uuid::new_v4().simple().to_string(),
        parent_span_id: String::new(),
        trace_state: String::new(),
        name: "/api/checkout".into(),
        kind: "SERVER".into(),
        service_name: "checkout-api".into(),
        duration_ns: duration_ms * 1_000_000,
        status_code: "OK".into(),
        status_message: String::new(),
        resource_attributes: AttrMap::new(),
        span_attributes: AttrMap::new(),
        events_timestamps: Vec::new(),
        events_names: Vec::new(),
        events_attributes: Vec::new(),
        links_trace_ids: Vec::new(),
        links_span_ids: Vec::new(),
    }
}

fn product_event(
    ts: DateTime<Utc>,
    project: &str,
    distinct_id: &str,
    session_id: &str,
    event_name: &str,
) -> ProductEventRow {
    ProductEventRow {
        timestamp: ts,
        project_id: project.into(),
        event_name: event_name.into(),
        distinct_id: distinct_id.into(),
        anonymous_id: String::new(),
        session_id: session_id.into(),
        properties: String::new(),
        user_properties: String::new(),
        context: String::new(),
        source: "web".into(),
        trace_id: String::new(),
        span_id: String::new(),
        event_id: Uuid::new_v4().to_string(),
    }
}

fn add_funnel_events(
    rows: &mut Vec<ProductEventRow>,
    bucket: DateTime<Utc>,
    project: &str,
    prefix: &str,
    started: u32,
    completed: u32,
) {
    for i in 0..started {
        let distinct_id = format!("{prefix}-u-{i}");
        let session_id = format!("{prefix}-s-{i}");
        rows.push(product_event(
            bucket + Duration::minutes(5),
            project,
            &distinct_id,
            &session_id,
            "checkout_started",
        ));
        if i < completed {
            rows.push(product_event(
                bucket + Duration::minutes(10),
                project,
                &distinct_id,
                &session_id,
                "checkout_completed",
            ));
        }
    }
}

async fn seed(app: &TestApp, from: DateTime<Utc>) {
    let project = &app.project_slug;
    let buckets = [from, from + Duration::hours(1), from + Duration::hours(2), from + Duration::hours(3)];

    let spans = vec![
        span_at(buckets[0] + Duration::minutes(1), project, 1_100),
        span_at(buckets[0] + Duration::minutes(2), project, 1_200),
        span_at(buckets[1] + Duration::minutes(1), project, 1_300),
        span_at(buckets[1] + Duration::minutes(2), project, 1_400),
        span_at(buckets[2] + Duration::minutes(1), project, 2_300),
        span_at(buckets[2] + Duration::minutes(2), project, 2_500),
        span_at(buckets[3] + Duration::minutes(1), project, 2_400),
        span_at(buckets[3] + Duration::minutes(2), project, 2_600),
    ];
    app.ch.insert("faro.spans", &spans).await.expect("insert spans");

    let mut events = Vec::new();
    add_funnel_events(&mut events, buckets[0], project, "b0", 10, 8);
    add_funnel_events(&mut events, buckets[1], project, "b1", 10, 8);
    add_funnel_events(&mut events, buckets[2], project, "b2", 10, 5);
    add_funnel_events(&mut events, buckets[3], project, "b3", 10, 5);
    app.ch
        .insert("faro.product_events", &events)
        .await
        .expect("insert product events");
}

async fn query(
    app: &TestApp,
    session: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> LatencyFunnelImpactResult {
    let url = format!("{}/api/v1/insights/latency-funnel-impact", app.api_url);
    let from_s = from.to_rfc3339();
    let to_s = to.to_rfc3339();
    let resp = app
        .http
        .get(&url)
        .query(&[
            ("project", app.project_slug.as_str()),
            ("from", from_s.as_str()),
            ("to", to_s.as_str()),
            ("span_name", "/api/checkout"),
            ("service", "checkout-api"),
            ("latency_threshold_ms", "2000"),
            ("bucket_minutes", "60"),
        ])
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send");

    assert!(
        resp.status().is_success(),
        "GET /insights/latency-funnel-impact failed ({}): {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );

    resp.json().await.expect("decode json")
}

#[tokio::test]
async fn latency_funnel_impact_reports_conversion_drop_when_p95_is_slow() {
    let app = TestApp::spawn().await;
    let email = app.create_user("hunter2-test").await;
    let session = app.login_session(&email, "hunter2-test").await;
    let from = Utc.with_ymd_and_hms(2026, 5, 24, 10, 0, 0).unwrap();
    let to = from + Duration::hours(4);
    seed(&app, from).await;

    let result = query(&app, &session, from, to).await;

    assert_eq!(result.span_name, "/api/checkout");
    assert_eq!(result.service_name, "checkout-api");
    assert_eq!(result.funnel_from, "checkout_started");
    assert_eq!(result.funnel_to, "checkout_completed");
    assert_eq!(result.bucket_minutes, 60);
    assert_eq!(result.p95_threshold_ms, 2_000);
    assert_eq!(result.slow_bucket_count, 2);
    assert_eq!(result.baseline_bucket_count, 2);
    assert!((result.baseline_conversion_rate - 0.8).abs() < 0.0001);
    assert!((result.slow_conversion_rate - 0.5).abs() < 0.0001);
    assert!((result.conversion_drop_points - 30.0).abs() < 0.0001);
    assert_eq!(
        result.summary,
        "Cuando /api/checkout p95 supera 2s, el funnel checkout cae 30 puntos."
    );
    assert_eq!(result.buckets.len(), 4);
    assert_eq!(result.buckets.iter().filter(|b| b.slow).count(), 2);
    assert!(result.buckets.iter().any(|b| !b.slow && b.funnel_started == 10 && b.funnel_completed == 8));
    assert!(result.buckets.iter().any(|b| b.slow && b.funnel_started == 10 && b.funnel_completed == 5));
}

#[tokio::test]
async fn latency_funnel_impact_requires_session() {
    let app = TestApp::spawn().await;

    let resp = app
        .http
        .get(format!(
            "{}/api/v1/insights/latency-funnel-impact?span_name=/api/checkout",
            app.api_url
        ))
        .send()
        .await
        .expect("send");

    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
```

- [ ] **Step 2: Run the integration test and verify RED**

Run:

```powershell
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc "/usr/local/cargo/bin/cargo test --test latency_funnel_impact"
```

Expected: the authenticated test fails with `404 Not Found` because the route is not implemented yet. The unauthenticated test may pass.

## Task 4: Implement The Latency Funnel Impact Endpoint

**Files:**

- Modify: `backend/src/api/insights.rs`

- [ ] **Step 1: Add imports and route**

At the top of `backend/src/api/insights.rs`, change imports:

```rust
use std::collections::BTreeMap;
```

Change `router()` to include both insights:

```rust
pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/insights/revenue-impact", get(revenue_impact))
        .route("/insights/latency-funnel-impact", get(latency_funnel_impact))
}
```

- [ ] **Step 2: Add constants and types**

Add below existing revenue constants/types:

```rust
const DEFAULT_FUNNEL_FROM_EVENT: &str = "checkout_started";
const DEFAULT_LATENCY_THRESHOLD_MS: u32 = 2_000;
const DEFAULT_BUCKET_MINUTES: u32 = 60;
const MAX_BUCKET_MINUTES: u32 = 24 * 60;

#[derive(Debug, Deserialize)]
pub struct LatencyFunnelImpactQuery {
    #[serde(flatten)]
    pub range: Range,
    pub span_name: String,
    pub service: Option<String>,
    pub funnel_from: Option<String>,
    pub funnel_to: Option<String>,
    pub latency_threshold_ms: Option<u32>,
    pub bucket_minutes: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LatencyFunnelImpactResult {
    pub span_name: String,
    pub service_name: String,
    pub funnel_from: String,
    pub funnel_to: String,
    pub bucket_minutes: u32,
    pub p95_threshold_ms: u32,
    pub slow_bucket_count: u32,
    pub baseline_bucket_count: u32,
    pub baseline_conversion_rate: f64,
    pub slow_conversion_rate: f64,
    pub conversion_drop_points: f64,
    pub summary: String,
    pub buckets: Vec<LatencyFunnelBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyFunnelBucket {
    pub bucket_start: String,
    pub p95_latency_ms: f64,
    pub funnel_started: u64,
    pub funnel_completed: u64,
    pub conversion_rate: f64,
    pub slow: bool,
}

#[derive(Debug, Deserialize)]
struct SpanLatencyBucketRow {
    bucket_start: String,
    p95_latency_ms: f64,
}

#[derive(Debug, Deserialize)]
struct FunnelBucketRow {
    bucket_start: String,
    funnel_started: u64,
    funnel_completed: u64,
}
```

- [ ] **Step 3: Add the handler**

Add this handler before the helper functions:

```rust
async fn latency_funnel_impact(
    State(state): State<SharedState>,
    Query(q): Query<LatencyFunnelImpactQuery>,
) -> ApiResult<Json<LatencyFunnelImpactResult>> {
    let span_name = q.span_name.trim();
    if span_name.is_empty() {
        return Err(ApiError::BadRequest("span_name no puede ser vacío".into()));
    }

    let funnel_from = q.funnel_from.as_deref().unwrap_or(DEFAULT_FUNNEL_FROM_EVENT).trim();
    let funnel_to = q.funnel_to.as_deref().unwrap_or(DEFAULT_CHECKOUT_EVENT).trim();
    if funnel_from.is_empty() || funnel_to.is_empty() {
        return Err(ApiError::BadRequest(
            "funnel_from y funnel_to no pueden ser vacíos".into(),
        ));
    }
    if funnel_from == funnel_to {
        return Err(ApiError::BadRequest(
            "funnel_from y funnel_to deben ser distintos".into(),
        ));
    }

    let threshold_ms = q.latency_threshold_ms.unwrap_or(DEFAULT_LATENCY_THRESHOLD_MS);
    if threshold_ms == 0 {
        return Err(ApiError::BadRequest(
            "latency_threshold_ms debe ser positivo".into(),
        ));
    }
    let bucket_minutes = q
        .bucket_minutes
        .unwrap_or(DEFAULT_BUCKET_MINUTES)
        .clamp(1, MAX_BUCKET_MINUTES);
    let bucket_seconds = (bucket_minutes * 60).to_string();

    let (from, to) = q.range.resolve();
    if from >= to {
        return Err(ApiError::BadRequest("rango temporal inválido".into()));
    }
    let from_s = ch_dt(from);
    let to_s = ch_dt(to);

    let project_clause_plain = match &q.range.project {
        Some(p) if !p.is_empty() => " AND project_id = {project:String}",
        _ => "",
    };
    let service_clause = match &q.service {
        Some(s) if !s.is_empty() => " AND service_name = {service:String}",
        _ => "",
    };

    let mut params: Vec<(&str, &str)> = vec![
        ("from", from_s.as_str()),
        ("to", to_s.as_str()),
        ("span_name", span_name),
        ("funnel_from", funnel_from),
        ("funnel_to", funnel_to),
        ("bucket_seconds", bucket_seconds.as_str()),
    ];
    if let Some(project) = &q.range.project {
        if !project.is_empty() {
            params.push(("project", project.as_str()));
        }
    }
    if let Some(service) = &q.service {
        if !service.is_empty() {
            params.push(("service", service.as_str()));
        }
    }

    let span_sql = format!(
        "SELECT toString(toStartOfInterval(timestamp, toIntervalSecond({{bucket_seconds:UInt32}}))) AS bucket_start, \
                toFloat64(quantileExact(0.95)(duration_ns)) / 1000000.0 AS p95_latency_ms \
         FROM faro.spans \
         WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
           AND name = {{span_name:String}}{project_clause_plain}{service_clause} \
         GROUP BY bucket_start \
         ORDER BY bucket_start"
    );
    let span_rows: Vec<SpanLatencyBucketRow> =
        state.ch.select_with_params(&span_sql, &params).await?;

    let funnel_sql = format!(
        "SELECT toString(toStartOfInterval(timestamp, toIntervalSecond({{bucket_seconds:UInt32}}))) AS bucket_start, \
                toUInt64(uniqExactIf(distinct_id, event_name = {{funnel_from:String}})) AS funnel_started, \
                toUInt64(uniqExactIf(distinct_id, event_name = {{funnel_to:String}})) AS funnel_completed \
         FROM faro.product_events \
         WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <  toDateTime64({{to:DateTime64(9)}}, 9) \
           AND event_name IN ({{funnel_from:String}}, {{funnel_to:String}}){project_clause_plain} \
         GROUP BY bucket_start \
         ORDER BY bucket_start"
    );
    let funnel_rows: Vec<FunnelBucketRow> =
        state.ch.select_with_params(&funnel_sql, &params).await?;

    let mut buckets_by_start: BTreeMap<String, LatencyFunnelBucket> = BTreeMap::new();
    for row in span_rows {
        buckets_by_start.insert(
            row.bucket_start.clone(),
            LatencyFunnelBucket {
                bucket_start: row.bucket_start,
                p95_latency_ms: row.p95_latency_ms,
                funnel_started: 0,
                funnel_completed: 0,
                conversion_rate: 0.0,
                slow: row.p95_latency_ms >= threshold_ms as f64,
            },
        );
    }
    for row in funnel_rows {
        let bucket = buckets_by_start
            .entry(row.bucket_start.clone())
            .or_insert_with(|| LatencyFunnelBucket {
                bucket_start: row.bucket_start,
                p95_latency_ms: 0.0,
                funnel_started: 0,
                funnel_completed: 0,
                conversion_rate: 0.0,
                slow: false,
            });
        bucket.funnel_started = row.funnel_started;
        bucket.funnel_completed = row.funnel_completed;
    }

    let mut buckets: Vec<LatencyFunnelBucket> = buckets_by_start
        .into_values()
        .map(|mut bucket| {
            bucket.conversion_rate =
                funnel_conversion_rate(bucket.funnel_started, bucket.funnel_completed);
            bucket.slow = bucket.p95_latency_ms >= threshold_ms as f64;
            bucket
        })
        .collect();

    let mut slow_started = 0u64;
    let mut slow_completed = 0u64;
    let mut baseline_started = 0u64;
    let mut baseline_completed = 0u64;
    let mut slow_bucket_count = 0u32;
    let mut baseline_bucket_count = 0u32;
    for bucket in &buckets {
        if bucket.funnel_started == 0 {
            continue;
        }
        if bucket.slow {
            slow_bucket_count += 1;
            slow_started += bucket.funnel_started;
            slow_completed += bucket.funnel_completed;
        } else {
            baseline_bucket_count += 1;
            baseline_started += bucket.funnel_started;
            baseline_completed += bucket.funnel_completed;
        }
    }

    let baseline_conversion_rate = funnel_conversion_rate(baseline_started, baseline_completed);
    let slow_conversion_rate = funnel_conversion_rate(slow_started, slow_completed);
    let drop_points = conversion_drop_points(baseline_conversion_rate, slow_conversion_rate);

    buckets.truncate(q.range.limit() as usize);
    let service_name = q.service.clone().unwrap_or_default();

    Ok(Json(LatencyFunnelImpactResult {
        span_name: span_name.to_string(),
        service_name,
        funnel_from: funnel_from.to_string(),
        funnel_to: funnel_to.to_string(),
        bucket_minutes,
        p95_threshold_ms: threshold_ms,
        slow_bucket_count,
        baseline_bucket_count,
        baseline_conversion_rate,
        slow_conversion_rate,
        conversion_drop_points: drop_points,
        summary: latency_funnel_summary(span_name, threshold_ms, drop_points),
        buckets,
    }))
}
```

- [ ] **Step 4: Run the integration test and fix only implementation issues**

Run:

```powershell
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc "/usr/local/cargo/bin/cargo test --test latency_funnel_impact"
```

Expected:

```text
test result: ok. 2 passed
```

If ClickHouse rejects `toIntervalSecond({bucket_seconds:UInt32})`, change both SQL queries to use a validated interpolated integer:

```rust
let bucket_seconds_sql = bucket_minutes * 60;
...
toStartOfInterval(timestamp, INTERVAL {bucket_seconds_sql} second)
```

Only interpolate `bucket_seconds_sql`, which is server-side validated numeric code, never raw user text.

## Task 5: Final Verification

**Files:**

- Verify: `backend/src/api/insights.rs`
- Verify: `backend/tests/latency_funnel_impact.rs`
- Existing: `backend/tests/revenue_impact_insights.rs`

- [ ] **Step 1: Run focused verification**

Run:

```powershell
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc "/usr/local/cargo/bin/rustup component add rustfmt >/tmp/rustfmt-install.log && /usr/local/cargo/bin/rustfmt --edition 2021 --check src/api/insights.rs tests/latency_funnel_impact.rs tests/revenue_impact_insights.rs && /usr/local/cargo/bin/cargo check && /usr/local/cargo/bin/cargo test --test latency_funnel_impact && /usr/local/cargo/bin/cargo test --test revenue_impact_insights"
```

Expected:

```text
Finished `dev` profile
test result: ok. 2 passed
test result: ok. 2 passed
```

- [ ] **Step 2: Document known full-suite blockers**

Run:

```powershell
docker compose -f docker-compose.test.yml run --rm --entrypoint bash backend-test -lc "/usr/local/cargo/bin/cargo test api::insights::tests"
```

Expected in this workspace: the command may fail before reaching insights unit tests because unrelated existing unit tests in `src/ingest/events.rs` do not compile. If it fails for those unrelated errors, record that in the final response and rely on the focused integration tests plus `cargo check`.

- [ ] **Step 3: Inspect final diff**

Run:

```powershell
git diff -- backend/src/api/insights.rs backend/tests/latency_funnel_impact.rs
```

Expected: only the latency-funnel endpoint, helpers, tests, and route addition inside `insights::router()`.
