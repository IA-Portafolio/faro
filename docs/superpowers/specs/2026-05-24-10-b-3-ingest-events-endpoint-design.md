# 10.B.3 Events Ingest Endpoint Design

- **Fecha:** 2026-05-24
- **Estado:** Aprobado para implementacion
- **Scope:** `POST /api/v1/ingest/events`

## Objetivo

Cerrar el contrato de ingesta de product events:

```json
{
  "batch": [
    {
      "type": "track",
      "event": "checkout_completed",
      "distinct_id": "user_42",
      "anonymous_id": "anon_abc",
      "session_id": "ses_xyz",
      "properties": { "amount": 99.50 },
      "context": { "user_agent": "...", "url": "..." },
      "timestamp": "2026-05-24T10:23:00.123Z",
      "trace_id": "abc"
    }
  ]
}
```

El backend autentica por `Authorization: Bearer <project_token>`, valida, aplica redaccion y persiste en `faro.product_events`.

## Contexto

El working tree ya tiene `backend/src/ingest/events.rs` conectado a `/api/v1/ingest/events`, pero acepta el envelope `{ service, events: [...] }` y usa `name` como nombre del evento. Los SDKs actuales mandan ese formato. La implementacion debe aceptar el contrato nuevo sin romper compatibilidad.

## Diseño

El endpoint acepta ambos envelopes:

- Nuevo: `batch`
- Legacy SDK: `events`

Cada item acepta ambos nombres:

- Nuevo: `event`
- Legacy SDK: `name`

Para `track`, el `event_name` final debe ser slug-like y tener entre 1 y 64 caracteres. Se permite alfabeto ASCII `A-Z`, `a-z`, digitos, `_`, `-`, `.`, `$`. Esto cubre eventos de usuario (`checkout_completed`) y especiales PostHog (`$feature_exposure`).

Eventos especiales conservan la semantica existente:

- `identify` -> `$identify`
- `page` -> `$pageview`
- `screen` -> `$screen`
- `alias` -> `$alias`

`properties`, serializado como JSON, no puede exceder 16KB. El limite aplica antes de insertar; requests con cualquier evento invalido devuelven `400` y no insertan el batch.

La redaccion se aplica a `properties`, `user_properties` y `context`. No se redactan `event_name`, `distinct_id` ni `anonymous_id` para no romper grouping ni identity joins.

## Testing

Agregar tests unitarios en `backend/src/ingest/events.rs` para:

- `batch` + `event` normaliza un `track`.
- `events` + `name` sigue funcionando.
- nombres invalidos se rechazan.
- `properties` mayor a 16KB se rechaza.
- redaccion toca `properties/context` pero no `event_name`.

Verificacion focalizada:

- `cargo test ingest::events --lib`
- si el entorno lo permite, tests de integracion de ingest events.
