# Feature Flag Error Rollback Alerts Design

Goal 10.G.3 links feature flag exposure with error tracking so Faro can recommend rollback when the treatment group gets materially worse.

## Recommendation

Use an automatic backend worker, not a user-authored alert rule. The worker scans recent `$feature_exposure` product events, links later product events carrying `trace_id` to `faro.error_events`, compares error rate per exposed user for variant `B` against variant `A`, and writes incidents into the existing `faro.alert_incidents` table.

This keeps product analytics and error tracking unified:

- Feature exposure comes from SDK product analytics.
- Backend errors come from `error_events`.
- `trace_id` is the bridge between product action and backend failure.
- The existing alert incident UI shows the rollback recommendation without a new storage model.

## Semantics

- Variant `A`: control, flag disabled.
- Variant `B`: treatment, flag enabled.
- Error rate: linked error events divided by exposed users in that variant.
- Fire condition:
  - both variants meet minimum sample size,
  - treatment has a minimum number of linked errors,
  - `treatment_error_rate / control_error_rate >= 5.0`.
- If control has zero errors and treatment meets the minimum error count, treat the ratio as infinite.
- Resolve condition: ratio drops below a lower cooldown threshold, default `2.0`, or samples disappear.

## Incident

Incident namespace:

`feature-rollback:<project_id>:<flag_key>`

Example note:

`Rollback recomendado: flag new-checkout tiene 5.8x más errores en variante B (B 29/8200 = 0.35%, A 5/8150 = 0.06%). Top servicio: checkout-api.`

## First-Version Limitations

- Requires product events with `trace_id` to link frontend/product activity to backend errors.
- Compares `A` vs `B` only.
- No automatic flag mutation. Faro recommends rollback but does not disable the flag.
