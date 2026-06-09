# Combined Insights Dashboard Design

## Goal

Build `/insights` as the product differentiator view: one panel that ties product events,
errors, traces, and latency together around a user journey such as checkout.

## Recommendation

Use a single backend aggregate endpoint for the first version:

`GET /api/v1/insights/service-dashboard`

The UI can still link out to `/events`, `/errors`, `/traces`, and `/sessions`, but the
headline panel should load from one API call so the numbers are internally consistent.

## Default Lens

- service: `checkout`
- funnel_from: `checkout_started`
- funnel_to: `checkout_completed`
- span_name: `/api/checkout`
- range: current global range, defaulting to 24h in the UI

## Response Shape

- event counts for started/completed and conversion rate
- session counts for started/completed/failed checkout sessions
- errors linked to failed checkout sessions by `session_id`
- top linked issues with fingerprint, message, affected sessions, and direct error links
- p95 latency for the selected span/service
- narrative sentence suitable for an executive dashboard card

## Linking Model

The key join is `session_id`:

- product events define failed checkout sessions: had `funnel_from`, did not have `funnel_to`
- error events join through `attributes['session.id']` or `attributes['session_id']`
- spans provide p95 latency by `service_name` + `name`

Trace-level links remain available through session trace linking and existing trace detail pages.

## UX

The first viewport should be the dashboard itself, not a landing page. It should show:

- service lens and controls
- one narrative panel
- three compact metric groups: Events, Errors, Latency
- top linked issues table
- small outbound links to underlying pillar pages
