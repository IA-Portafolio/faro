# 10.E.2 User Properties Enrichment

## Goal

`faro.identify("user_42", { "plan": "pro", "signup_date": "...", "industry": "fintech" })`
must enrich the stable row for `user_42` in `faro.product_users`. Cohort queries must then be
able to answer questions like "all pro users in fintech" without adding typed columns for every
new trait. User properties remain JSON stored in the existing `product_users.properties` column.

## Current Context

Product events already persist `user_properties` as JSON text on each `$identify` event. The
`user_unifier` worker aggregates events into `faro.product_users` and currently copies the latest
non-empty `user_properties` payload into `product_users.properties`.

Cohorts currently evaluate behavior from `faro.product_events`; `CohortDefinition.filters` applies
to event `properties`, not user properties. That distinction should stay intact so existing cohorts
keep their meaning.

## Design

Add `user_filters` to `CohortDefinition` while keeping `filters` unchanged.

- `filters`: event-level JSON predicates, evaluated against `product_events.properties`.
- `user_filters`: user-level JSON predicates, evaluated against `product_users.properties`.

The worker will merge identify traits into the existing user JSON instead of replacing the whole
object whenever a later identify sends a partial update. New keys and changed values from the latest
identify win. Existing keys not present in the latest identify are preserved. Empty or invalid JSON
payloads do not erase existing properties.

The cohort query remains anchored in `product_events` for behavior:

1. Filter events by `event_name`, `project_id`, `timestamp`, and event `filters`.
2. If `user_filters` is non-empty, join `faro.product_users FINAL` by `(project_id, distinct_id)`.
3. Apply each user filter with `JSONExtractString(u.properties, key) = value`.
4. Group by `distinct_id` and apply the count comparator.

This supports a cohort such as:

```json
{
  "event": "checkout_completed",
  "op": ">=",
  "count": 1,
  "last_days": 30,
  "filters": [],
  "user_filters": [
    { "key": "plan", "value": "pro" },
    { "key": "industry", "value": "fintech" }
  ]
}
```

## Validation And Limits

`user_filters` uses the same shape and limits as `filters`: non-empty key and value, key length up
to 128, value length up to 256. The total filter cap remains small to protect ClickHouse scans; the
implementation should count event filters and user filters together against the existing max of 3.

All SQL values remain parameterized. Only the validated comparison operator is interpolated, as it is
today.

## Data Compatibility

No ClickHouse migration is required for the storage layer because `faro.product_users.properties`
already exists as JSON text. Existing cohort definitions without `user_filters` deserialize with an
empty array via `#[serde(default)]`, so old saved cohorts remain valid.

Rows already present in `product_users` keep their current properties. They are enriched the next
time the worker sees a `$identify` event in its scan window.

## Testing

Add unit tests first for:

- Merging identify traits preserves old keys and lets new values win.
- Empty or invalid latest user properties do not wipe existing properties.
- Cohort validation counts `filters + user_filters` against the max.
- Cohort SQL without `user_filters` keeps the current shape.
- Cohort SQL with `user_filters` joins `product_users FINAL`, filters `u.properties`, and keeps user
  values out of the SQL string.

No frontend changes are required for this goal unless the UI already exposes cohort filters and needs
to surface `user_filters`.
