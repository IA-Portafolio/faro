# 10.B SDK Tracking API Design

- **Fecha:** 2026-05-24
- **Estado:** Aprobado para implementacion
- **Scope:** API canonica de tracking en los 7 SDKs

## Objetivo

Cerrar la API de tracking estilo Segment/PostHog en todos los SDKs de Faro:

- `track(name, properties)`
- `identify(userId, traits)`
- `alias(previousId, userId)`
- `page(path, properties)` solo en runtimes web/mobile donde aplica
- `screen(name, properties)` solo en runtimes mobile donde aplica

La implementacion debe enviar product events a `POST /api/v1/ingest/events` y mantener una semantica consistente de IDs entre SDKs.

## Alcance

El working tree ya contiene una implementacion parcial o mayoritaria de esta API. El trabajo de esta iteracion no es reescribirla, sino cerrar el contrato:

1. Verificar exports/metodos publicos por SDK.
2. Agregar tests focalizados para los metodos que no tengan cobertura.
3. Corregir brechas pequenas si los tests revelan diferencias.
4. Mantener la documentacion de `sdks/README.md` como contrato canonico.

## Contrato

`track` emite `type: "track"` con `name` igual al evento custom.

`identify` fija `distinct_id` para eventos posteriores y emite `type: "identify"`, `name: "$identify"`, con `user_properties` igual a los traits enviados.

`alias` fija `distinct_id` al nuevo usuario y emite `type: "alias"`, `name: "$alias"`, llevando el ID previo en `anonymous_id`.

`page` emite un page view con `type: "page"` y fuente `web` cuando hay contexto web. Para Flutter tambien existe por soporte web.

`screen` emite `type: "screen"` y fuente `mobile` en Expo, Flutter y Kotlin.

Los SDKs server-side no deben exponer `page` ni `screen` salvo que el runtime tenga un concepto propio de navegacion cliente.

## Testing

Seguir TDD sobre brechas de cobertura. Los tests deben capturar el payload HTTP real con servidores locales o los helpers existentes, sin mocks de comportamiento interno salvo stubs de browser/RN inevitables.

Verificacion esperada:

- `npm test` en `sdks/node`, `sdks/nextjs` y `sdks/expo` cuando se toquen.
- `go test ./...` en `sdks/go` si se toca Go.
- `pytest tests/` en `sdks/python` si se toca Python.
- `flutter test` en `sdks/flutter` si se toca Flutter.
- Gradle test en `sdks/kotlin` si se toca Kotlin.
