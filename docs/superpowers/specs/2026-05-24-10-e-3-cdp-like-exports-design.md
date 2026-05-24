# 10.E.3 CDP-like Exports

- **Fecha:** 2026-05-24
- **Estado:** Futuro / post-v1
- **Scope:** Direccion de producto, no plan de implementacion

## Objetivo

Una vez Faro tiene eventos de producto, perfiles unificados y properties de
usuario, el siguiente paso natural es exportar esos perfiles y segmentos hacia
herramientas comerciales: CRM, email marketing y customer engagement.

Esto mueve Faro hacia una categoria tipo Customer Data Platform / Reverse ETL:
no solo observar comportamiento, sino activar datos de producto en otros
sistemas.

## Contexto

La base ya queda formada por:

- `faro.product_events`: eventos historicos de comportamiento.
- `faro.product_users`: perfil unificado por `distinct_id`, con properties JSON
  enriquecidas por `identify(user_id, traits)`.
- Cohorts con `user_filters`: segmentos como "plan=pro e industry=fintech".
- Automations/webhooks genericos: primera forma de sacar acciones de Faro sin
  construir integraciones nativas.

El spec de Reverse ETL automations deja explicitamente fuera de scope HubSpot,
Salesforce y Customer.io. Este documento captura esa etapa siguiente.

## Decision

No incluir exports CDP-like en v1.

La v1 debe cerrar la cadena interna:

1. Capturar eventos.
2. Resolver identidad y perfiles.
3. Consultar usuarios/cohorts.
4. Disparar webhooks genericos.

Las integraciones nativas se disenan despues, cuando haya suficiente confianza
en el modelo de perfiles, permisos, delivery y auditoria.

## Producto

La unidad de producto seria **Destinations**:

- HubSpot
- Salesforce
- Mailchimp
- Customer.io
- Webhook generico como destino base

Cada destination define:

- credenciales y metodo de autenticacion.
- mapping de campos Faro -> campos externos.
- seleccion de audiencia: todos los usuarios, cohort guardado o query ad hoc.
- modo de sync: manual, programado o incremental.
- estado de entregas, errores y ultimo sync.

## Data Flow Esperado

1. SDKs envian events y identify traits.
2. `user_unifier` mantiene `product_users`.
3. El usuario crea una destination y define mapping.
4. Faro evalua una audiencia desde `product_users` y/o cohorts.
5. Faro envia upserts al sistema externo.
6. Faro guarda resultados de sync para auditoria y retry.

## Requisitos Fuertes

- **Consentimiento y suppression lists:** no exportar usuarios opt-out.
- **Auditoria:** saber que usuario se envio, a que destino y con que estado.
- **Idempotencia:** upsert por identificador estable externo cuando exista.
- **Retries controlados:** backoff y dead-letter para errores persistentes.
- **Rate limits:** cada proveedor tiene cuotas y errores transitorios.
- **Field mapping explicito:** no mandar todo el JSON por defecto.
- **Secret handling:** tokens redacted en API/UI y nunca en logs.

## Arquitectura Futura

Una implementacion madura probablemente necesita:

- `faro.destinations`: configuracion, kind, credenciales cifradas/redactadas,
  project_id, estado, timestamps.
- `faro.destination_mappings`: mapping declarativo o JSON por destination.
- `faro.destination_syncs`: ejecuciones de sync.
- `faro.destination_deliveries`: resultado por usuario y destination.
- Worker `destination_syncer` separado de alerts y automations.
- Adaptadores por proveedor con interfaz comun: validate, upsert_user,
  batch_upsert si el proveedor lo permite.

## Fuera De Scope Por Ahora

- Implementacion de OAuth.
- Secret vault dedicado.
- Sync bidireccional.
- Identity resolution contra IDs externos del CRM.
- UI de mapping visual avanzada.
- Materializar membership tables para todos los cohorts.
- Garantias exactly-once; basta con at-least-once + idempotencia externa.

## Relacion Con Automations

Automations responde: "cuando este usuario haga X, dispara una accion".

Destinations responde: "mantene este conjunto de perfiles sincronizado con una
herramienta externa".

Ambas usan `product_users`, pero no deben ser la misma feature. Automations son
side effects event-driven; destinations son sincronizacion de datos con estado,
mapping y auditoria propia.
