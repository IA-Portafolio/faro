# @iaportafolio/browser

SDK browser para Faro. Captura errores no manejados, Web Vitals, navegaciones y clicks como breadcrumbs, envía todo en lotes y hace flush con `sendBeacon` cuando el tab se cierra (sin perder eventos).

Pensado para apps web vanilla (Vue, Svelte, HTML puro, etc.). Si usas Next.js prefiere [`@iaportafolio/nextjs`](../nextjs/) que envuelve este core y añade integración con el router de Next.

## Instalación

```bash
npm install @iaportafolio/browser
```

## Uso mínimo

```ts
import { init, captureException, setUser } from '@iaportafolio/browser';

init({
  endpoint: 'https://faro.iaportafolio.com',
  token: process.env.PUBLIC_FARO_TOKEN!,
  service: 'mi-app-web',
  environment: import.meta.env.MODE,
  release: __APP_VERSION__,
});

// Después del login
setUser({ id: user.id, email: user.email });

// Errores manuales
try {
  await pagar(carro);
} catch (e) {
  captureException(e, { tags: { flow: 'checkout' } });
}
```

A partir del `init()`, **window.error**, **unhandledrejection**, **Web Vitals** (LCP/CLS/INP/FCP/TTFB), **clicks** y **navegaciones SPA** se capturan solos.

## Opciones

```ts
init({
  endpoint, token, service,                       // obligatorios
  environment, release,                           // opcional
  attributes: { region: 'eu-west-1' },            // default attrs
  flushIntervalMs: 2000,                          // cadencia de flush
  maxBatchSize: 100,                              // events por POST
  maxQueueSize: 2000,                             // drop si se satura
  maxBreadcrumbs: 30,                             // ring buffer
  captureUnhandled: true,                         // window.error + unhandledrejection
  captureConsole: false,                          // intercepta console.error/warn (cuidado: ruidoso)
  captureWebVitals: true,                         // LCP/CLS/INP/FCP/TTFB
  captureClicks: true,                            // breadcrumbs de clicks
  captureNavigation: true,                        // breadcrumbs de history.pushState
  beforeSend: (evt) => evt.message.includes('cancel') ? null : evt,  // muestreo / redacción
});
```

## React Error Boundary

```tsx
import { FaroErrorBoundary } from '@iaportafolio/browser/react';

<FaroErrorBoundary
  fallback={({ error, reset }) => (
    <div>
      <h1>Algo se rompió</h1>
      <pre>{error.message}</pre>
      <button onClick={reset}>Reintentar</button>
    </div>
  )}
  tags={{ module: 'checkout' }}
>
  <Checkout />
</FaroErrorBoundary>
```

El error se reporta a Faro con `origin=react.error-boundary` y todos los breadcrumbs acumulados.

## Breadcrumbs manuales

```ts
import { addBreadcrumb } from '@iaportafolio/browser';

addBreadcrumb({
  category: 'custom',
  message: 'usuario agregó producto al carro',
  data: { product_id: 'sku-42', qty: 2 }
});
```

Los últimos N breadcrumbs viajan automáticamente con cada evento en el atributo `breadcrumbs` (JSON-encoded).

## Identificar al usuario

```ts
setUser({ id: '42', email: 'user@example.com', username: 'alice' });
// Se serializa a `user.id`, `user.email`, `user.name` en cada evento posterior.

setUser(null);  // al hacer logout
```

## Notas

- **El token vive en el bundle** — es deliberado, igual que en Sentry. El token de ingesta solo permite ENVIAR, no leer datos. Si lo filtran, rótalo desde `/projects` en el dashboard de Faro.
- **Web Vitals** se importa dinámicamente (`import('web-vitals')`) — si no instalas la dep o el bundler la elimina, simplemente se omiten esas métricas sin error.
- **sendBeacon** se usa al `pagehide`/`visibilitychange=hidden` para no perder eventos al cerrar el tab. El token va como query param `?_token=...` porque sendBeacon no soporta headers.
