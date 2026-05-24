# Latency Funnel Impact Design

## Goal

Add an insight that answers:

> Which slow backend traces correlate with funnel conversion drop-off?

The headline output should support copy like:

> Cuando `/api/checkout` p95 supera 2s, el funnel checkout cae 12 puntos.

This is the counterpart to revenue-impact errors: it turns backend latency into product impact so teams can prioritize performance work by conversion loss, not just p95 charts.

## Scope

This iteration ships a backend API endpoint only. It reuses existing ClickHouse tables:

- `faro.spans`
- `faro.product_events`

No schema migration or materialized view is required.

## API

Route:

```http
GET /api/v1/insights/latency-funnel-impact
```

Query parameters:

- Common range params from `api::params::Range`: `from`, `to`, `last_minutes`, `limit`, `project`.
- `span_name`: required span/route name to analyze, for example `/api/checkout`.
- `service`: optional service filter for spans.
- `funnel_from`: optional product event that starts the funnel step. Default: `checkout_started`.
- `funnel_to`: optional product event that completes the funnel step. Default: `checkout_completed`.
- `latency_threshold_ms`: optional threshold for slow buckets. Default: `2000`.
- `bucket_minutes`: optional time bucket size. Default: `60`; clamp to a practical range.

Response:

```json
{
  "span_name": "/api/checkout",
  "service_name": "checkout-api",
  "funnel_from": "checkout_started",
  "funnel_to": "checkout_completed",
  "bucket_minutes": 60,
  "p95_threshold_ms": 2000,
  "slow_bucket_count": 6,
  "baseline_bucket_count": 18,
  "baseline_conversion_rate": 0.71,
  "slow_conversion_rate": 0.59,
  "conversion_drop_points": 12.0,
  "summary": "Cuando /api/checkout p95 supera 2s, el funnel checkout cae 12 puntos.",
  "buckets": [
    {
      "bucket_start": "2026-05-24 14:00:00",
      "p95_latency_ms": 2340.5,
      "funnel_started": 1200,
      "funnel_completed": 690,
      "conversion_rate": 0.575,
      "slow": true
    }
  ]
}
```

## Semantics

The endpoint groups both traces and product events into aligned time buckets.

For each bucket:

- `p95_latency_ms` is `quantileExact(0.95)(duration_ns) / 1_000_000` for spans matching `span_name`, optional `service`, project, and range.
- `funnel_started` is the count of distinct product users/sessions that fired `funnel_from`.
- `funnel_completed` is the count of distinct product users/sessions that fired `funnel_to` in the same bucket.
- `conversion_rate` is `funnel_completed / funnel_started`, with `0.0` if no one started.
- `slow` is true when `p95_latency_ms >= latency_threshold_ms`.

The aggregate baseline conversion rate is computed across non-slow buckets with funnel starts:

```text
sum(non_slow.completed) / sum(non_slow.started)
```

The slow conversion rate is computed across slow buckets with funnel starts:

```text
sum(slow.completed) / sum(slow.started)
```

The drop in points is:

```text
max(baseline_conversion_rate - slow_conversion_rate, 0) * 100
```

This first version correlates by time bucket, not by individual `trace_id`. That is intentional: `product_events.trace_id` exists, but not all browser/product events can guarantee trace propagation. Time-bucket correlation gives a reliable enterprise insight using the data Faro already stores.

## Architecture

Extend `backend/src/api/insights.rs` with:

- Route: `/insights/latency-funnel-impact`
- `LatencyFunnelImpactQuery`
- `LatencyFunnelImpactResult`
- `LatencyFunnelBucket`
- `latency_funnel_impact` handler
- Pure helpers for conversion rate, percentage-point drop, and summary text

The handler follows the same patterns as `revenue_impact`:

- Resolve ranges with `Range::resolve`.
- Format ClickHouse dates with `ch_dt`.
- Bind all user input through `select_with_params`.
- Validate required/positive params before querying.
- Keep business math in Rust where it is easy to unit test.

## Data Flow

1. Validate `span_name`, funnel event names, threshold, bucket size, and time range.
2. Query span p95 by bucket from `faro.spans`.
3. Query funnel counts by bucket from `faro.product_events`.
4. Merge rows by bucket in Rust.
5. Split buckets into slow and baseline groups.
6. Compute aggregate conversion rates, drop points, and summary.
7. Return the top `limit` buckets ordered by bucket time, plus aggregate headline metrics.

The implementation may use two ClickHouse queries rather than one deeply nested query. That keeps SQL readable and avoids coupling span aggregation to funnel aggregation.

## Error Handling

- Empty `span_name` returns `400`.
- Empty `funnel_from` or `funnel_to` returns `400`.
- Equal `funnel_from` and `funnel_to` returns `400`.
- Non-positive `latency_threshold_ms` returns `400`.
- Non-positive `bucket_minutes` returns `400`.
- Invalid time range returns `400`.
- If there are no matching spans or funnel starts, return a valid response with zeroed aggregate metrics and an empty or sparse bucket list.

## Testing

Add unit tests for pure helpers:

- Conversion rate is completed over started.
- Conversion rate is zero when started is zero.
- Drop points clamp negative deltas to zero.
- Summary text formats threshold seconds and rounded drop points.

Add an integration test with seeded ClickHouse data:

- Four hourly buckets for `/api/checkout`.
- Two non-slow buckets below 2s p95 with strong checkout conversion.
- Two slow buckets above 2s p95 with weaker checkout conversion.
- Assert the response says baseline conversion is higher, slow conversion is lower, and drop points match the seeded data.

## Future Work

- Add trace-level evidence when `product_events.trace_id` is consistently populated.
- Add `lost_revenue` by combining this endpoint with average order value.
- Add service/route ranking mode when `span_name` is omitted, returning the slow routes with the largest correlated drop.
- Add frontend copy and charts that show p95 latency and funnel conversion on the same timeline.
