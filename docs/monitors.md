# Monitores de API

Los monitores hacen checks HTTP sintéticos a intervalos configurables y
registran uptime, latencia y errores de conectividad. Son la base para SLOs y
alertas de disponibilidad.

## CRUD

Endpoints (bajo `/api/v1`, requieren admin):

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/monitors` | Lista (soporta `?project=`) |
| `POST` | `/monitors` | Crear |
| `GET` | `/monitors/:id` | Detalle |
| `PUT` | `/monitors/:id` | Editar |
| `DELETE` | `/monitors/:id` | Soft-delete |
| `GET` | `/monitors/:id/results` | Últimos resultados de checks (soporta `?from=&to=`) |
| `GET` | `/monitors/:id/uptime` | Agregados de uptime y latencia (soporta `?from=&to=`) |

Body de create/update:

```json
{
  "name": "Checkout API",
  "project": "default",
  "method": "GET",
  "url": "https://api.example.com/health",
  "headers": { "Authorization": "Bearer ..." },
  "body": "",
  "interval_seconds": 60,
  "timeout_seconds": 30,
  "expected_status_min": 200,
  "expected_status_max": 299,
  "expected_body_regex": "",
  "enabled": 1
}
```

## Campos

| Campo | Tipo | Default | Descripción |
| ----- | ---- | ------- | ----------- |
| `name` | `string` | — | Nombre visible. |
| `project` | `string` | `default` | Slug del proyecto. |
| `method` | `string` | — | Método HTTP: `GET`, `POST`, `PUT`, `DELETE`, etc. |
| `url` | `string` | — | URL a chequear. Validada contra SSRF (se rechazan IPs internas/loopback salvo `localhost`). |
| `headers` | `map` | `{}` | Headers HTTP del request. |
| `body` | `string` | `""` | Body del request (para POST/PUT). |
| `interval_seconds` | `u32` | `60` | Cadencia entre checks. |
| `timeout_seconds` | `u32` | `30` | Timeout del request. |
| `expected_status_min` | `u16` | `200` | Status code mínimo aceptado como success. |
| `expected_status_max` | `u16` | `299` | Status code máximo aceptado como success. |
| `expected_body_regex` | `string` | `""` | Regex que debe matchear el body. Vacío = no valida body. |
| `enabled` | `u8` | `1` | `1` = activo, `0` = pausado. |

## Resultados (`MonitorResult`)

Cada check genera una fila en `faro.monitor_results`:

```json
{
  "monitor_id": "uuid",
  "timestamp": "2026-06-16T10:00:00Z",
  "success": 1,
  "status_code": 200,
  "duration_ms": 127,
  "error_message": "",
  "response_size": 4096
}
```

## Uptime

`GET /api/v1/monitors/:id/uptime` devuelve agregados del rango consultado:

```json
{
  "total": 1440,
  "success": 1435,
  "uptime_pct": 99.65,
  "avg_duration_ms": 132.5,
  "p95_duration_ms": 287.0
}
```

## Cómo funciona

El worker `monitor_runner` lee `faro.api_monitors` cada 10s y agenda cada
monitor en su propio `interval_seconds`. Cada check:

1. Hace el request HTTP con `method`, `url`, `headers`, `body` y `timeout_seconds`.
2. Verifica `status_code` contra `[expected_status_min, expected_status_max]`.
3. Si `expected_body_regex` no está vacío, verifica que el body matchee.
4. Registra el resultado en `faro.monitor_results`.

Los resultados alimentan las reglas de alerta con `source: "monitors"` (ver
[alerts.md](alerts.md)).

## Seguridad de URL

La URL del monitor se valida antes de persistir (`validate_monitor_url`).
Se rechazan:

- IPs de loopback/link-local/privadas (salvo `localhost` explícito para dev)
- URLs sin esquema `http`/`https`
- Redirects a hosts internos (no se sigue la cadena de redirects)

Esto previene que un monitor configurado maliciosamente use el backend como
proxy para escanear la red interna (SSRF).
