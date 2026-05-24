# 10.F.2 Session-Level Properties Design

## Context

Faro already materializes product sessions from `product_events` into
`product_sessions`. The current table stores `duration_seconds` and `page_count`,
but the worker currently computes `page_count` with `count()`, so it behaves like
total event count rather than pageview count.

10.F.2 adds session-level properties that product analytics and user timelines
can consume without scanning raw events for every query.

## Decision

Extend `faro.product_sessions` with additive session quality fields:

- `event_count UInt32`: total events in the session.
- `pageview_count UInt32`: count of page/screen navigation events.
- `is_bounce UInt8`: `1` when `event_count <= 1`.
- `is_engaged UInt8`: `1` when `event_count > 1`.
- `converted UInt8`: `1` when the session contains a conversion event.
- `quality_score Float32`: normalized 0-100 score composed from depth, time,
  and conversion.

Keep `duration_seconds` as-is.

Keep `page_count` for compatibility, but from this change forward it should
mirror `pageview_count`. New code should read `pageview_count` or `event_count`
depending on intent.

## Session Quality

The first version uses a deterministic scoring formula:

```text
depth_score      = min(event_count / 10, 1) * 35
time_score       = min(duration_seconds / 300, 1) * 35
conversion_score = converted ? 30 : 0

quality_score = depth_score + time_score + conversion_score
```

Interpretation:

- shallow one-event sessions remain low quality
- longer sessions improve quality up to a 5-minute cap
- conversion can lift a short session because it is a strong success signal
- score remains bounded in `[0, 100]`

## Event Classification

Pageview events:

- `$pageview`
- `page_view`
- `$screen`
- `screen_view`

Conversion events for the MVP:

- `$conversion`
- `checkout_completed`
- `purchase`
- `signup_completed`
- `trial_started`

This list is intentionally hardcoded for 10.F.2. Project-configurable conversion
definitions can come later without changing the stored session columns.

## Components

- `clickhouse/init/86-product-events-aux.sql`: canonical fresh-install schema.
- `clickhouse/migrations/018-session-level-properties.sql`: additive migration
  for existing installations.
- `backend/src/storage/models.rs`: `ProductSessionRow` struct fields.
- `backend/src/workers/session_aggregator.rs`: aggregation query and row mapping.
- `backend/src/api/sessions.rs`: exposes session-level properties in
  `GET /api/v1/sessions`.
- `frontend/src/routes/sessions/+page.svelte`: shows engaged/bounce rates,
  average quality score, per-row quality score, and the derived session type.
- `backend/tests/workers_session_aggregator.rs`: integration coverage for the
  new session properties.

## Data Flow

1. SDKs send raw product events.
2. `session_aggregator` groups events into sessions using the existing 10.F.1
   rules.
3. For each session group, the query computes total events, pageview events,
   bounce/engaged booleans, conversion flag, duration, and score.
4. The worker inserts the enriched row into `product_sessions`.
5. `GET /api/v1/sessions` returns the properties with replay/error enrichment.
6. `/sessions` renders session quality, bounce/engaged state, counts, and
   navigation links.
7. `ReplacingMergeTree(ended_at)` keeps the latest version as in-flight sessions
   gain more events.

## Compatibility

The migration is additive and uses defaults, so older rows remain readable.

`page_count` stays present because existing frontend/API code may already
reference it. The worker will write it as `pageview_count` going forward. Any
new surface should prefer the explicit fields.

## Testing

Add focused integration assertions for:

- one-event sessions are bounce and not engaged
- sessions with more than one event are engaged
- total `event_count` and `pageview_count` differ when custom events occur
- conversion events set `converted = 1`
- `quality_score` increases with depth/time/conversion and stays at or below
  100

## Non-Goals

- Project-configurable conversion definitions.
- Complex ML or percentile-based quality models.
- Backfilling historical `product_sessions` beyond the normal worker lookback.
