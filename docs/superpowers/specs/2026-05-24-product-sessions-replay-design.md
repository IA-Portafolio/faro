# Product Sessions Replay Design

## Goal

Build `/sessions` as a product-side session list that answers: "what recent user sessions happened, which ones had errors, and which can be replayed?"

This is not Faro dashboard auth sessions. It uses end-user product sessions from `faro.product_sessions` and replay chunks from `faro.session_replays`.

## Recommended Approach

Add a dedicated API endpoint, `GET /api/v1/sessions`, and a dedicated frontend route, `/sessions`.

Existing replay playback already exists at `/replays/[session_id]` and uses `GET /api/v1/replays/:session_id` to mount rrweb-player. `/sessions` should link into that player instead of duplicating playback code.

## Backend

Create `backend/src/api/sessions.rs`.

Route:

- `GET /api/v1/sessions`

Query params:

- Existing range params: `from`, `to`, `last_minutes`, `project`, `limit`.
- `distinct_id`: optional exact match.
- `has_replay`: optional boolean-ish string (`1`, `true`, `yes`).
- `has_error`: optional boolean-ish string (`1`, `true`, `yes`).

Response row:

```json
{
  "project_id": "default",
  "session_id": "sess_123",
  "distinct_id": "user_42",
  "started_at": "2026-05-24 12:00:00.000000000",
  "ended_at": "2026-05-24 12:05:00.000000000",
  "duration_seconds": 300,
  "pageview_count": 4,
  "event_count": 22,
  "error_count": 1,
  "has_error": 1,
  "has_replay": 1,
  "replay_event_count": 620,
  "replay_chunk_count": 3,
  "source": "web"
}
```

Error count comes from `faro.error_events` using session attributes. Support both `attributes['session.id']` and `attributes['session_id']` because the repo currently uses both spellings in different places.

Replay presence comes from `faro.session_replays` grouped by `session_id`.

## Frontend

Add `/sessions` with:

- Header and global time range picker.
- Filters: search by `distinct_id` or `session_id`, replay-only toggle, error-only toggle.
- Cards: total rows in view, sessions with replay, sessions with errors, total pageviews.
- Table columns: session, user, start/end, duration, pageviews, events, errors, replay.
- Row click opens `/replays/:session_id` only when `has_replay === 1`.
- Secondary links:
  - user profile: `/users/:distinct_id`
  - events filtered by session: `/events?query=session_id:<id>` using the existing events page convention.

Add `frontend/src/lib/sessions.ts` for pure helpers:

- `formatSessionDuration(seconds)`
- `sessionReplayHref(row)`
- `sessionEventsHref(row, project, range)`
- `sessionUserHref(row, project, range)`
- `sessionHealth(row)` returning `error`, `replay`, or `plain`.

## Navigation

Expose `/sessions` in:

- Sidebar near `/users`.
- Command palette as `nav.sessions`.

## Error Handling

Frontend shows inline API errors and keeps filters intact.

If there are no sessions, show the product-events onboarding empty state. If filters hide all rows, show the same empty state as filtered.

## Testing

Use TDD for frontend helper behavior and command palette navigation.

Backend unit tests cover boolean query parsing and session health math helpers. Full ClickHouse query behavior is integration-level and may not be runnable in this local environment if `cargo` is unavailable.
