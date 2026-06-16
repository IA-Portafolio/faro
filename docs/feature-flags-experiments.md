# Feature Flags, Experimentos y Rollback Recomendado

Faro trata los feature flags como parte del flujo de observabilidad de producto:
un flag no solo decide UI, también crea la señal que permite medir conversión y
errores por variante.

## Crear y configurar flags

Los flags viven en la tabla `faro.feature_flags` de ClickHouse. Hoy no hay
endpoint REST de escritura — se crean/modifican con un `INSERT` directo contra
ClickHouse:

```sql
INSERT INTO faro.feature_flags
    (project_id, key, rollout_percentage, conditions, active, updated_at, version)
VALUES
    ('default', 'new-checkout', 50, '{"properties":{"plan":"pro"}}', 1, now64(3), 1);
```

Campos:

| Campo | Tipo | Descripción |
| ----- | ---- | ----------- |
| `project_id` | `LowCardinality(String)` | Slug del proyecto. Default `'default'`. |
| `key` | `String` | Identificador estable del flag (p. ej. `new-checkout`). |
| `rollout_percentage` | `UInt8` | Porcentaje de usuarios expuestos (0–100). El SDK hace sticky bucketing por `distinct_id` (FNV-1a). |
| `conditions` | `String` | JSON con reglas de targeting. Shape actual: `{"properties":{"plan":"pro"}}` — todas las properties listadas deben coincidir exactamente en el contexto del SDK. |
| `active` | `UInt8` | `1` = servido a SDKs; `0` = oculto del payload. |
| `updated_at` | `DateTime64(3,'UTC')` | Timestamp del último cambio (`now64(3)` al insertar). |
| `version` | `UInt64` | Versión para `ReplacingMergeTree`. Bumpeala en cada upsert para que el `FINAL` del backend elija la fila más nueva. |

La PK es `(project_id, key)`. El backend carga los flags activos en un cache
in-memory al boot y refresca cada 30s; un `INSERT` nuevo tarda hasta 30s en
verse reflejado en el próximo refresh de los SDKs.

> **Seguridad:** las `conditions` se sirven a los SDKs (incluido el browser).
> No pongas secretos ni reglas server-only en `conditions` — el flag es público
> por diseño.

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
