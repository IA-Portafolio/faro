# Revenue Impact Insights Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `GET /api/v1/insights/revenue-impact`, ranking error issues by checkout conversion impact and estimated lost revenue.

**Architecture:** Add a focused `api::insights` module with one route, typed query/response structs, small pure revenue math helpers, and ClickHouse SQL that joins error sessions to checkout product events. Register the router under the existing authenticated `/api/v1` router. Cover the behavior with helper unit tests and a route-level integration test that seeds ClickHouse.

**Tech Stack:** Rust 2021, Axum 0.7, serde, chrono, ClickHouse HTTP JSONEachRow via `storage::Client`, existing `backend/tests/common` integration harness.

---

## File Structure

- Create `backend/src/api/insights.rs`: route module, query parsing, response structs, helper math, ClickHouse queries, unit tests.
- Modify `backend/src/api/mod.rs`: expose and merge the new insights router.
- Create `backend/tests/revenue_impact_insights.rs`: route-level integration test with seeded product events and error events.

## Task 1: Add Failing Unit Tests For Revenue Math

**Files:**

- Create: `backend/src/api/insights.rs`
- Modify: `backend/src/api/mod.rs`

- [ ] **Step 1: Create the new module with tests that call not-yet-implemented helpers**

Create `backend/src/api/insights.rs` with this exact content:

```rust
use axum::Router;

use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_rate_counts_completed_sessions_over_affected_sessions() {
        let rate = conversion_rate(12, 5);

        assert!((rate - 7.0 / 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn conversion_rate_is_zero_when_there_are_no_affected_sessions() {
        assert_eq!(conversion_rate(0, 0), 0.0);
    }

    #[test]
    fn estimated_lost_revenue_uses_positive_conversion_gap() {
        let lost = estimated_lost_revenue(0.71, 0.60, 1_247, 100.0);

        assert!((lost - 13_717.0).abs() < 0.0001);
    }

    #[test]
    fn estimated_lost_revenue_clamps_when_issue_outperforms_baseline() {
        let lost = estimated_lost_revenue(0.40, 0.60, 1_247, 100.0);

        assert_eq!(lost, 0.0);
    }
}
```

- [ ] **Step 2: Register the module without adding the route merge yet**

In `backend/src/api/mod.rs`, add the module declaration next to the other API modules:

```rust
pub mod insights;
```

Do not add `.merge(insights::router())` yet in this task.

- [ ] **Step 3: Run the unit tests and verify they fail for the expected reason**

Run:

```powershell
cd backend
cargo test api::insights::tests
```

Expected: compile failure with unresolved functions including:

```text
cannot find function `conversion_rate` in this scope
cannot find function `estimated_lost_revenue` in this scope
```

This is the RED step for the pure math.

## Task 2: Implement Revenue Math Helpers

**Files:**

- Modify: `backend/src/api/insights.rs`

- [ ] **Step 1: Add the minimal helper implementations**

In `backend/src/api/insights.rs`, add these functions above the test module:

```rust
fn conversion_rate(affected_sessions: u64, sessions_without_checkout: u64) -> f64 {
    if affected_sessions == 0 {
        return 0.0;
    }
    let completed = affected_sessions.saturating_sub(sessions_without_checkout);
    completed as f64 / affected_sessions as f64
}

fn estimated_lost_revenue(
    baseline_conversion_rate: f64,
    issue_conversion_rate: f64,
    affected_sessions: u64,
    average_order_value: f64,
) -> f64 {
    let gap = (baseline_conversion_rate - issue_conversion_rate).max(0.0);
    gap * affected_sessions as f64 * average_order_value
}
```

- [ ] **Step 2: Run the unit tests and verify they pass**

Run:

```powershell
cd backend
cargo test api::insights::tests
```

Expected:

```text
test result: ok. 4 passed
```

- [ ] **Step 3: Commit the helper tests and implementation**

Run:

```powershell
git add backend/src/api/insights.rs backend/src/api/mod.rs
git commit -m "test: add revenue impact math"
```

## Task 3: Add A Failing Integration Test For The Endpoint

**Files:**

- Create: `backend/tests/revenue_impact_insights.rs`

- [ ] **Step 1: Create the route-level integration test**

Create `backend/tests/revenue_impact_insights.rs` with this exact content:

```rust
mod common;

use chrono::{Duration, Utc};
use common::TestApp;
use faro::storage::{AttrMap, ErrorEventRow, ProductEventRow};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct RevenueImpactIssue {
    fingerprint: String,
    service_name: String,
    exception_type: String,
    message: String,
    affected_sessions: u64,
    sessions_without_checkout: u64,
    issue_conversion_rate: f64,
    baseline_conversion_rate: f64,
    conversion_gap: f64,
    estimated_lost_revenue: f64,
}

fn product_event(
    secs_ago: i64,
    project: &str,
    session_id: &str,
    distinct_id: &str,
    event_name: &str,
) -> ProductEventRow {
    ProductEventRow {
        timestamp: Utc::now() - Duration::seconds(secs_ago),
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

fn error_event(
    secs_ago: i64,
    project: &str,
    session_id: &str,
    fingerprint: &str,
    service_name: &str,
    message: &str,
) -> ErrorEventRow {
    let mut attributes = AttrMap::new();
    attributes.insert("session_id".into(), session_id.into());

    ErrorEventRow {
        timestamp: Utc::now() - Duration::seconds(secs_ago),
        project_id: project.into(),
        fingerprint: fingerprint.into(),
        service_name: service_name.into(),
        severity_text: "ERROR".into(),
        message: message.into(),
        exception_type: "TypeError".into(),
        exception_message: message.into(),
        stack_trace: String::new(),
        trace_id: String::new(),
        span_id: String::new(),
        attributes,
    }
}

async fn seed(app: &TestApp) {
    let p = &app.project_slug;
    let product_events = vec![
        product_event(600, p, "s-1", "u-1", "checkout_started"),
        product_event(590, p, "s-1", "u-1", "checkout_completed"),
        product_event(580, p, "s-2", "u-2", "checkout_started"),
        product_event(570, p, "s-2", "u-2", "checkout_completed"),
        product_event(560, p, "s-3", "u-3", "checkout_started"),
        product_event(550, p, "s-3", "u-3", "checkout_completed"),
        product_event(540, p, "s-4", "u-4", "checkout_started"),
        product_event(530, p, "s-4", "u-4", "checkout_completed"),
        product_event(520, p, "s-5", "u-5", "checkout_started"),
        product_event(510, p, "s-5", "u-5", "checkout_completed"),
        product_event(500, p, "s-6", "u-6", "checkout_started"),
        product_event(490, p, "s-6", "u-6", "checkout_completed"),
        product_event(480, p, "s-7", "u-7", "checkout_started"),
        product_event(470, p, "s-7", "u-7", "checkout_completed"),
        product_event(460, p, "s-8", "u-8", "checkout_started"),
        product_event(450, p, "s-8", "u-8", "checkout_completed"),
        product_event(440, p, "s-9", "u-9", "checkout_started"),
        product_event(430, p, "s-10", "u-10", "checkout_started"),
    ];
    app.ch
        .insert("faro.product_events", &product_events)
        .await
        .expect("insert product events");

    let errors = vec![
        error_event(425, p, "s-9", "fp-payment", "checkout-api", "payment provider failed"),
        error_event(415, p, "s-10", "fp-payment", "checkout-api", "payment provider failed"),
        error_event(595, p, "s-1", "fp-ui", "web", "button label missing"),
    ];
    app.ch
        .insert("faro.error_events", &errors)
        .await
        .expect("insert error events");
}

async fn query(app: &TestApp, session: &str) -> Vec<RevenueImpactIssue> {
    let url = format!(
        "{}/api/v1/insights/revenue-impact?project={}&last_minutes=120&average_order_value=100&limit=10",
        app.api_url, app.project_slug
    );
    let resp = app
        .http
        .get(&url)
        .header(reqwest::header::COOKIE, format!("faro_session={session}"))
        .send()
        .await
        .expect("send");

    assert!(
        resp.status().is_success(),
        "GET /insights/revenue-impact failed ({}): {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );

    resp.json().await.expect("decode json")
}

#[tokio::test]
async fn revenue_impact_prioritizes_errors_by_checkout_loss() {
    let app = TestApp::spawn().await;
    let email = app.create_user("hunter2-test").await;
    let session = app.login_session(&email, "hunter2-test").await;
    seed(&app).await;

    let rows = query(&app, &session).await;

    assert_eq!(rows.len(), 2);

    let top = &rows[0];
    assert_eq!(top.fingerprint, "fp-payment");
    assert_eq!(top.service_name, "checkout-api");
    assert_eq!(top.exception_type, "TypeError");
    assert_eq!(top.message, "payment provider failed");
    assert_eq!(top.affected_sessions, 2);
    assert_eq!(top.sessions_without_checkout, 2);
    assert!((top.issue_conversion_rate - 0.0).abs() < 0.0001);
    assert!((top.baseline_conversion_rate - 0.8).abs() < 0.0001);
    assert!((top.conversion_gap - 0.8).abs() < 0.0001);
    assert!((top.estimated_lost_revenue - 160.0).abs() < 0.0001);

    let second = &rows[1];
    assert_eq!(second.fingerprint, "fp-ui");
    assert_eq!(second.affected_sessions, 1);
    assert_eq!(second.sessions_without_checkout, 0);
    assert_eq!(second.estimated_lost_revenue, 0.0);
}

#[tokio::test]
async fn revenue_impact_requires_session() {
    let app = TestApp::spawn().await;

    let resp = app
        .http
        .get(format!("{}/api/v1/insights/revenue-impact", app.api_url))
        .send()
        .await
        .expect("send");

    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
```

- [ ] **Step 2: Run the integration test and verify it fails because the route is missing**

Run:

```powershell
cd backend
cargo test --test revenue_impact_insights
```

Expected: `revenue_impact_prioritizes_errors_by_checkout_loss` fails with an HTTP error status, typically `404 Not Found`, because the route has not been merged/implemented yet. `revenue_impact_requires_session` may pass depending on router ordering; the RED proof is the first test.

## Task 4: Implement The Revenue Impact Endpoint

**Files:**

- Modify: `backend/src/api/insights.rs`
- Modify: `backend/src/api/mod.rs`

- [ ] **Step 1: Replace `backend/src/api/insights.rs` with the route implementation**

Use this complete module:

```rust
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};

use crate::api::params::{ch_dt, Range};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;

const DEFAULT_CHECKOUT_EVENT: &str = "checkout_completed";
const DEFAULT_AVERAGE_ORDER_VALUE: f64 = 100.0;

pub fn router() -> Router<SharedState> {
    Router::new().route("/insights/revenue-impact", get(revenue_impact))
}

#[derive(Debug, Deserialize)]
pub struct RevenueImpactQuery {
    #[serde(flatten)]
    pub range: Range,
    pub checkout_event: Option<String>,
    pub average_order_value: Option<f64>,
    pub service: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevenueImpactIssue {
    pub fingerprint: String,
    pub service_name: String,
    pub exception_type: String,
    pub message: String,
    pub affected_sessions: u64,
    pub sessions_without_checkout: u64,
    pub issue_conversion_rate: f64,
    pub baseline_conversion_rate: f64,
    pub conversion_gap: f64,
    pub estimated_lost_revenue: f64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Deserialize)]
struct BaselineRow {
    total_sessions: u64,
    sessions_with_checkout: u64,
}

#[derive(Debug, Deserialize)]
struct ImpactRow {
    fingerprint: String,
    service_name: String,
    exception_type: String,
    message: String,
    affected_sessions: u64,
    sessions_without_checkout: u64,
    first_seen: String,
    last_seen: String,
}

async fn revenue_impact(
    State(state): State<SharedState>,
    Query(q): Query<RevenueImpactQuery>,
) -> ApiResult<Json<Vec<RevenueImpactIssue>>> {
    let checkout_event = q
        .checkout_event
        .as_deref()
        .unwrap_or(DEFAULT_CHECKOUT_EVENT)
        .trim();
    if checkout_event.is_empty() {
        return Err(ApiError::BadRequest(
            "checkout_event no puede ser vacío".into(),
        ));
    }

    let average_order_value = q.average_order_value.unwrap_or(DEFAULT_AVERAGE_ORDER_VALUE);
    if !average_order_value.is_finite() || average_order_value <= 0.0 {
        return Err(ApiError::BadRequest(
            "average_order_value debe ser un número positivo".into(),
        ));
    }

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
    let project_clause_pe = match &q.range.project {
        Some(p) if !p.is_empty() => " AND pe.project_id = {project:String}",
        _ => "",
    };
    let project_clause_e = match &q.range.project {
        Some(p) if !p.is_empty() => " AND e.project_id = {project:String}",
        _ => "",
    };
    let service_clause = match &q.service {
        Some(s) if !s.is_empty() => " AND e.service_name = {service:String}",
        _ => "",
    };

    let mut params: Vec<(&str, &str)> = vec![
        ("from", from_s.as_str()),
        ("to", to_s.as_str()),
        ("checkout_event", checkout_event),
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

    let baseline_sql = format!(
        "WITH sessions AS ( \
           SELECT project_id, session_id, max(event_name = {{checkout_event:String}}) AS has_checkout \
           FROM faro.product_events \
           WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
             AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
             AND session_id != ''{project_clause_plain} \
           GROUP BY project_id, session_id \
         ) \
         SELECT toUInt64(count()) AS total_sessions, \
                toUInt64(sum(has_checkout)) AS sessions_with_checkout \
         FROM sessions"
    );

    let baseline = state
        .ch
        .select_one_with_params::<BaselineRow>(&baseline_sql, &params)
        .await?
        .unwrap_or(BaselineRow {
            total_sessions: 0,
            sessions_with_checkout: 0,
        });
    let baseline_conversion_rate = if baseline.total_sessions == 0 {
        0.0
    } else {
        baseline.sessions_with_checkout as f64 / baseline.total_sessions as f64
    };

    let impact_sql = format!(
        "WITH error_sessions AS ( \
           SELECT e.project_id AS project_id, \
                  e.session_id AS session_id, \
                  e.fingerprint AS fingerprint, \
                  e.service_name AS service_name, \
                  argMax(e.exception_type, e.timestamp) AS exception_type, \
                  argMax(e.message, e.timestamp) AS message, \
                  min(e.timestamp) AS first_error_at, \
                  min(e.timestamp) AS first_seen_ts, \
                  max(e.timestamp) AS last_seen_ts \
           FROM ( \
             SELECT timestamp, project_id, fingerprint, service_name, exception_type, message, \
                    attributes['session_id'] AS session_id \
             FROM faro.error_events \
             WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
               AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
           ) AS e \
           WHERE e.session_id != ''{project_clause_e}{service_clause} \
           GROUP BY e.project_id, e.session_id, e.fingerprint, e.service_name \
         ), \
         issue_sessions AS ( \
           SELECT es.fingerprint AS fingerprint, \
                  es.service_name AS service_name, \
                  es.session_id AS session_id, \
                  any(es.exception_type) AS exception_type, \
                  any(es.message) AS message, \
                  min(es.first_seen_ts) AS first_seen_ts, \
                  max(es.last_seen_ts) AS last_seen_ts, \
                  max(pe.event_name = {{checkout_event:String}} AND pe.timestamp > es.first_error_at) AS has_checkout_after_error \
           FROM error_sessions AS es \
           LEFT JOIN faro.product_events AS pe \
             ON pe.project_id = es.project_id \
            AND pe.session_id = es.session_id \
            AND pe.timestamp >= es.first_error_at \
            AND pe.timestamp <= toDateTime64({{to:DateTime64(9)}}, 9){project_clause_pe} \
           GROUP BY es.fingerprint, es.service_name, es.session_id \
         ) \
         SELECT fingerprint, \
                service_name, \
                any(exception_type) AS exception_type, \
                any(message) AS message, \
                toUInt64(count()) AS affected_sessions, \
                toUInt64(sum(if(has_checkout_after_error = 0, 1, 0))) AS sessions_without_checkout, \
                toString(min(first_seen_ts)) AS first_seen, \
                toString(max(last_seen_ts)) AS last_seen \
         FROM issue_sessions \
         GROUP BY fingerprint, service_name"
    );

    let rows: Vec<ImpactRow> = state.ch.select_with_params(&impact_sql, &params).await?;
    let mut issues: Vec<RevenueImpactIssue> = rows
        .into_iter()
        .filter(|row| row.affected_sessions > 0)
        .map(|row| {
            let issue_conversion_rate =
                conversion_rate(row.affected_sessions, row.sessions_without_checkout);
            let conversion_gap = (baseline_conversion_rate - issue_conversion_rate).max(0.0);
            let estimated_lost_revenue = estimated_lost_revenue(
                baseline_conversion_rate,
                issue_conversion_rate,
                row.affected_sessions,
                average_order_value,
            );

            RevenueImpactIssue {
                fingerprint: row.fingerprint,
                service_name: row.service_name,
                exception_type: row.exception_type,
                message: row.message,
                affected_sessions: row.affected_sessions,
                sessions_without_checkout: row.sessions_without_checkout,
                issue_conversion_rate,
                baseline_conversion_rate,
                conversion_gap,
                estimated_lost_revenue,
                first_seen: row.first_seen,
                last_seen: row.last_seen,
            }
        })
        .collect();

    issues.sort_by(|a, b| {
        b.estimated_lost_revenue
            .partial_cmp(&a.estimated_lost_revenue)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.sessions_without_checkout.cmp(&a.sessions_without_checkout))
    });
    issues.truncate(q.range.limit() as usize);

    Ok(Json(issues))
}

fn conversion_rate(affected_sessions: u64, sessions_without_checkout: u64) -> f64 {
    if affected_sessions == 0 {
        return 0.0;
    }
    let completed = affected_sessions.saturating_sub(sessions_without_checkout);
    completed as f64 / affected_sessions as f64
}

fn estimated_lost_revenue(
    baseline_conversion_rate: f64,
    issue_conversion_rate: f64,
    affected_sessions: u64,
    average_order_value: f64,
) -> f64 {
    let gap = (baseline_conversion_rate - issue_conversion_rate).max(0.0);
    gap * affected_sessions as f64 * average_order_value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_rate_counts_completed_sessions_over_affected_sessions() {
        let rate = conversion_rate(12, 5);

        assert!((rate - 7.0 / 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn conversion_rate_is_zero_when_there_are_no_affected_sessions() {
        assert_eq!(conversion_rate(0, 0), 0.0);
    }

    #[test]
    fn estimated_lost_revenue_uses_positive_conversion_gap() {
        let lost = estimated_lost_revenue(0.71, 0.60, 1_247, 100.0);

        assert!((lost - 13_717.0).abs() < 0.0001);
    }

    #[test]
    fn estimated_lost_revenue_clamps_when_issue_outperforms_baseline() {
        let lost = estimated_lost_revenue(0.40, 0.60, 1_247, 100.0);

        assert_eq!(lost, 0.0);
    }
}
```

- [ ] **Step 2: Merge the router**

In `backend/src/api/mod.rs`, add this line inside `fn v1_router() -> Router<SharedState>` after `.merge(funnels::router())`:

```rust
        .merge(insights::router())
```

- [ ] **Step 3: Run unit and integration tests**

Run:

```powershell
cd backend
cargo test api::insights::tests
cargo test --test revenue_impact_insights
```

Expected:

```text
test result: ok. 4 passed
test result: ok. 2 passed
```

- [ ] **Step 4: Commit the endpoint**

Run:

```powershell
git add backend/src/api/insights.rs backend/src/api/mod.rs backend/tests/revenue_impact_insights.rs
git commit -m "feat: add revenue impact insights endpoint"
```

## Task 5: Final Verification

**Files:**

- Verify: `backend/src/api/insights.rs`
- Verify: `backend/src/api/mod.rs`
- Verify: `backend/tests/revenue_impact_insights.rs`

- [ ] **Step 1: Run focused backend verification**

Run:

```powershell
cd backend
cargo test api::insights::tests
cargo test --test revenue_impact_insights
cargo check
```

Expected:

```text
test result: ok. 4 passed
test result: ok. 2 passed
Finished `dev` profile
```

- [ ] **Step 2: Confirm the new route appears in the router code**

Run:

```powershell
Select-String -Path backend/src/api/mod.rs -Pattern "insights"
```

Expected output includes:

```text
pub mod insights;
.merge(insights::router())
```

- [ ] **Step 3: Inspect the final diff**

Run:

```powershell
git diff -- backend/src/api/insights.rs backend/src/api/mod.rs backend/tests/revenue_impact_insights.rs
```

Expected: only the new insights module, module registration/router merge, and integration test changed.
