# 10.F.3 Session Trace Linking Design

## Context

Product events already carry optional `trace_id` and `span_id`, and backend
traces live in `faro.spans`. This lets Faro answer a product-observability
question that external analytics tools usually cannot answer directly: which
backend traces served the user's session?

This is especially useful when a session ends in error, conversion drop-off, or
low quality. A user/session timeline can jump from product behavior to the
backend requests that supported that behavior.

## Decision

Materialize the session-to-trace relationship in `faro.product_sessions`.

Add:

- `trace_ids Array(String) DEFAULT []`
- `trace_count UInt32 DEFAULT 0`

`session_aggregator` computes these from the grouped product events:

```sql
groupUniqArrayIf(trace_id, trace_id != '') AS trace_ids
length(trace_ids) AS trace_count
```

This must happen during aggregation because synthetic sessions are derived by
the backend. Their raw `product_events` still have `session_id = ''`, so a later
query like `WHERE session_id = :id` cannot reconstruct the session membership.

## API

Add:

```http
GET /api/v1/sessions/:session_id/traces?project=<project_id>
```

Behavior:

1. Read `faro.product_sessions FINAL` by `(project_id, session_id)`.
2. If the session does not exist, return 404.
3. If `trace_ids` is empty, return `[]`.
4. Query `faro.spans` for those trace ids and return `TraceSummary[]` grouped by
   `trace_id`.

The response shape should reuse the existing trace summary contract:

- `trace_id`
- `timestamp`
- `service_name`
- `root_name`
- `duration_ns`
- `status_code`
- `span_count`

## Error Traces

Do not materialize `error_trace_count` in the worker for the MVP. Derive it in
read paths by checking returned trace summaries where `status_code = 'ERROR'`.

Rationale: product session aggregation should stay independent from trace
ingestion timing. A product event may arrive before the backend trace is fully
ingested; deriving error state at read time avoids stale denormalized flags.

## Components

- `clickhouse/init/86-product-events-aux.sql`: fresh-install session schema.
- `clickhouse/migrations/019-session-trace-linking.sql`: additive migration for
  existing installs.
- `backend/src/storage/models.rs`: `ProductSessionRow` trace fields.
- `backend/src/workers/session_aggregator.rs`: aggregate trace ids/count.
- `backend/src/api/sessions.rs`: new session traces endpoint.
- `backend/src/api/mod.rs`: mount the sessions API.
- `frontend/src/lib/api.ts`: `fetchProductSessionTraces` helper.
- `frontend/src/lib/sessions.ts`: `sessionTracesHref` helper.
- `frontend/src/routes/sessions/+page.svelte`: shows `trace_count` links in
  session rows.
- `frontend/src/routes/sessions/[session_id]/traces/+page.svelte`: navigable
  trace summaries for one product session.
- `backend/tests/workers_session_aggregator.rs`: aggregation coverage.
- `backend/tests/api_session_traces.rs`: endpoint coverage.

## Data Flow

1. SDK sends product events with optional `trace_id`.
2. `session_aggregator` groups events into sessions.
3. For each session group, it stores unique non-empty trace ids and their count.
4. UI/API requests session traces.
5. Backend reads trace ids from `product_sessions` and summarizes matching spans.
6. `/sessions` shows "N traces" when `trace_count > 0`.
7. `/sessions/:session_id/traces?project=<project>` lists the backend trace
   summaries and lets the user open each trace detail.
8. UI can highlight sessions/traces whose returned summaries include backend
   errors.

## Compatibility

The migration is additive. Existing sessions get empty `trace_ids` and
`trace_count = 0` until the aggregator rewrites them during the lookback window.

This design works for both SDK-provided sessions and backend-derived synthetic
sessions.

## Testing

Add integration coverage for:

- sessions with events that carry multiple `trace_id`s store unique trace ids
  and `trace_count`
- events without `trace_id` do not affect `trace_count`
- synthetic sessions also store trace ids
- `GET /sessions/:session_id/traces` returns summarized spans for stored trace ids
- missing sessions return 404
- sessions with no trace ids return an empty list

## Non-Goals

- Worker-side `error_trace_count`.
- Joining product events to spans during session aggregation.
- Trace event counts per session in the MVP.
