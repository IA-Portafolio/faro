# Product Retention Design

## Goal

Build `/retention` as a product analytics view that answers: "do users come back?" It must show a classic cohort heatmap with D1, D7, and D30 retention for app end-users (`distinct_id`), not Faro dashboard users.

## Recommended Approach

Use a dedicated backend endpoint, `GET /api/v1/retention`, instead of reusing saved cohorts. Saved cohorts answer "who matches this definition now"; this view answers "for users first seen on each cohort day, did they come back later?" Those are different analytics questions and should stay separate.

The endpoint computes cohorts from `faro.product_events`:

- Cohort date: `toDate(min(timestamp))` per `(project_id, distinct_id)` inside the selected range.
- Return event: any event by default, or a selected `event_name`.
- Returned on Dn: the user has at least one matching return event on `cohort_date + n`.
- Columns: D1, D7, D30.

This keeps the first implementation narrow, fast enough for product ranges, and compatible with existing event catalog endpoints.

## Backend

Add `backend/src/api/retention.rs`.

Routes:

- `GET /api/v1/retention`

Query params:

- Existing range params: `from`, `to`, `last_minutes`, `project`.
- `event_name`: optional. Empty or absent means "any event".
- `interval`: optional, default `day`. Only `day` is accepted for now.

Response:

```json
{
  "from": "2026-05-01T00:00:00.000Z",
  "to": "2026-05-24T23:59:59.000Z",
  "event_name": "",
  "interval": "day",
  "columns": [1, 7, 30],
  "cohorts": [
    {
      "cohort_date": "2026-05-01",
      "cohort_size": 123,
      "d1_users": 45,
      "d7_users": 22,
      "d30_users": 0
    }
  ],
  "took_ms": 42
}
```

The SQL must be parameterized. The query should group first-touch users by day, then join back to events for the selected return days. D30 can be zero for recent cohorts where the day has not elapsed; the UI will mark those cells as not mature instead of treating them as bad retention.

## Frontend

Add `/retention` with:

- Header: "Retention".
- Range and project inherit global stores.
- Event selector with default `Cualquier evento` and candidates from `fetchFunnelEvents`.
- Metric cards: total cohort users, weighted D1, weighted D7, weighted D30.
- Heatmap table: rows are cohort dates, columns are cohort size, D1, D7, D30.
- Cell color intensity is based on retention percentage, with unavailable future cells muted.
- Empty state uses existing `OnboardingEmpty kind="events"`.

Add helper module `frontend/src/lib/retention.ts` for pure calculations:

- `retentionRate(row, day)`
- `isMature(cohortDate, day, asOf)`
- `weightedRetention(cohorts, day, asOf)`
- `heatColor(rate, mature)`

## Navigation

Expose `/retention` in:

- Sidebar, between Funnels and Cohorts.
- Command palette as `nav.retention`.

## Error Handling

- Backend returns `400` for unsupported intervals or invalid time ranges.
- Frontend shows the API error inline and keeps filters intact.
- Empty cohorts render a product-events onboarding empty state.

## Testing

Use TDD:

- Backend unit tests for interval validation and weighted row math where possible without ClickHouse.
- Frontend unit tests for maturity/rate/weighted helper behavior.
- Palette test for `nav.retention`.
- Run frontend tests, backend tests for the new module, frontend build, and svelte-check. Existing unrelated `svelte-check` failures may remain documented.
