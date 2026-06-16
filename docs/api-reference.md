# Referencia de la API REST

Todos los endpoints están bajo `/api/v1` en el puerto `:8080`. Los endpoints de
lectura aceptan `?from=`, `?to=` (ISO-8601), `?project=` y `?limit=` como
parámetros de rango comunes.

## Autenticación

- **Endpoints de lectura/escritura del dashboard**: cookie de sesión
  `faro_session` (login vía `/auth/login`). Los endpoints de admin requieren
  rol `admin`.
- **Endpoints de ingesta**: token por proyecto vía `Authorization: Bearer`,
  `x-faro-token` o `?_token=` (ver [README](../README.md#autenticación-de-ingesta)).

## Logs

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/logs` | Lista filtrable |
| `GET` | `/logs/live` | Stream SSE en vivo |
| `GET` | `/logs/stats` | Volumen por bucket y severidad |

Query params de `/logs`: `service`, `min_severity` (número OTel), `query`
(substring case-insensitive sobre `body`), `trace_id`, `regex` (interpretar
`query` como regex — solo live tail), `limit` (default 200).

## Events (product analytics)

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/events` | Lista filtrable |
| `GET` | `/events/live` | Stream SSE en vivo |
| `GET` | `/events/stats` | Volumen por bucket |

Query params de `/events`: `event_name`, `distinct_id`, `anonymous_id`,
`session_id`, `trace_id`, `source`, `query` (substring sobre JSON de
properties), `prop` (pares `key:value` repetibles, máx 5, para
`JSONExtractString(properties, key) = value`).

## Traces

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/traces` | Lista de trazas (reagregada desde spans) |
| `GET` | `/traces/:trace_id` | Todos los spans de una traza |

Query params de `/traces`: `service`, `status` (`OK`/`ERROR`/`UNSET`),
`min_duration_ms`.

## Services

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/services` | Lista con conteos de logs y errores |
| `GET` | `/services/map` | Grafo de dependencias (service map) |

`/services` devuelve `[{ service_name, log_count, error_count, last_seen }]`.
`/services/map` devuelve `{ nodes: [{ service, calls, errors, p95_ms, is_root }], edges: [{ source, target, calls, errors }] }`.

## Metrics

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/metrics/series` | Series temporales de una métrica |
| `GET` | `/metrics/names` | Catálogo de métricas disponibles |

Query params de `/metrics/series`: `name` (requerido), `service`,
`bucket_seconds` (default 60), `agg` (`avg`/`sum`/`max`/`min`/`count`).

Las métricas con prefijo `events.` y sufijo `.count` son **virtuales**: se
derivan de `faro.product_events` al vuelo (p. ej. `events.checkout_completed.count`).

## Errors / Issues

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/errors` | Issues agrupados por fingerprint |
| `GET` | `/errors/:fingerprint` | Issue + eventos recientes |
| `POST` | `/errors/:fingerprint/status` | Cambiar estado |
| `GET` | `/errors/:fingerprint/sessions` | Sesiones donde ocurrió |

Query params de `/errors`: `service`, `status` (`unresolved`/`resolved`/`ignored`).

Body de POST status: `{ "status": "resolved", "assignee": "", "note": "", "service_name": "api" }`.

## Sessions

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/sessions` | Lista de sesiones (duración, errores, replay) |
| `GET` | `/sessions/:session_id/traces` | Trazas de una sesión |

Query params de `/sessions`: `session_id`, `distinct_id`, `has_replay`,
`has_error`.

Cada sesión devuelve: `duration_seconds`, `pageview_count`, `event_count`,
`is_bounce`, `is_engaged`, `converted`, `quality_score`, `error_count`,
`has_replay`, `replay_chunk_count`, `trace_count`, `source`.

## Replays

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/replays` | Lista de sesiones con grabación |
| `GET` | `/replays/:session_id` | Payload del replay (eventos rrweb) |

Query params de `/replays`: `service`, `session_id`.

## Product Users

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/product-users` | Usuarios identificados con breakdown de devices |
| `GET` | `/product-users/:distinct_id` | Perfil completo del usuario |
| `GET` | `/product-users/:distinct_id/events` | Eventos del usuario en cualquier device |

Query params de `/product-users`: `query` (substring sobre `distinct_id` o
`properties`), `source` (filtra usuarios vistos en web/mobile, repetible).

El worker `user_unifier` fusiona `anonymous_id`s de múltiples devices en un
solo `distinct_id` vía `faro.product_user_aliases`. Los eventos expanden
automáticamente los aliases para incluir actividad pre-login.

## Retention

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/retention` | Retención por cohortes (D1/D7/D30) |

Query params: `event_name` (evento ancla; default cualquier evento),
`interval` (`day` — único soportado).

Devuelve `{ columns: [1,7,30], cohorts: [{ cohort_date, cohort_size, d1_users, d7_users, d30_users }] }`.

## Cohorts

Ver [product-analytics.md](product-analytics.md#cohorts) para el contrato completo.

## Funnels

Ver [product-analytics.md](product-analytics.md#funnels) para el contrato completo.

## Experiments

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `POST` | `/experiments/analyze` | Análisis A/B de un feature flag |

Body:

```json
{
  "flag_key": "new-checkout",
  "conversion_event": "checkout_completed",
  "project": "default",
  "from": "2026-06-01T00:00:00Z",
  "to": "2026-06-15T00:00:00Z"
}
```

Response:

```json
{
  "flag_key": "new-checkout",
  "variants": [
    { "variant": "control", "sample": 5000, "conversions": 250, "conversion_rate": 0.05 },
    { "variant": "treatment", "sample": 5000, "conversions": 310, "conversion_rate": 0.062 }
  ],
  "winner": "treatment",
  "absolute_delta": 0.012,
  "relative_lift": 0.24,
  "p_value": 0.021,
  "ci95_low": 0.003,
  "ci95_high": 0.021,
  "summary": "Treatment es significativamente mejor (p=0.021, IC95 no cruza cero)."
}
```

**Estadística**: z-test de dos proporciones con **SE pooled** (error estándar
combinado). El IC95 usa SE no-pooled. Umbral de significancia: **p < 0.05**.
`winner` es `control`, `treatment` o `none` (si p ≥ 0.05). **Solo soporta A/B**
(dos variantes); multi-variant devuelve error.

## Insights

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/insights/service-dashboard` | Resumen combinado por servicio |
| `GET` | `/insights/revenue-impact` | Impacto estimado en ingresos por issue |
| `GET` | `/insights/latency-funnel-impact` | Latencia vs conversión del funnel |
| `GET` | `/insights/web-vitals-conversion-impact` | Web vitals vs conversión |

Todos aceptan `?service=` y rango estándar.

**service-dashboard**: devuelve issues ordenados por impacto, con
`error_count`, `affected_users`, `revenue_at_risk`, `latency_p95_ms`.

**revenue-impact**: estima ingresos perdidos por errores usando valor de orden
promedio × conversión afectada.

**latency-funnel-impact**: bucketa requests por latencia y calcula conversión
por bucket — detecta "a partir de X ms la conversión cae".

**web-vitals-conversion-impact**: correlaciona LCP/INP/CLS con conversión.

## Dashboard

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/dashboard` | Contadores agregados |

Devuelve `{ log_count, error_count, service_count, trace_count, open_issue_count, firing_incident_count, monitors_total, monitors_down }`.

## Projects

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/projects` | Lista (admin) |
| `POST` | `/projects` | Crear (admin) |
| `GET` | `/projects/:slug` | Detalle |
| `PUT` | `/projects/:slug` | Editar (admin) |
| `DELETE` | `/projects/:slug` | Soft-delete (admin) |
| `POST` | `/projects/:slug/rotate` | Rotar token de ingesta (admin) |
| `GET` | `/projects/:slug/redaction` | Config de redacción PII |
| `PUT` | `/projects/:slug/redaction` | Actualizar redacción (admin) |
| `GET` | `/projects/:slug/origins` | Origins permitidas (CORS) |
| `PUT` | `/projects/:slug/origins` | Actualizar origins (admin) |

`ProjectView` incluye `dsn` con formato `<endpoint>|<slug>|<token>` — el SDK
parsea este string en `init({ dsn })`.

Redacción: lista de built-ins (`email`, `phone`, `credit_card`, `ssn`, `jwt`,
`api_key`, `password`) + patterns custom (regex). Validación 400 si un pattern
no compila.

## Alerts

Ver [alerts.md](alerts.md).

## Monitors

Ver [monitors.md](monitors.md).

## Feature Flags

### GET /ingest/feature-flags (SDK-facing)

Retorna las flags activas del proyecto identificado por el token de ingesta.

Response:

```json
{
  "project": "default",
  "flags": [
    {
      "key": "new-checkout",
      "rollout_percentage": 50,
      "conditions": { "properties": { "plan": "pro" } },
      "active": 1
    }
  ]
}
```

El SDK hace sticky bucketing por `distinct_id` (FNV-1a) contra
`rollout_percentage`, y luego evalúa `conditions.properties` contra el contexto
local. `conditions` es público (se sirve al browser) — no pongas secretos.

Ver [feature-flags-experiments.md](feature-flags-experiments.md) para creación y
configuración.

## Notification Channels

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/integrations/channels` | Lista (admin) |
| `POST` | `/integrations/channels` | Crear (admin) |
| `GET` | `/integrations/channels/kinds` | Kinds soportados |
| `GET` | `/integrations/channels/:id` | Detalle |
| `PUT` | `/integrations/channels/:id` | Upsert (admin) |
| `DELETE` | `/integrations/channels/:id` | Soft-delete (admin) |
| `POST` | `/integrations/channels/:id/test` | Notificación de prueba |

Ver [README — Canales](../README.md#canales-de-notificación) para kinds y config.

## Preferences

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/me/preferences` | Preferencias de UI del usuario actual |
| `PUT` | `/me/preferences` | Actualizar |

Campos: `theme` (`light`/`dark`/`system`), `default_project`, `default_time_range`
(`5m`/`15m`/`1h`/`6h`/`24h`/`7d`). Validación: `400` si el tema o rango no está
en la whitelist.

La página `/settings/appearance` del dashboard es la UI que edita estas
preferencias. `theme: system` sigue `prefers-color-scheme` del SO. El cambio es
instantáneo (sin reload) vía un store Svelte reactivo.

## Auth & Account

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `POST` | `/auth/login` | Login (email + password + TOTP si activo) |
| `POST` | `/auth/logout` | Revoca sesión actual |
| `GET` | `/me` | Usuario actual |
| `GET` | `/me/preferences` | Preferencias de UI (tema, defaults) |
| `PUT` | `/me/preferences` | Actualizar preferencias |
| `GET` | `/me/sessions` | Sesiones activas |
| `POST` | `/me/sessions/revoke-others` | Revoca todas menos la actual |
| `GET` | `/me/security/2fa` | Status 2FA |
| `POST` | `/me/security/2fa/setup` | Inicia enrolamiento (secret + QR) |
| `POST` | `/me/security/2fa/enable` | Verifica código y activa → recovery codes |
| `POST` | `/me/security/2fa/disable` | Desactiva (password + TOTP/recovery) |
| `POST` | `/me/security/2fa/recovery-codes` | Regenera recovery codes |

## Users (admin)

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/users` | Lista (admin) |
| `POST` | `/users` | Crear (admin) |
| `DELETE` | `/users/:id` | Soft-delete (admin) |

## Rate limiting

- **Ingesta**: `429` con `Retry-After` cuando el proyecto excede
  `INGEST_RATE_LIMIT_PER_MIN` (default 10 000/min).
- **Auth**: `429` tras 5 intentos fallidos en 60s (`/auth/login`, `/auth/totp`).
- **SSE** (`/logs/live`, `/events/live`): sin rate limit explícito, pero el
  channel de broadcast tiene capacidad finita. Si el cliente no consume rápido
  suficiente, la conexión se cierra. El filtro `?project=` es aceptado en todos
  los endpoints de lectura y SSE para acotar resultados.

## Contrato de ingesta de product events

`POST /ingest/events` acepta dos shapes (ambas soportadas para compat):

```json
// Nuevo contrato (recomendado):
{ "service": "web", "batch": [{ "event": "checkout", "distinct_id": "user_42", ... }] }

// Legacy (los SDKs actuales lo usan):
{ "service": "web", "events": [{ "name": "checkout", "distinct_id": "user_42", ... }] }
```

El backend prefiere `batch` con campo `event`; si no está, cae a `events` con
campo `name`. Máximo **100 eventos** por request (`400` si excede).

> **Nota sobre el límite**: el cap del backend es 100 eventos/request. Los SDKs
> server (Node, Go, Python) usan `maxBatchSize` default de 100-200. Si tu SDK
> envía batches de 200, el backend rechaza con 400. Asegurá que
> `maxBatchSize ≤ 100` en la config del SDK, o el batch se descarta
> silenciosamente (pérdida de datos).

## Límites de ingesta

| Signal | Límite por request | Validación |
| ------ | ------------------ | ---------- |
| Logs | Sin cap explícito (rate limit compartido) | — |
| Events | `MAX_BATCH_EVENTS = 100` | `400` si el batch excede |
| Spans | Sin cap explícito (rate limit por record) | — |
| Metrics | Sin cap explícito (rate limit por record) | — |
| Replay | `events.len() ≤ 5000` por chunk | `400` si excede; `session_id` ≤128 chars |

Los SDKs usan defaults de 50-100 eventos por flush, así que el cap de events no
impacta en uso normal. El rate limiter comparte bucket entre todas las signals
— un proyecto no puede esquivar el límite cambiando de signal.

## PII Redaction

La redacción de PII se configura **por proyecto** desde el dashboard
(`/projects/:slug/redaction`) y se aplica **al ingestar**: el row entra ya
redactado a ClickHouse, sin guardar el original.

### Built-ins disponibles

| Slug | Qué matchea |
| ---- | ----------- |
| `email` | Direcciones de correo (`user@host.tld`) |
| `jwt` | Tokens JWT (`xxx.yyy.zzz` base64url, min 8 chars/segmento) |
| `credit_card` | Secuencias de 13-19 dígitos (con/sin separadores) |
| `bearer` | `Authorization: Bearer xxx` y variantes |
| `password_kv` | `password=`, `pwd=`, `pass=` en logs key=value |
| `apikey_kv` | `api_key=`, `apikey=`, `secret=`, `token=` en logs key=value |
| `ip` | IPv4 (IPv6 omitido — FPs con MAC addresses) |

### Custom rules

Regex + replacement, validadas al guardar (`PUT /projects/:slug/redaction`):
- `400` si el pattern no compila
- `400` si contiene nested quantifiers o backtracking exponencial (el crate
  `regex` no soporta lookaround, lo que elimina la mayoría de patrones peligrosos)
- Los patterns se aplican a `body`, `message`, y valores de `attributes` de
  cada row

### Por signal

- **Logs**: redacta `body` + valores de `attributes`
- **Events**: redacta `message` + valores de `properties`
- **Spans**: redacta valores de `attributes`, **preserva** `name` del span (el
  nombre de la operación es información operacional, no PII)
- **Replay**: no aplica redacción server-side (los eventos rrweb se guardan
  crudos; el SDK browser hace `maskAllText`/`maskInputs` client-side)

El texto redactado se reemplaza por `[REDACTED]`.
