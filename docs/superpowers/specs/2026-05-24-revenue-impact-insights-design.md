# Revenue Impact Insights Design

## Goal

Add `GET /api/v1/insights/revenue-impact`, an enterprise-grade insight that answers:

> Which errors affected sessions where checkout was not completed afterward?

The endpoint prioritizes error issues by potential lost revenue and returns enough context to support copy like:

> This error affected 1,247 sessions; 832 of them did not complete checkout (60% conversion vs 71% baseline). Estimated loss: $14k this week.

## Scope

This iteration ships the backend API only. It uses existing ClickHouse tables:

- `faro.error_events`
- `faro.product_events`
- `faro.product_sessions`

No schema migration or materialized view is required.

## API

Route:

```http
GET /api/v1/insights/revenue-impact
```

Query parameters:

- Common range params from `api::params::Range`: `from`, `to`, `last_minutes`, `limit`, `project`.
- `checkout_event`: optional event name that represents successful purchase completion. Default: `checkout_completed`.
- `average_order_value`: optional positive numeric value used to estimate lost revenue. Default: `100`.
- `service`: optional service filter for the originating error.

Response:

```json
[
  {
    "fingerprint": "abc123",
    "service_name": "checkout-api",
    "exception_type": "TypeError",
    "message": "Cannot read properties of undefined",
    "affected_sessions": 1247,
    "sessions_without_checkout": 832,
    "issue_conversion_rate": 0.333,
    "baseline_conversion_rate": 0.71,
    "conversion_gap": 0.377,
    "estimated_lost_revenue": 47001.9,
    "first_seen": "2026-05-18 10:00:00.000000000",
    "last_seen": "2026-05-24 15:00:00.000000000"
  }
]
```

## Semantics

An affected session is a session that has at least one `error_events` row with a non-empty `session_id`.

An impacted checkout is counted as missing when there is no `product_events` row in the same `(project_id, session_id)` with `event_name = checkout_event` and `timestamp > first_error_at`. This keeps the query aligned with the product question: the error happened before the session failed to complete checkout.

The issue conversion rate is:

```text
(affected_sessions - sessions_without_checkout) / affected_sessions
```

The baseline conversion rate is computed across all sessions with product activity in the selected range:

```text
sessions_with_checkout / total_sessions
```

The estimated lost revenue is:

```text
max(baseline_conversion_rate - issue_conversion_rate, 0)
  * affected_sessions
  * average_order_value
```

The endpoint sorts by `estimated_lost_revenue DESC`, then by `sessions_without_checkout DESC`.

## Architecture

Add `backend/src/api/insights.rs` with:

- `router() -> Router<SharedState>`
- `RevenueImpactQuery`
- `RevenueImpactIssue`
- `revenue_impact` handler
- Small pure helpers for conversion and lost revenue math

Register the router in `backend/src/api/mod.rs` with `pub mod insights;` and `.merge(insights::router())`.

The handler follows existing endpoint patterns:

- Resolve time ranges with `Range::resolve`.
- Format ClickHouse dates with `ch_dt`.
- Bind user input via `select_with_params`.
- Reuse project filtering style from the existing API modules.
- Return `ApiError::BadRequest` for invalid `average_order_value` or empty `checkout_event`.

## Data Flow

1. Resolve range and query params.
2. Query ClickHouse for the baseline conversion rate.
3. Query ClickHouse for per-fingerprint affected sessions and sessions without checkout.
4. Compute conversion rate, gap, and lost revenue in Rust.
5. Return rows already sorted by lost revenue.

The first implementation may use two ClickHouse queries rather than a single deeply nested query. That keeps the SQL readable and makes the business math testable in Rust.

## Error Handling

- Empty `checkout_event` returns `400`.
- Non-positive or non-finite `average_order_value` returns `400`.
- ClickHouse query failures propagate through existing `ApiResult`.
- If there are no baseline sessions, `baseline_conversion_rate` is `0.0`.
- If an issue has zero affected sessions, it is not returned.

## Testing

Add focused Rust unit tests for the pure math:

- Conversion rate is `completed / affected`.
- Conversion rate is `0.0` when affected sessions are zero.
- Lost revenue clamps negative conversion gaps to zero.
- Lost revenue multiplies conversion gap, affected sessions, and AOV.

If the existing integration test harness can seed ClickHouse cheaply, add a route-level integration test with:

- A baseline set of sessions with and without checkout.
- One error fingerprint whose sessions underperform baseline.
- A second fingerprint with lower or zero estimated impact.

If the integration harness is not practical in this pass, keep the endpoint SQL covered by compile checks and pure helper tests.

## Future Work

- Derive `average_order_value` from `checkout_completed.properties.amount` or `properties.revenue` once the SDK/event contract standardizes revenue fields.
- Add a materialized view if the endpoint becomes hot on large tenants.
- Surface representative replay/session links for the top impacted sessions.
- Add frontend UI copy that renders the enterprise value statement directly.
