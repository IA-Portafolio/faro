# Feature Flags, Experimentos y Rollback Recomendado

Faro trata los feature flags como parte del flujo de observabilidad de producto:
un flag no solo decide UI, también crea la señal que permite medir conversión y
errores por variante.

## SDK

Los **7 SDKs** (Node, Next.js, Expo, Python, Go, Kotlin, Flutter) mantienen una
cache local de flags y la refrescan cada 30 segundos. La evaluación es local
(sin red por evaluación) y sticky por `distinct_id` — el mismo usuario obtiene
la misma variante en cualquier plataforma. Hasta el primer refresh los flags
evalúan a `false`; `refreshFeatureFlags()` fuerza un refresh inmediato. La firma
por lenguaje está en la [tabla de SDKs](../sdks/README.md#feature-flags-y-experimentos).

```ts
if (faro.isFeatureEnabled('new-checkout', {
  distinct_id: 'user_42',
  properties: { plan: 'pro' },
})) {
  // render new checkout
}
```

Cada evaluación que entra al targeting emite una vez un evento especial:

```json
{
  "name": "$feature_exposure",
  "properties": {
    "flag_key": "new-checkout",
    "variant": "B",
    "enabled": true
  }
}
```

Convención:

- `A`: control, flag apagado.
- `B`: treatment, flag encendido.
- Usuarios que no cumplen `conditions` no entran al experimento.

## A/B Testing

La pantalla `/experiments` usa `POST /api/v1/experiments/analyze` para comparar
la exposición al flag contra un evento de conversión, por ejemplo
`checkout_completed`.

El backend calcula:

- sample por variante,
- conversiones por variante,
- conversion rate,
- lift relativo de B vs A,
- p-value frequentist para dos proporciones,
- intervalo de confianza 95%.

Ejemplo de lectura:

`Variante B convierte 4.2% mejor (p=0.030, sample=8200, 95% CI: 1.1% - 7.3%)`

## Rollback Recomendado por Errores

El worker de rollback recomendado cruza:

1. `$feature_exposure` en `faro.product_events`,
2. eventos de producto posteriores con `trace_id`,
3. errores backend en `faro.error_events` con el mismo `trace_id`.

Si la variante B tiene una tasa de errores al menos 5x mayor que A, abre un
incidente `feature-rollback:<project>:<flag>` en `faro.alert_incidents`.

Importante:

- Faro recomienda rollback, pero no desactiva el flag automáticamente.
- Para que el enlace product analytics + error tracking funcione, los eventos de
  producto relevantes deben traer `trace_id`.
- El detector compara tasas por usuario expuesto, no conteos crudos.

Variables principales:

- `FARO_FEATURE_ROLLBACK_ENABLED=true`
- `FARO_FEATURE_ROLLBACK_RATIO=5.0`
- `FARO_FEATURE_ROLLBACK_RESOLVE_RATIO=2.0`
- `FARO_FEATURE_ROLLBACK_MIN_SAMPLE=20`
- `FARO_FEATURE_ROLLBACK_MIN_TREATMENT_ERRORS=5`

La referencia completa de variables vive en
[`docs/reference/environment.md`](reference/environment.md).
