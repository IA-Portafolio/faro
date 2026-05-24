# Revision 10.J observability + product analytics

Fecha: 2026-05-24

Alcance revisado:

- `GET /api/v1/insights/revenue-impact`
- `GET /api/v1/insights/latency-funnel-impact`
- `GET /api/v1/insights/web-vitals-conversion-impact`
- Metricas virtuales `events.<event_name>.count` en `/api/v1/metrics/names` y
  `/api/v1/metrics/series`
- Integracion minima de `/metrics` frontend para graficar eventos como series

## Hallazgos

No quedan hallazgos bloqueantes en el alcance revisado.

### P3 - Las metricas derivadas de eventos son solo conteos por ahora

Archivo: `backend/src/api/metrics.rs`

Las metricas virtuales `events.<event_name>.count` resuelven correctamente
conteos por bucket desde `faro.product_events`. Esto cubre casos como
`checkout_completed` por hora y permite poner eventos de negocio junto a
metricas tecnicas.

Limitacion:

- No calcula aun sumas de revenue desde `properties.amount`.
- No calcula rates derivados como `checkout_completed / checkout_started`.
- No calcula percentiles o distribuciones de propiedades JSON.

Recomendacion:

- Mantener este primer contrato como `count`.
- Agregar una capa declarativa posterior para metricas de evento con
  `operation = count | sum | avg | p95 | ratio` y `property` opcional.

### P3 - La comparacion de latency funnel es correlacion por bucket

Archivo: `backend/src/api/insights.rs`

`latency-funnel-impact` compara p95 de spans y conversion del funnel por bucket
temporal. Es robusto para dashboards y priorizacion, pero no prueba causalidad
por request individual.

Recomendacion:

- Mantener el texto del producto como correlacion operacional.
- Agregar evidencia por `trace_id` cuando `product_events.trace_id` este
  poblado consistentemente desde el SDK.

### P3 - Web Vitals depende de `session.id` en logs

Archivo: `backend/src/api/insights.rs`

`web-vitals-conversion-impact` une `faro.logs` con `faro.product_events` por
`session.id` o `session_id`. El SDK Next.js ya adjunta `session.id`; otros SDKs
o integraciones custom deben hacer lo mismo para que el insight funcione.

Recomendacion:

- Documentar `metric.name`, `metric.value` y `session.id` como contrato de RUM.
- En SDKs futuros, emitir tambien un evento de producto opcional para Web Vitals
  si se quiere analizar performance a nivel de pageview especifico.

## Verificacion ejecutada

Pasaron:

- `rustfmt --edition 2021 --check` para archivos backend tocados.
- `cargo check`.
- `cargo test api::insights::tests`.
- `cargo test api::metrics::tests`.
- `cargo test --test revenue_impact_insights`.
- `cargo test --test latency_funnel_impact`.
- `cargo test --test web_vitals_conversion_impact`.
- `cargo test --test event_derived_metrics`.

No paso por causas fuera de este alcance:

- `npm run check` en frontend falla por errores preexistentes:
  `vite.config.ts` no encuentra tipos de Node y hay checks de `editing`
  posiblemente null en `routes/monitors` y `routes/settings/alerts`.
  No aparecieron errores nuevos en `routes/metrics/+page.svelte`.

## Documentacion actualizada

- `docs/product-analytics.md` documenta:
  - endpoints 10.J.1, 10.J.2 y 10.J.3;
  - preguntas que responde cada insight;
  - contrato `events.<event_name>.count`;
  - semantica de `service` como `product_events.source` para metricas virtuales;
  - limitaciones actuales de metricas derivadas.
