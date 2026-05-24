# Feature Flags Design

## Goal

Make feature flags a first-class Faro primitive backed by ClickHouse and usable from the JavaScript SDK with:

```ts
if (faro.isFeatureEnabled('new-checkout', { distinct_id: 'user_42', properties: { plan: 'pro' } })) {
  // render new checkout
}
```

## Architecture

Faro stores flag definitions in `faro.feature_flags`. The backend loads active rows into an in-memory cache at boot and refreshes every 30 seconds. SDKs fetch the active flag list for their project via a Bearer-token endpoint and evaluate flags locally from that cached list.

This deliberately treats flag rules as non-secret. A browser SDK can see flag keys, rollout percentages, and property conditions. Secrets or server-only targeting should not be encoded in feature flag rules.

## Data Model

`faro.feature_flags` stores:

- `project_id`: project slug.
- `key`: stable flag key such as `new-checkout`.
- `rollout_percentage`: integer percentage clamped by the backend/SDK to 0..100.
- `conditions`: JSON string. The first supported shape is `{"properties":{"plan":"pro"}}`, meaning all listed property values must match exactly.
- `active`: 1 means served to SDKs; 0 means hidden from SDK payloads.
- `updated_at` and `version`: operational columns for `ReplacingMergeTree`.

The primary identity is `(project_id, key)`.

## API

The SDK endpoint is public in the same sense as ingest endpoints:

`GET /api/v1/ingest/feature-flags`

Authentication uses the existing project Bearer token. The response contains only active flags for the resolved project:

```json
{
  "project": "default",
  "flags": [
    {
      "key": "new-checkout",
      "rollout_percentage": 10,
      "conditions": {"properties":{"plan":"pro"}}
    }
  ]
}
```

Dashboard CRUD is intentionally out of this first slice; flags can be inserted by migration, ClickHouse console, or later admin endpoints/UI.

## SDK Behavior

The JS SDK adds:

- `isFeatureEnabled(key, context)`: synchronous evaluation from the local cache.
- `refreshFeatureFlags()`: explicit async fetch.
- Automatic refresh every 30 seconds by default.

`context.distinct_id` drives sticky rollout. If it is omitted, the SDK falls back to its current `distinctId` or `anonymousId`, so pre-login users remain sticky too.

Evaluation order:

1. Missing/inactive flag returns `false`.
2. Property conditions must all match exactly against `context.properties`.
3. Rollout `0` returns `false`, `100` returns `true`.
4. Intermediate rollout hashes `project/key/distinct_id` into bucket `0..99` and enables when `bucket < rollout_percentage`.

## Error Handling

SDK fetch failures keep the previous cache and log through `diag`; no user request should block on flag refresh. Backend cache reload failures keep the previous cache and warn via tracing.

## Testing

Backend tests cover token-scoped flag fetch and inactive flag filtering. SDK tests cover local property conditions, sticky rollout determinism, and refresh failure preserving the prior cache.
