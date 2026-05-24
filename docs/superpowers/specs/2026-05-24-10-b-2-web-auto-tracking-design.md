# 10.B.2 Web Auto Tracking Design

- **Fecha:** 2026-05-24
- **Estado:** Aprobado para implementacion
- **Scope:** SDK browser de `@iaportafolio/nextjs`, reutilizable por un futuro SDK Svelte/vanilla

## Objetivo

Agregar auto-tracking opt-in al SDK browser:

```ts
faro.init({
  autoCapture: {
    pageViews: true,
    clicks: true,
    formSubmissions: true,
    rageClicks: true,
    deadClicks: true,
  },
});
```

Esto debe producir product events en `/api/v1/ingest/events`, no breadcrumbs.

## Decisiones

`autoCapture` es independiente de `captureClicks` y `captureNavigation`. Los flags existentes siguen generando breadcrumbs para compatibilidad; `autoCapture` genera eventos de producto.

Eventos:

- `pageViews`: usa `page(path, properties)` en init y en `pushState`, `replaceState`, `popstate`, `hashchange`.
- `clicks`: usa `track("$autocapture", properties)` para `[data-faro]`, `<button>` y `<a>`.
- `formSubmissions`: usa `track("$form_submit", properties)` solo para `form[data-faro-form]`.
- `rageClicks`: usa `track("$rage_click", properties)` cuando hay 3+ clicks en menos de 2s sobre el mismo elemento.
- `deadClicks`: usa `track("$dead_click", properties)` cuando un click elegible no causa cambio de URL ni mutacion de DOM dentro de una ventana corta.

Los eventos incluyen propiedades compactas y no sensibles: `type`, `tag`, `id`, `text`, `href`, `faro`, `path`, `url`, `navigation_type`, `click_count` segun aplique.

## Testing

Seguir TDD en `sdks/nextjs/test/browser.test.mjs` con stubs de DOM/eventos:

- `autoCapture` vacio no emite eventos extra.
- `pageViews` emite page view inicial y al navegar con History API.
- `clicks` captura solo elementos elegibles.
- `formSubmissions` captura solo formularios con `data-faro-form`.
- `rageClicks` dispara al tercer click en la ventana.
- `deadClicks` dispara si no hay mutacion ni cambio de URL.

Verificacion: `cd sdks/nextjs && npm test`.
