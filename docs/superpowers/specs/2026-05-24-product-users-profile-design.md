# Product users profile design

- **Fecha:** 2026-05-24
- **Estado:** Draft aprobado para plan de implementación
- **Scope:** Goal 10.C.4 `/users` product, no admin

## Objetivo

Convertir `/users` en la vista de end-users del producto: los `distinct_id` que
llegan desde las apps cliente. Esta pantalla no administra usuarios del
dashboard Faro; esos siguen viviendo en `/settings/users`.

La experiencia debe parecerse al perfil de usuario de PostHog/Mixpanel:
lista de usuarios, click en uno, y perfil con timeline cronológico de eventos,
sesiones y traces vinculadas.

## Decisiones

Usamos ruta dedicada para el perfil:

- `/users` lista end-users de producto.
- `/users/[distinct_id]` muestra el perfil de un end-user.

La ruta dedicada es preferible al drawer porque el perfil necesita deep links,
historial del navegador, espacio para crecer y links compartibles. El drawer
queda reservado para detalles puntuales de un evento dentro del timeline.

## Arquitectura

El backend ya expone la base necesaria:

- `GET /api/v1/product-users`
- `GET /api/v1/product-users/:distinct_id`
- `GET /api/v1/product-users/:distinct_id/events`

El frontend debe agregar helpers tipados en `src/lib/api.ts` para esos
endpoints y reemplazar el redirect actual de `src/routes/users/+page.svelte`.

Para traces no hace falta endpoint nuevo: los eventos ya traen `trace_id` y el
perfil puede linkear a `/traces/:trace_id`.

Para sesiones, la primera iteración agrupa eventos por `session_id` en el
cliente. Si existe replay para una sesión, el perfil debe poder linkear a
`/replays/:session_id`. Si la API actual no permite validar disponibilidad de
replay de forma barata para un usuario, se mostrará el link contextual sólo
cuando la sesión venga de datos que ya confirman replay; el endpoint dedicado
para "sessions del product user con replay flag" queda fuera de esta iteración.

## `/users`

La pantalla lista usuarios activos en el rango actual y proyecto seleccionado.
Debe usar los stores globales de proyecto y rango, igual que `/events`.

Columnas:

- `distinct_id`
- `last_seen`
- `first_seen`
- `event_count`
- `sources`
- cantidad de `anonymous_ids`
- preview de `properties`

Controles:

- búsqueda por `distinct_id` o properties
- filtro `source`
- recargar
- selección global de rango/proyecto existente

Cada fila navega a `/users/[distinct_id]`, preservando `project` y `range` en
query string cuando estén presentes.

## `/users/[distinct_id]`

La pantalla carga en paralelo:

- resumen del usuario desde `/product-users/:distinct_id`
- eventos desde `/product_users/:distinct_id/events`

Header:

- `distinct_id`
- first/last seen
- event count
- sources
- anonymous IDs
- properties principales

Breakdown:

- tarjetas compactas por source/device con event count, last seen y conteo de
  anonymous IDs.

Timeline:

- orden cronológico descendente por defecto, igual que el endpoint.
- eventos como unidad base.
- sesiones como grupos derivados por `session_id`, con inicio, fin y cantidad
  de eventos.
- links directos a `/traces/:trace_id` cuando exista `trace_id`.
- links directos a `/events?distinct_id=...` para abrir exploración avanzada.
- detalle de evento reutilizando o adaptando `EventDetailDrawer`.

Estados:

- loading con skeletons.
- empty state cuando el usuario existe pero no tiene eventos en el rango.
- not found/error cuando el `distinct_id` no existe.

## Data flow

`/users`:

1. lee `selectedProject` y `timeRange`
2. llama `fetchProductUsers`
3. renderiza tabla
4. navega al perfil al hacer click

`/users/[distinct_id]`:

1. lee `distinct_id` desde params
2. resuelve rango/proyecto desde stores globales
3. carga summary y eventos en paralelo
4. construye sesiones en memoria agrupando por `session_id`
5. renderiza header, breakdown y timeline

## Errores y seguridad

Los endpoints protegidos ya pasan por auth de sesión. El frontend debe manejar
401 vía `api()` existente.

Los parámetros se codifican con `encodeURIComponent`. No se renderiza JSON de
properties con `{@html}`; todo se parsea y muestra como texto.

## Testing

Seguir TDD en implementación:

- tests de helpers frontend para URL builders de product users.
- tests unitarios para agrupar eventos por sesión y ordenar timeline.
- si se agrega endpoint backend para sesiones/replays, test Rust antes de la
  implementación.

Verificación final esperada:

- `npm test` o tests focalizados del frontend.
- `npm run check` si está disponible en frontend.
- build/check Rust sólo si se toca backend.

## Fuera de scope

- administrar users del dashboard Faro.
- edición de user properties.
- merge manual de identities.
- endpoint backend nuevo para sessions/replay si no es estrictamente necesario.
- cohort membership dentro del perfil.
