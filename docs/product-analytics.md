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
| `/insights` | Dashboard combinado que cruza conversion de eventos, errores linkeados a sesiones fallidas y p95 de spans backend. |

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
  linkeados a sesiones sin checkout posterior. Parametros clave:
  `checkout_event`, `average_order_value`, `service`.
- `GET /api/v1/insights/latency-funnel-impact`: compara buckets lentos de un
  span contra conversion de un funnel. Parametros clave: `span_name`, `service`,
  `funnel_from`, `funnel_to`, `latency_threshold_ms`, `bucket_minutes`.
- `GET /api/v1/insights/web-vitals-conversion-impact`: cruza web vitals
  reportadas como logs contra conversion. Parametros clave: `metric`,
  `threshold_ms`, `conversion_event`, `pageview_event`, `service`.

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

## Flujo recomendado para instrumentar un producto

1. Emitir `session_id` estable por sesion de navegador o mobile.
2. Emitir `distinct_id` estable para el usuario final; antes de login usar el
   id anonimo y luego unificar via alias/identify.
3. Enviar eventos clave de negocio (`signup`, `login`, `checkout_started`,
   `checkout_completed`) con `trace_id` y `span_id` cuando el evento se origina
   en una respuesta backend instrumentada.
4. Adjuntar `session.id` o `session_id` a errores frontend/backend para que
   `/sessions` e `/insights` puedan explicar que sesiones se rompieron.
5. Enviar replay chunks con el mismo `session_id`; cuando el reproductor rrweb
   este disponible, `/sessions` podra abrir la reproduccion desde la fila.

## Limitaciones actuales

- La retencion soporta intervalos diarios y columnas fijas D1/D7/D30.
- `/sessions` muestra disponibilidad y conteos de replay; la reproduccion rrweb
  queda condicionada a completar la pieza de replay.
- Los endpoints combinados dependen de que clientes y SDKs propaguen
  consistentemente `session_id`, `trace_id` y atributos de error. Sin esos ids,
  Faro puede contar eventos aislados, pero no explicar causalidad entre pilares.
