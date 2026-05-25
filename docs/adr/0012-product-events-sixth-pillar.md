# ADR-0012: Product events como 6º pilar de observabilidad

- **Estado**: Accepted
- **Fecha**: 2026-05-24
- **Autores**: @victalejo

## Contexto

Faro hasta hoy captura cinco pilares: logs, traces, metrics, errors y monitores
HTTP. Eso responde a "¿el sistema falló?", pero no a "¿el usuario hizo X?". Las
apps de los clientes (web, mobile) necesitan reportar eventos de producto
—`page_view`, `checkout_completed`, `signup`— para análisis de adopción,
embudos, retención y cohortes. Hoy esa pieza la cubren productos separados
(Mixpanel, Amplitude, PostHog) que viven aislados del resto de telemetría:
cuando una conversión cae, hay que cruzar a mano entre el dashboard de producto
y el de observabilidad para saber si fue un bug, latencia o decisión del
usuario.

Tener ambos mundos en el mismo backend permite, además, **vincular product
events con traces OTel**: un `checkout_completed` puede llevar el `trace_id` del
span del backend que lo procesó. Ninguno de los productos comerciales lo hace
porque no son su universo. En Faro, los traces ya viven en ClickHouse al lado;
juntarlos sale casi gratis si el schema lo soporta desde el día uno.

## Decisión

Añadimos un **sexto pilar** llamado `product_events`, con su propio schema en
ClickHouse, ingesta HTTP separada (`POST /ingest/events`) y endpoints de lectura
(`GET /events`, `GET /events/stats`, `GET /events/live`, `GET /funnels/events`,
`POST /funnels/compute`). El schema:

- **`faro.product_events`** — tabla principal, una fila por evento. `properties`
  / `user_properties` / `context` viajan como **String JSON**, no como `Map`.
  `trace_id`/`span_id` opcionales para linkear con un span existente. Retención
  365 días.
- **`faro.product_users`** — ReplacingMergeTree con `last_seen` como versión.
  Permite "¿cuántos usuarios únicos vimos esta semana?" sin escanear
  `product_events`.
- **`faro.product_sessions`** — pre-agregada por un worker que cierra sesiones
  por inactividad. ReplacingMergeTree con `ended_at` como versión para extender
  sesiones en flight. Además guarda propiedades de sesión (`event_count`,
  `pageview_count`, `is_bounce`, `is_engaged`, `converted`, `quality_score`) y
  el vínculo session → traces (`trace_ids`, `trace_count`) derivado de los
  eventos de producto.
- **Materialized views** que llenan `product_events_per_day` (`countState`) y
  `product_unique_users_per_day` (`uniqExactState(distinct_id)`) directamente
  desde `product_events`. Cohorts y cards instantáneas sin escaneo.

Decisiones de schema explícitas y los motivos detrás:

1. **JSON String en vez de `Map(String, String)`** para `properties` /
   `user_properties` / `context`. Una `Map` con cardinalidad de keys ilimitada
   (cada producto inventa sus propias keys) mata el merge de granules y dispara
   el footprint en RAM. JSON con `JSONExtractString()` en queries es más lento
   por fila pero predecible en tamaño y compatible con el ZSTD(3) que ya
   usamos para columnas verbose.

2. **`distinct_id` + `anonymous_id` separados** (no un único `user_id`). Antes
   del login el usuario tiene un `anonymous_id` (cookie/device); tras el login
   se le asigna un `distinct_id` estable. El evento `$alias` los une. Sin esta
   separación, las sesiones pre-login se pierden o se atribuyen a un usuario
   sintético — patrón heredado de PostHog/Segment, probado en producción.

3. **`uniqExact` (no `uniq` HLL)** en la MV de usuarios únicos por día. `uniq`
   es aproximado (~2% de error) y suficiente para dashboards de tráfico, pero
   los cohorts necesitan exact match: "¿estos 5 usuarios que pagaron en marzo
   también pagaron en abril?" no tolera fuzziness. `uniqExactState` ocupa más,
   pero la cardinalidad por (día, proyecto) es manejable.

4. **TTL 365 días** (vs 30 días de logs). El análisis de cohortes y retención
   exige histórico largo: "% de usuarios de hace 12 meses que siguen activos"
   es una métrica común. Logs los podemos rotar agresivamente porque son para
   debug en caliente; product events son la métrica de negocio.

5. **`product_users` y `product_sessions` poblados por worker, no MV**.
   `ReplacingMergeTree` reemplaza la fila completa al merger, así que un
   `first_seen = min(...)` materializado vía MV pierde el valor mínimo en el
   primer replacement. El worker lee batches, hace `min/max` correctamente y
   hace upsert. Trade-off aceptado: latencia de unos segundos vs. exactitud.

6. **Session → trace se materializa en `product_sessions`**. Para sesiones con
   `session_id` enviado por SDK se podría reconstruir desde `product_events`,
   pero las sesiones sintéticas se derivan por gap y sus eventos originales
   conservan `session_id = ''`. Guardar `trace_ids` durante la agregación hace
   posible responder "qué traces backend sirvieron esta sesión" sin re-ejecutar
   la lógica de sesionización en cada lectura. El endpoint protegido
   `GET /api/v1/sessions/:session_id/traces?project=<project>` resuelve esos ids
   contra `faro.spans` y devuelve el mismo summary usado por `/traces`.

7. **PROJECTION `by_event`** en `product_events`. El `ORDER BY` primario es
   `(project_id, timestamp, event_name)` para rangos temporales por proyecto
   (lectura natural del dashboard). La projection re-ordena por
   `(project_id, event_name, timestamp)` para queries del tipo "todos los
   `checkout_completed` del último mes" sin escanear granules irrelevantes.

## Alternativas consideradas

- **Reusar `faro.logs` con un nivel especial `event`**. Simple pero confunde la
  semántica de logs (debugging) con la de product events (negocio); los TTLs
  son distintos (30d vs 365d), las queries son distintas y el schema necesita
  columnas que los logs no tienen (`distinct_id`, `session_id`). Forzar ambos
  en la misma tabla resulta en columnas opcionales y predicates `WHERE
  severity_text = 'event'` por todos lados.

- **Mandar product events a un Mixpanel/PostHog externo**. Pierde el linkeo con
  traces, suma una dep externa al stack y desalinea el modelo de "todo en
  ClickHouse" que ADR-0002 estableció. Además, para clientes regulados
  (sanidad, finanzas) mandar product events a un SaaS de terceros es un nudo
  legal — Faro self-hosted resuelve eso.

- **Mantener `properties` como columnas typed individuales** (una por evento
  posible). No-go: el universo de eventos custom no se conoce de antemano,
  cada cliente define los suyos. Una migración cada vez que alguien quiere
  trackear un evento nuevo es operacionalmente insostenible.

## Consecuencias

### Positivas

- **Linkeo product-event ↔ trace** vía `trace_id`/`span_id`. Un funnel que
  muestra "100 checkout iniciados, 60 completados" puede mostrar para los 40
  que abandonaron el span del backend y latencia/errores asociados.
- **Linkeo session ↔ traces** vía `product_sessions.trace_ids`. Una sesión con
  drop-off, error o baja calidad puede abrir directamente los traces backend
  que ocurrieron durante esa navegación.
- **Cohorts y retención sobre el mismo backend de observabilidad**. Una sola
  ClickHouse, un solo dashboard, un solo modelo de auth.
- **TTL diferenciado por tipo de dato**. Logs/traces se rotan rápido (storage
  caliente); product events viven 365 días para análisis longitudinal.
- **MVs y projection cubren los hot paths** sin afectar la ingesta: writes a
  `product_events` no esperan al cómputo de agregados.

### Negativas / costo asumido

- **Más superficie de queries y APIs** que mantener. Endpoints nuevos
  (`/events`, `/funnels`), schemas nuevos en utoipa, frontend nuevo
  (siguiente fase).
- **Worker adicional** para `product_users` y `product_sessions`. Suma carga
  operativa y un loop más que monitorear.
- **`uniqExact` no se puede mergear con `uniq`** — si en algún momento queremos
  cohorts multi-proyecto o sliding window, hay que estar atentos a no mezclar
  los dos tipos de state.

### Trabajo de seguimiento

- **Worker de sesionalización** (10.F.1): implementado con
  `session_aggregator`; respeta `session_id` del SDK y, cuando viene vacío,
  corta sesiones por gap de 30 min en `product_sessions`. La unificación de
  usuarios vive en el worker separado `user_unifier`.
- **Endpoints de cohorts** (10.C): mergear `product_unique_users_per_day` con
  filtros de cohorte (usuarios que hicieron evento X en día N y volvieron en
  día N+7).
- **`mv_funnel_steps`** (10.E): MV adicional para funnels pre-calculados;
  difiere a su propia ADR cuando se diseñe.
- **SDKs** (10.F): `track`/`identify`/`page`/`screen`/`alias` en los SDKs
  `@iaportafolio/*` con la convención PostHog (`$identify`, `$pageview`,
  `$screen`, `$alias`).
- **Frontend** (10.G): la superficie de product analytics cubre `/events`,
  `/users`, `/retention`, `/sessions` e `/insights`. La guia operativa de estas
  vistas y sus APIs vive en [`../product-analytics.md`](../product-analytics.md).

### Archivos creados/modificados en esta iteración (10.A)

- `clickhouse/init/85-product-events.sql` — definición canónica (DB fresca).
- `clickhouse/init/86-product-events-aux.sql` — `product_users`,
  `product_sessions`, MVs.
- `clickhouse/migrations/013-product-events.sql` — migration para instancias
  existentes (paralelo al init).
- `clickhouse/migrations/014-product-aux-tables.sql` — idem aux.
- `clickhouse/test-migrations.sh` — `EXPECTED[]` extendido con las 7 nuevas
  entradas (3 tablas de datos + 2 tablas-estado + 2 MVs).
