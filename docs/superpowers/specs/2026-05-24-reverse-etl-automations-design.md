# Reverse ETL automations design

- **Fecha:** 2026-05-24
- **Estado:** Draft aprobado para plan de implementación
- **Scope:** Goal 10.K.1 Reverse ETL simple

## Objetivo

Permitir que Faro dispare webhooks hacia herramientas externas cuando un
segmento de usuarios cumpla una condición de comportamiento. El caso guía es:

> Cuando un usuario hace `pricing_viewed` 3+ veces sin hacer
> `upgrade_completed`, disparar un webhook a HubSpot.

La primera versión debe dar growth automation útil sin sumar un Zapier completo:
reglas simples sobre product events, usuarios unificados y webhooks genéricos.

## Decisión

Crear una feature propia de `automations`, no extender alerts ni cohorts.

Las alerts están orientadas a incidentes operativos. Los cohorts son una capa de
análisis y exploración. Las automations tienen side effects externos, dedupe por
usuario y semántica de "este usuario está listo para una acción". Mantenerlas
separadas evita mezclar consultas analíticas con acciones mutantes.

La implementación se apoya en tablas ya existentes:

- `faro.product_events` para detectar comportamiento.
- `faro.product_users` para enriquecer el payload con perfil unificado.
- `faro.product_user_aliases` queda disponible indirectamente vía el worker de
  unificación, pero no es necesario consultarla en la v1.

## Arquitectura

Agregar:

- `faro.automation_rules`: reglas activas/inactivas con definición JSON,
  configuración webhook, timestamps, soft-delete y versionado.
- `faro.automation_deliveries`: historial de disparos por regla y usuario,
  con estado `delivered` o `failed`.
- API REST bajo `/api/v1/automations` para CRUD y preview.
- Worker `automation_runner` que evalúa reglas activas por intervalo y dispara
  webhooks.
- Página frontend "Automations" para crear reglas, ver estado y revisar envíos
  recientes.

La v1 sólo soporta destino webhook HTTP genérico. Integraciones específicas
como HubSpot OAuth, Salesforce o Customer.io quedan fuera de scope; el cliente
puede pegar la URL de webhook de su herramienta o de un middleware propio.

## Modelo de regla

`automation_rules.definition` se guarda como JSON String. Esquema v1:

```json
{
  "trigger_event": "pricing_viewed",
  "trigger_op": ">=",
  "trigger_count": 3,
  "window_days": 7,
  "exclude_event": "upgrade_completed",
  "exclude_window_days": 365,
  "cooldown_days": 30,
  "filters": [
    { "key": "plan", "value": "free" }
  ]
}
```

Semántica:

- Seleccionar usuarios que hicieron `trigger_event` `trigger_op`
  `trigger_count` veces en los últimos `window_days`.
- Aplicar hasta 3 filtros opcionales sobre `properties` del evento trigger.
- Excluir usuarios que hicieron `exclude_event` en los últimos
  `exclude_window_days`.
- Excluir usuarios que ya recibieron la misma regla durante `cooldown_days`.

Límites v1:

- `trigger_op`: whitelist `==`, `>=`, `>`, `<=`, `<`.
- `trigger_count`: 1 a 1,000,000.
- `window_days`, `exclude_window_days`, `cooldown_days`: 1 a 365.
- `filters`: máximo 3, usando igualdad exacta sobre `JSONExtractString`.
- `trigger_event` es requerido.
- `exclude_event` es opcional; si está vacío, no se aplica exclusión por
  conversión.

## Webhook config

`automation_rules.webhook` se guarda como JSON String:

```json
{
  "url": "https://example.hubspot-webhook.test/faro",
  "headers": {
    "Authorization": "Bearer ..."
  }
}
```

La v1 no soporta template de body. El body es estable y controlado por Faro para
que los receptores puedan confiar en el contrato.

## Payload

Cada disparo envía:

```json
{
  "type": "faro.automation.triggered",
  "rule_id": "00000000-0000-0000-0000-000000000000",
  "rule_name": "Pricing intent without upgrade",
  "project_id": "default",
  "distinct_id": "user_42",
  "matched_at": "2026-05-24T21:00:00Z",
  "segment": {
    "trigger_event": "pricing_viewed",
    "trigger_count": 4,
    "window_days": 7,
    "exclude_event": "upgrade_completed"
  },
  "user": {
    "properties": {},
    "anonymous_ids": [],
    "sources": []
  }
}
```

`user.properties` se parsea desde el JSON crudo de `product_users.properties`.
Si no es JSON válido, se envía `{}` y se conserva el error en logs, no en el
payload. `anonymous_ids` y `sources` salen del perfil unificado.

## API

Endpoints:

- `GET /api/v1/automations?project=default`
- `POST /api/v1/automations`
- `GET /api/v1/automations/:id`
- `PUT /api/v1/automations/:id`
- `DELETE /api/v1/automations/:id`
- `POST /api/v1/automations/preview`
- `GET /api/v1/automations/:id/deliveries`

`preview` evalúa una definición sin guardar y devuelve tamaño, sample de
`distinct_id` y conteo de usuarios excluidos por conversión/cooldown si la
consulta puede obtenerlos sin costo excesivo. Si no, devuelve size + sample.

## Worker

`automation_runner` corre cada `automation_runner_interval_secs`.

Por cada regla activa:

1. Parsear y validar definition/webhook.
2. Construir query parametrizada contra `product_events`.
3. Agrupar por `distinct_id` y calcular `trigger_count`.
4. Excluir conversions con subquery sobre `exclude_event`.
5. Excluir cooldown con `automation_deliveries`.
6. Limitar candidatos por tick con `automation_runner_max_matches_per_rule`.
7. Leer `product_users FINAL` para enriquecer payload.
8. Enviar webhook por usuario.
9. Insertar delivery `delivered` o `failed`.

Un error de webhook no detiene la regla completa. El worker registra cada fallo
con status code y body truncado. No hay reintentos automáticos en v1; el
cooldown sólo aplica a deliveries exitosas. Esto evita perder usuarios por un
incidente temporal del receptor y deja la política de retry para una iteración
posterior.

## Data flow

1. SDKs envían product events.
2. `user_unifier` mantiene `product_users`.
3. El usuario crea una automation.
4. `automation_runner` evalúa reglas contra eventos recientes.
5. Faro envía webhooks para usuarios candidatos.
6. `automation_deliveries` evita duplicados y da audit trail.

## Errores y seguridad

La API valida longitudes, operadores, ventanas y cantidad de filtros. Los
valores de usuario van siempre como parámetros ClickHouse; sólo los operadores
whitelisteados se interpolan en el SQL.

Los headers de webhook pueden contener secretos. El backend debe devolverlos
redactados en list/detail, preservarlos en update cuando el cliente no mande un
valor nuevo y nunca loguearlos en claro.

El worker debe aplicar timeouts de HTTP y limitar batch size por regla. Las URLs
vacías o inválidas dejan la regla en estado inválido para ese tick y se loguean.

## Frontend

Agregar una pantalla de automations con:

- tabla de reglas por proyecto: nombre, estado, condición resumida, último
  delivery y último error.
- formulario create/edit con campos explícitos para evento trigger, count,
  ventana, evento de exclusión, cooldown y webhook URL.
- botón preview antes de guardar.
- vista de deliveries recientes por regla.

La pantalla debe sentirse operativa y densa, consistente con `/cohorts`,
`/events` y settings. No se necesita landing ni explicación extensa in-app.

## Testing

Seguir TDD en implementación:

- tests unitarios de validación de definición.
- tests unitarios del builder SQL para confirmar placeholders parametrizados y
  whitelist de operadores.
- tests de payload builder.
- tests de redacción de headers secretos.
- tests de cooldown/idempotencia en el query builder o helper de selección.
- tests de API CRUD si el harness existente lo permite.
- tests frontend para serialización de formulario y preview URL helpers.

Verificación final esperada:

- `cargo test` focalizado en backend para automations.
- tests frontend focalizados si se toca UI.
- `cargo fmt` para Rust.

## Fuera de scope

- Integraciones nativas con HubSpot/Salesforce/Customer.io.
- OAuth o secret vault dedicado.
- Builder visual de reglas arbitrarias con AND/OR anidados.
- Reintentos automáticos con backoff.
- Export batch programado tipo CSV/S3.
- Membership table materializada para segmentos.
- Acciones distintas a webhook HTTP.
