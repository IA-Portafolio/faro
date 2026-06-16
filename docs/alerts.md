# Alertas

Las alertas de Faro evalúan queries declarativas de ClickHouse contra un
umbral y disparan/resuelven incidentes automáticamente, con notificaciones
multicanal.

## Reglas

Una regla (`AlertRule`) define qué medir, cuándo disparar y a quién avisar.

### CRUD

Endpoints (bajo `/api/v1`, requieren admin):

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/alerts/rules` | Lista (soporta `?project=`) |
| `POST` | `/alerts/rules` | Crear |
| `GET` | `/alerts/rules/:id` | Detalle |
| `PUT` | `/alerts/rules/:id` | Editar |
| `DELETE` | `/alerts/rules/:id` | Soft-delete |

Body de create/update:

```json
{
  "name": "Tasa de errores alta",
  "project": "default",
  "description": "Más de 25 errores en 5 minutos",
  "source": "logs",
  "query": "(SELECT countIf(severity_number >= 17) FROM faro.logs WHERE timestamp > now() - INTERVAL :window_seconds SECOND)",
  "condition": "gt",
  "threshold": 25,
  "window_seconds": 300,
  "interval_seconds": 60,
  "severity": "error",
  "notification_targets": ["channel://ops-pagerduty"],
  "enabled": 1
}
```

### Campos

| Campo | Tipo | Default | Descripción |
| ----- | ---- | ------- | ----------- |
| `name` | `string` | — | Nombre visible. |
| `project` | `string` | `default` | Slug del proyecto. |
| `description` | `string` | `""` | Descripción humana. |
| `source` | `string` | — | Origen de la señal: `logs`, `spans`, `monitors`, `metrics`. |
| `query` | `string` | — | SQL de ClickHouse que devuelve un escalar. Debe usar `:window_seconds` como placeholder del ventana. Se valida contra SSRF/RCE (table-functions prohibidas). |
| `condition` | `string` | — | Comparador contra `threshold`: `gt`, `gte`, `lt`, `lte`, `eq`. |
| `threshold` | `f64` | — | Umbral numérico. |
| `window_seconds` | `u32` | `300` | Ventana de evaluación en segundos. |
| `interval_seconds` | `u32` | `60` | Cadencia de evaluación en segundos. |
| `severity` | `string` | `warn` | Nivel del incidente: `info`, `warn`, `error`, `critical`. |
| `notification_targets` | `string[]` | `[]` | Destinos de notificación. Ver [Canales](#destinos-de-notificación). |
| `enabled` | `u8` | `1` | `1` = activa, `0` = pausada. |

### Destinos de notificación

Cada string en `notification_targets` puede ser:

- **URL directa** — POST JSON a la URL (`https://discord.com/api/webhooks/...`)
- **`tg://...`** — Alias legacy de Telegram
- **`channel://<id>`** — Canal configurable (ver [canales](../README.md#canales-de-notificación))

### Condiciones soportadas

| Valor | Semántica |
| ----- | --------- |
| `gt` | `value > threshold` |
| `gte` | `value >= threshold` |
| `lt` | `value < threshold` |
| `lte` | `value <= threshold` |
| `eq` | `value == threshold` |

### Niveles de severity

`info`, `warn`, `error`, `critical`. El severity se propaga al incidente y al
payload de notificación.

## Incidentes

El worker `alert_evaluator` corre cada 15s, evalúa cada regla activa en su
`interval_seconds`, y abre/resuelve incidentes en `faro.alert_incidents`.

### Endpoint

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/alerts/incidents` | Lista incidentes recientes (soporta `?from=&to=&project=`) |

### Estructura del incidente (`AlertIncident`)

```json
{
  "id": "uuid",
  "rule_id": "uuid",
  "rule_name": "Tasa de errores alta",
  "started_at": "2026-06-16T10:00:00Z",
  "resolved_at": null,
  "value": 32.0,
  "threshold": 25.0,
  "severity": "error",
  "status": "firing",
  "note": ""
}
```

| Campo | Descripción |
| ----- | ----------- |
| `status` | `firing` (activo) o `resolved` (auto-resuelto cuando la condición deja de cumplirse). |
| `value` | Último valor medido por el evaluador. |
| `threshold` | Umbral de la regla que disparó. |
| `resolved_at` | `null` mientras `status = firing`. |

## Ejemplos de queries

**Pico de tasa de errores** (`source: logs`):

```sql
(SELECT countIf(severity_number >= 17) FROM faro.logs
 WHERE timestamp > now() - INTERVAL :window_seconds SECOND)
```

**Latencia p95** (`source: spans`):

```sql
(SELECT toFloat64(quantile(0.95)(duration_ns))/1e6 FROM faro.spans
 WHERE service_name='api' AND timestamp > now() - INTERVAL :window_seconds SECOND)
```

**Uptime de monitor** (`source: monitors`):

```sql
(SELECT sum(success)/count()*100 FROM faro.monitor_results
 WHERE monitor_id = 'YOUR-UUID' AND timestamp > now() - INTERVAL :window_seconds SECOND)
```

## Anomalías automáticas

El worker `anomaly_detector` corre en paralelo al `alert_evaluator` y detecta
spikes anómalos en latencia p95 de spans usando z-score sobre una ventana
móvil:

- **Threshold de disparo**: `mean + z_fire * stddev` (z_fire configurable,
  default 3).
- **Severity**: si `z >= 5.0` el incidente se marca `critical`; si no, `warn`.
- No requiere configuración de reglas — corre automáticamente sobre los
  servicios que reportan spans.

Los incidentes generados aparecen en el mismo endpoint
`GET /api/v1/alerts/incidents` que los de reglas manuales.

## Seguridad

El `query` es SQL crudo de ClickHouse — flexible pero potente. El backend
valida contra table-functions peligrosas (`url()`, `file()`, `s3()`, etc.)
antes de persistir la regla. Aun así, trata las reglas como **solo-admin**:
un query malicioso puede saturar ClickHouse o leer tablas internas.
