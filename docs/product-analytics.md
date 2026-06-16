# Product analytics

Faro trata los eventos de producto como un sexto pilar junto a logs, traces,
metrics, errors y monitores. La diferencia importante no es solo tener eventos,
sino poder cruzarlos con las otras señales: una sesion puede abrir sus traces,
un checkout fallido puede mostrar errores linkeados y un dashboard puede poner
conversion, errores y latencia en el mismo contexto.

## Vistas del frontend

| Ruta | Para qué |
| --- | --- |
| `/events` | Exploracion de eventos de producto, actividad live y preview de funnels. |
| `/users` | Lista de end-users del producto por `distinct_id`; al abrir uno muestra timeline cronologico de events, sessions y traces linkeados. |
| `/retention` | Heatmap de cohortes D1/D7/D30. El filtro `event_name` define que evento cuenta como retorno; vacio significa cualquier evento. |
| `/sessions` | Lista de sesiones recientes con duracion, pageviews, events, replay, errores, traces linkeados y metricas de calidad de sesion. |
| `/sessions/:session_id/traces?project=<project>` | Traces backend materializados para una sesion concreta; abre cada trace en `/traces/:trace_id`. |
| `/insights` | Dashboard combinado que cruza conversion de eventos, errores linkeados a sesiones fallidas y p95 de spans backend. |
| `/metrics` | Metric explorer unificado. Incluye metricas tecnicas de `faro.metrics` y metricas virtuales derivadas de `faro.product_events`. |

## Endpoints principales

Todos los endpoints viven bajo `/api/v1` y aceptan los parametros comunes de
rango (`from`, `to`, `project`, `limit`) definidos por `Range` cuando aplica.

### Retention

`GET /api/v1/retention`

Parametros:

- `event_name`: evento que define retorno. Si esta vacio, cualquier evento de
  producto cuenta como actividad de retorno.
- `interval`: por ahora solo `day`.

Respuesta: cohortes por fecha con `cohort_size`, `d1_users`, `d7_users` y
`d30_users`. La cohorte se define por el primer evento historico del usuario
dentro del rango consultado.

### Sessions

`GET /api/v1/sessions`

Parametros adicionales:

- `session_id`: filtra una sesion concreta.
- `distinct_id`: filtra las sesiones de un end-user.
- `has_replay`: acepta `1`, `true`, `yes`, `y` u `on`.
- `has_error`: acepta `1`, `true`, `yes`, `y` u `on`.

Respuesta: filas de `product_sessions` enriquecidas con replay, errores y
traces:

- `duration_seconds`, `pageview_count`, `event_count`
- `is_bounce`, `is_engaged`, `converted`, `quality_score`
- `has_replay`, `replay_event_count`, `replay_chunk_count`
- `has_error`, `error_count`
- `trace_count`

`GET /api/v1/sessions/:session_id/traces?project=<project>`

Resuelve `product_sessions.trace_ids` contra `faro.spans` y devuelve summaries
equivalentes a la lista de traces. `project` es obligatorio para evitar mezclar
sesiones con ids iguales en proyectos distintos.

La UI de `/sessions` muestra `trace_count` por fila. Cuando el conteo es mayor
que cero, el link abre `/sessions/:session_id/traces?project=<project>` para
navegar los traces backend que sirvieron esa sesion.

### Insights combinados

`GET /api/v1/insights/service-dashboard`

Parametros:

- `service`: default `checkout`.
- `span_name`: default `/api/checkout`.
- `funnel_from`: default `checkout_started`.
- `funnel_to`: default `checkout_completed`.

Devuelve el panel combinado: eventos iniciados/completados, conversion, sesiones
fallidas, errores linkeados a esas sesiones, p95 del span seleccionado y top de
errores que explican fallas del funnel.

Endpoints complementarios del mismo modulo:

- `GET /api/v1/insights/revenue-impact`: estima revenue perdido por errores
  linkeados a sesiones sin checkout posterior. Usa `faro.error_events` y
  `faro.product_events` unidos por `(project_id, session_id)`. Parametros
  clave: `checkout_event`, `average_order_value`, `service`.
- `GET /api/v1/insights/latency-funnel-impact`: compara buckets lentos de un
  span contra conversion de un funnel. Usa p95 de `faro.spans` y conteos de
  `faro.product_events` en el mismo bucket temporal. Parametros clave:
  `span_name`, `service`, `funnel_from`, `funnel_to`,
  `latency_threshold_ms`, `bucket_minutes`.
- `GET /api/v1/insights/web-vitals-conversion-impact`: cruza web vitals
  reportadas como logs contra conversion. Usa `attributes['metric.name']`,
  `attributes['metric.value']` y `session.id` en `faro.logs`, unidos con
  `$pageview` y el evento de conversion en `faro.product_events`. Parametros
  clave: `metric`, `threshold_ms`, `conversion_event`, `pageview_event`,
  `service`.

Ejemplos de preguntas cubiertas:

- "Que errores tocaron sesiones donde despues no hubo `checkout_completed`?"
- "Cuando `/api/checkout` p95 supera 2s, cuanto cae el funnel?"
- "Los usuarios con `LCP > 4s` convierten menos?"

### Metricas derivadas de eventos

Faro expone eventos de negocio como metricas virtuales en los endpoints
existentes de metricas. Esto permite graficar `checkout_completed` por hora
junto a p95 de latencia, error rate y cualquier metrica OTel sin exportar datos
a otro producto.

`GET /api/v1/metrics/names`

Ademas de filas reales de `faro.metrics`, devuelve eventos de producto con esta
convencion:

```text
events.<event_name>.count
```

Ejemplos:

- `events.checkout_completed.count`
- `events.$pageview.count`
- `events.$feature_exposure.count`

Para estas metricas virtuales:

- `metric_type` es `counter`.
- `metric_unit` es `events`.
- `service_name` viene de `product_events.source`, por ejemplo `web`, `mobile`
  o `backend`.

`GET /api/v1/metrics/series?name=events.checkout_completed.count&bucket_seconds=3600`

Devuelve la misma shape que una serie tecnica:

```json
[
  { "ts": "2026-05-24 10:00:00", "value": 42.0 },
  { "ts": "2026-05-24 11:00:00", "value": 37.0 }
]
```

El parametro `service` filtra `product_events.source` para metricas virtuales
de eventos. El parametro `agg` se ignora en estas metricas porque la agregacion
siempre es `count()` por bucket.

### Cohorts

Un cohort segmenta usuarios de producto por comportamiento declarativo. La
definición se evalúa al vuelo contra `faro.product_events` (no hay
materialización de membership).

Endpoints (bajo `/api/v1`):

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `GET` | `/cohorts` | Lista (soft-delete filtrado) |
| `POST` | `/cohorts` | Crear |
| `GET` | `/cohorts/:id` | Detalle |
| `PUT` | `/cohorts/:id` | Editar (bumpea version) |
| `DELETE` | `/cohorts/:id` | Soft-delete |
| `POST` | `/cohorts/preview` | Evaluar sin guardar → `size` + `sample` |
| `GET` | `/cohorts/:id/users` | Miembros paginables |
| `GET` | `/cohorts/:id/retention` | Fracción activa por día hacia atrás |
| `GET` | `/cohorts/:id/overlap?other=<uuid>` | Intersección con otro cohort |

Body de create/update (`CohortInput`):

```json
{
  "name": "Power users checkout",
  "description": "≥3 checkouts en 30 días, plan pro",
  "definition": {
    "event": "checkout_completed",
    "op": ">=",
    "count": 3,
    "last_days": 30,
    "filters": [
      { "key": "plan", "value": "pro" }
    ],
    "user_filters": [
      { "key": "industry", "value": "fintech" }
    ]
  }
}
```

`definition` (`CohortDefinition`):

| Campo | Tipo | Descripción |
| ----- | ---- | ----------- |
| `event` | `string` | Nombre del evento a contar. |
| `op` | `string` | Comparador: `==`, `>=`, `>`, `<=`, `<`. |
| `count` | `u32` | Umbral. Máx 1 000 000. |
| `last_days` | `u32` | Ventana hacia atrás en días. Rango [1, 365]. |
| `filters` | `CohortFilter[]` | Filtros sobre `properties` del evento. Máx 3 (sumados con `user_filters`). |
| `user_filters` | `CohortFilter[]` | Filtros sobre traits del usuario (`product_users.properties` persistidos vía `identify`). Máx 3 (sumados con `filters`). |

`CohortFilter`: `{ "key": string, "value": string }` — match exacto vía
`JSONExtractString(properties, key) = value`.

`preview` devuelve `{ size, sample: string[], took_ms }`. `retention` devuelve
`{ cohort_size, horizon_days, points: [{ day_back, active_users }], took_ms }`
(horizon default 30, máx 90). `overlap` devuelve
`{ size_a, size_b, intersection, jaccard, took_ms }`.

### Funnels

Los funnels miden conversión entre secuencia de eventos. Endpoints (bajo
`/api/v1`):

| Método | Ruta | Descripción |
| ------ | ---- | ----------- |
| `POST` | `/funnels/compute` | Conversión de una secuencia de steps |
| `POST` | `/funnels/drop-off` | Análisis de drop-off por step |
| `POST` | `/funnels/time-to-convert` | Tiempo mediano entre primer y último step |

Body de `/funnels/compute`:

```json
{
  "steps": ["checkout_started", "checkout_completed"],
  "from": "2026-05-01T00:00:00Z",
  "to": "2026-05-31T23:59:59Z",
  "project": "default",
  "window_seconds": 3600
}
```

`window_seconds` define el tiempo máximo permitido entre el primer y el último
step para contar como conversión.

## Modelo de datos esperado

- `faro.product_events` es la fuente de verdad de eventos. Para linkear bien
  con sesiones y backend debe incluir `project_id`, `distinct_id`, `session_id`,
  `event_name`, `trace_id` y `span_id` cuando existan.
- `faro.product_sessions` es producido por `session_aggregator`. La vista de
  sesiones depende de las columnas agregadas por la migracion de propiedades de
  sesion: `is_bounce`, `is_engaged`, `converted`, `quality_score`, `trace_ids`
  y `trace_count`.
- `faro.session_replays` se cruza por `(project_id, session_id)` para marcar
  sesiones con replay disponible.
- `faro.error_events` se cruza por `attributes['session.id']` o
  `attributes['session_id']`. Los SDKs deben enviar uno de esos atributos para
  que errores y sesiones queden linkeados.
- `faro.spans` se cruza por `trace_id` para abrir traces asociados a una
  sesion o para calcular latencia p95 en `/insights`.
- El endpoint de web vitals espera logs con `attributes['metric.name']`,
  `attributes['metric.value']` y `attributes['session.id']` o
  `attributes['session_id']`.
- Las metricas virtuales `events.<event_name>.count` se calculan al vuelo desde
  `faro.product_events`; no crean filas nuevas en `faro.metrics`.

## Flujo recomendado para instrumentar un producto

1. Emitir `session_id` estable por sesion de navegador o mobile.
2. Emitir `distinct_id` estable para el usuario final; antes de login usar el
   id anonimo y luego unificar via alias/identify.
3. Enviar eventos clave de negocio (`signup`, `login`, `checkout_started`,
   `checkout_completed`) con `trace_id` y `span_id` cuando el evento se origina
   en una respuesta backend instrumentada.
4. Adjuntar `session.id` o `session_id` a errores frontend/backend para que
   `/sessions` e `/insights` puedan explicar que sesiones se rompieron.
5. Enviar replay chunks con el mismo `session_id`; `/sessions` muestra
   `has_replay` y desde `/replays/:session_id` se reproduce con rrweb-player.

## Limitaciones actuales

- La retencion soporta intervalos diarios y columnas fijas D1/D7/D30.
- `/sessions` muestra disponibilidad y conteos de replay; la reproducción rrweb
  está disponible en `/replays/:session_id` cuando la sesión tiene chunks grabados.
- El link session -> traces depende de que los eventos de producto traigan
  `trace_id`; si el SDK no propaga ese valor, `trace_count` queda en cero aunque
  existan spans backend en ClickHouse.
- Los endpoints combinados dependen de que clientes y SDKs propaguen
  consistentemente `session_id`, `trace_id` y atributos de error. Sin esos ids,
  Faro puede contar eventos aislados, pero no explicar causalidad entre pilares.
- Las metricas virtuales de eventos hoy soportan conteo por bucket. Sumas de
  revenue, percentiles de propiedades JSON o rates derivados quedan para una
  capa posterior de metricas declarativas.
