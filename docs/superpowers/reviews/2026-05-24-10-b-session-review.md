# Revision 10.B SDK tracking/events

Fecha: 2026-05-24

Alcance revisado:

- SDK browser/Next.js: tracking API, autoCapture, identidad estable.
- SDKs server-side Node/Python/Go: auto-correlacion con tracing.
- Backend `POST /api/v1/ingest/events`: validacion, redaccion, alias e identidad.
- Documentacion en `sdks/README.md` y READMEs por SDK.

## Hallazgos

### P1 - `product_users` puede quedar temporalmente degradado por el upsert inmediato de alias

Archivo: `backend/src/ingest/events.rs`

El upsert best-effort de `$alias` inserta una fila nueva en `faro.product_users` con:

- `first_seen = row.timestamp`
- `last_seen = row.timestamp`
- `anonymous_ids = [row.anonymous_id]`
- `event_count = 1`
- `properties = row.user_properties`

Como `product_users` usa `ReplacingMergeTree(last_seen)`, si esa fila tiene un `last_seen` posterior al row canonico existente, puede ganar en `FINAL` y ocultar temporalmente `anonymous_ids`, `event_count`, `sources` y `properties` previos hasta que el worker `user_unifier` reconcilie.

Recomendacion:

- O bien no insertar `product_users` en el camino sincrono y dejar solo `product_user_aliases` inmediato.
- O hacer merge con la fila existente antes de insertar, preservando `first_seen`, uniendo arrays y sumando/derivando `event_count` de forma consistente.

### P2 - El upsert de alias ocurre antes de saber si el event fue aceptado

Archivo: `backend/src/ingest/events.rs`

El upsert de `product_user_aliases` / `product_users` ocurre antes de `try_send(row)` al canal de ingesta de events. Si el canal esta lleno y el event se descarta, la relacion de identidad puede quedar escrita aunque el evento no haya sido aceptado.

Recomendacion:

- Construir `alias_rows` / `user_rows` solo para filas que pasaron `try_send`.
- O mover el mantenimiento inmediato de identidad al writer/worker que confirma el insert en ClickHouse.

### P2 - Auto-deteccion OTel en Node depende de `process.cwd()`

Archivo: `sdks/node/src/index.ts`

La carga opcional de `@opentelemetry/api` usa `createRequire(`${process.cwd()}/faro-sdk.js`)`. Esto evita el warning de CJS, pero puede fallar si la app cambia el working directory, corre desde un subdirectorio, o el SDK se usa en un worker con cwd distinto. En esos casos el provider explicito `traceContext` funciona, pero la auto-deteccion deja de ser confiable.

Recomendacion:

- Resolver `@opentelemetry/api` desde el modulo del SDK cuando el formato lo permita, o documentar el fallback como "best effort".
- Agregar un test con dependencia OTel simulada o mockeada para cubrir la ruta automatica, no solo `traceContext`.

## Verificacion ejecutada

Pasaron:

- `sdks/nextjs`: `npm test` -> 24/24.
- `sdks/node`: `npm test` -> 20/20.
- `sdks/python`: `python -m pytest tests/test_client.py -q` -> 16/16.
- `sdks/go`: `go test ./...` -> OK.
- `git diff --check` para los archivos editados -> OK.

No ejecutado:

- Tests Rust de backend. `cargo` no esta disponible en este entorno (`cargo: The term 'cargo' is not recognized`).

## Documentacion revisada

Ya existe documentacion para:

- API `track` / `identify` / `page` / `screen` / `alias`.
- `autoCapture` web.
- `POST /api/v1/ingest/events` y wire format.
- Auto-correlacion con `trace_id` / `span_id`.
- Identidad estable browser con `crypto.randomUUID()`, `$alias {from,to}` y queries retrospectivas.

Pendiente recomendado:

- Documentar explicitamente que el upsert inmediato de identidad es best-effort y que `user_unifier` es la fuente de reconciliacion definitiva, o ajustar la implementacion para que el camino inmediato haga merge canonico.
