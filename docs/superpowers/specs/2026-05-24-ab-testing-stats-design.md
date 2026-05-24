# A/B Testing Stats Design

## Goal

Turn feature flags into measurable experiments: when `new-checkout` is rolled out to 50%, Faro can show conversion by variant, lift, p-value, sample size, and a 95% confidence interval.

## Recommended Cut

Use `faro.product_events` as the source of truth instead of adding a new experiment event store.

SDKs emit one `$feature_exposure` event per `(flag_key, distinct_id, variant)` when `isFeatureEnabled()` is evaluated. Conversion events remain normal product events such as `checkout_completed`. The backend joins exposures to later conversion events by `distinct_id` and computes a two-proportion z-test.

## Event Shape

Exposure event:

```json
{
  "type": "track",
  "name": "$feature_exposure",
  "distinct_id": "user_42",
  "properties": {
    "flag_key": "new-checkout",
    "variant": "B",
    "enabled": true
  }
}
```

Variant convention:

- `A`: disabled/control.
- `B`: enabled/treatment.

## Backend API

`POST /api/v1/experiments/analyze`

Request:

```json
{
  "project": "default",
  "flag_key": "new-checkout",
  "conversion_event": "checkout_completed",
  "last_minutes": 10080
}
```

Response includes one row per variant, overall sample, lift of B over A, p-value, and 95% confidence interval for the absolute conversion-rate delta.

The first implementation compares `A` and `B` only. Multi-variant experiments can extend the same exposure event shape later.

## Statistics

For each variant:

- sample = unique exposed users.
- conversions = unique exposed users who fired the conversion event after their first exposure in the selected range.
- conversion_rate = conversions / sample.

For `B - A`:

- absolute_delta = rate_B - rate_A.
- relative_lift = absolute_delta / rate_A.
- standard error = `sqrt(pA(1-pA)/nA + pB(1-pB)/nB)`.
- 95% CI = `absolute_delta ± 1.96 * standard_error`.
- p-value = two-sided normal approximation for the z statistic.

If either sample is zero, stats return neutral zeros/null-equivalent numeric values instead of failing.

## UI

Add `/experiments`: a work-focused page with inputs for flag key, conversion event, time range, and project. The result headline reads like:

`Variante B convierte 4.2% mejor (p=0.03, sample=8200, 95% CI: 1.1% - 7.3%)`

Below it, show A/B cards with sample, conversions, and conversion rate.

## Tests

- SDK tests assert exposure is emitted once per user/flag/variant and conversion events still flow normally.
- Backend unit tests cover z-test/lift/CI math.
- Frontend compile/build verifies the new route and API types.
