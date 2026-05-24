# 10.F Session Tracking Design

## Context

Faro already stores product events in `faro.product_events` and has a
`faro.product_sessions` table intended to be maintained by a background worker.
The current implementation includes a `session_aggregator` worker, config flags,
and boot wiring. Goal 10.F.1 is to make the session layer reliable enough to be
treated as product behavior infrastructure:

- a session uses a 30-minute inactivity timeout;
- the worker runs every 5 minutes and computes sessions retroactively;
- if an SDK sends `session_id`, the backend trusts it.

The main gap in the current worker is actor identity. It only includes events
with a non-empty `distinct_id`, so anonymous pre-login sessions are dropped even
when the event has `anonymous_id`.

## Recommended Approach

Use the existing worker and table design, then harden the aggregation query.

The session actor is:

```text
actor_id = distinct_id if distinct_id != '' else anonymous_id
```

Events with neither `distinct_id` nor `anonymous_id` are ignored for session
tracking because they cannot be grouped into a stable user/device sequence.

Two aggregation branches remain:

1. Events with SDK-provided `session_id`
   - group by `(project_id, actor_id, session_id)`;
   - trust the provided session boundary;
   - compute `started_at`, `ended_at`, `page_count`, `duration_seconds`, and
     representative `source`.

2. Events without `session_id`
   - partition by `(project_id, actor_id)`;
   - sort by `timestamp`;
   - start a new session when previous timestamp is missing or the gap is
     greater than `FARO_SESSION_GAP_MINUTES`;
   - generate a synthetic id from `(project_id, actor_id, started_at)`.

`product_sessions.distinct_id` will store the effective actor id for this goal.
That preserves the existing schema and avoids a migration. For identified users
it remains the real `distinct_id`; for anonymous sessions it stores the
`anonymous_id`. A future schema can rename this to `actor_id` if product UI
needs to distinguish the two explicitly.

## Architecture

The feature remains backend/data-layer only.

- `backend/src/workers/session_aggregator.rs`
  owns sessionization, ClickHouse query construction, and upserts into
  `faro.product_sessions`.
- `backend/src/config.rs`
  keeps existing tunables:
  - `FARO_SESSION_AGGREGATOR_ENABLED=true`
  - `FARO_SESSION_AGGREGATOR_INTERVAL_SECS=300`
  - `FARO_SESSION_GAP_MINUTES=30`
  - `FARO_SESSION_LOOKBACK_MINUTES=360`
- `clickhouse/init/86-product-events-aux.sql` and
  `clickhouse/migrations/014-product-aux-tables.sql` keep the current
  `product_sessions` table.
- No frontend route is added for 10.F.1.

## Data Flow

1. SDKs or ingest clients write events into `faro.product_events`.
2. Every aggregation tick, the worker scans the configured lookback window.
3. The query derives `actor_id` from `distinct_id` or `anonymous_id`.
4. Rows with explicit `session_id` are grouped directly.
5. Rows without explicit `session_id` are split by inactivity gap.
6. The worker inserts the resulting rows into `faro.product_sessions`.
7. `ReplacingMergeTree(ended_at)` keeps the newest version when a live session
   is extended on a later tick.

## Error Handling

The worker remains best-effort:

- invalid or empty actor identity rows are ignored;
- ClickHouse query/insert errors are logged and retried on the next tick;
- config values are defensively clamped so an accidental tiny interval or
  invalid gap does not hammer ClickHouse;
- late-arriving events are handled within the lookback window, but sessions
  older than `FARO_SESSION_LOOKBACK_MINUTES` can no longer be repaired.

## Acceptance Criteria

- Events with the same actor and gaps under 30 minutes produce one session.
- Events with the same actor and a gap exactly equal to 30 minutes remain in
  the same session.
- Events with the same actor and a gap over 30 minutes produce separate
  sessions.
- Events with SDK-provided `session_id` keep that id and are not split by gap.
- Events with only `anonymous_id` are sessionized.
- Events with neither `distinct_id` nor `anonymous_id` are ignored.
- The worker remains idempotent across repeated runs over the same lookback.
- Existing environment defaults stay documented and unchanged.

## Testing

Add focused backend coverage around the sessionization behavior. Prefer an
integration test against ClickHouse if the existing test harness can seed
`product_events` and inspect `product_sessions`; otherwise extract the SQL
builder enough to unit-test query shape and run one integration smoke for the
worker path.

Minimum cases:

- explicit `session_id` is trusted;
- no `session_id`, two events 10 minutes apart become one synthetic session;
- no `session_id`, two events 31 minutes apart become two synthetic sessions;
- anonymous-only events are included using `anonymous_id`;
- no actor id events are excluded.

## Non-Goals

- No session analytics UI in this goal.
- No schema migration from `distinct_id` to `actor_id`.
- No cross-device merging of anonymous and identified sessions after aliasing;
  that belongs to product user unification and later profile views.
- No persistent watermark. The worker intentionally recomputes a sliding
  lookback window so late arrivals inside that window can repair sessions.
