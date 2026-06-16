# @iaportafolio/nextjs

SDK para Next.js — App Router y Pages Router. Cubre ambos lados en un único paquete:

- **Server**: captura errores de Route Handlers, Server Actions y SSR. Usa [`@iaportafolio/node`](../node/) como dependencia peer.
- **Client (browser)**: RUM completo — Web Vitals (LCP/CLS/INP/FCP/TTFB), `window.error`, `unhandledrejection`, clicks/navegaciones como breadcrumbs, React `<FaroErrorBoundary>` y flush garantizado al cerrar el tab. Todo el código vive dentro de este mismo paquete.

> **Perfil de defaults:** el subimport `/server` usa el perfil `server` (750ms · 200 · 10 000) heredado de [`@iaportafolio/node`](../node/); el subimport cliente usa el perfil `browser` (2000ms · 100 · 2 000). Ver [perfiles](../README.md#perfiles-de-defaults).

```bash
npm install @iaportafolio/nextjs @iaportafolio/node
```

## Server-side

```ts
// instrumentation.ts (en la raíz del proyecto, junto a app/)
export async function register() {
  const { registerFaro } = await import('@iaportafolio/nextjs/server');
  registerFaro({
    endpoint: process.env.FARO_ENDPOINT!,
    token:    process.env.FARO_TOKEN!,
    service:  'mi-next-app',
    environment: process.env.NODE_ENV,
    release:  process.env.VERCEL_GIT_COMMIT_SHA,
  });
}

// Next 15+: hook nativo de errores de request.
export async function onRequestError(err: unknown, request: { path: string; method: string }) {
  const { captureRequestError } = await import('@iaportafolio/nextjs/server');
  captureRequestError(err, request);
}
```

En cualquier ruta server:

```ts
import { faro } from '@iaportafolio/nextjs/server';

export async function POST(req: Request) {
  try {
    return await procesar(req);
  } catch (e) {
    faro.captureException(e, { tags: { route: '/api/charge' } });
    return new Response('fallo', { status: 500 });
  }
}
```

## Client-side (RUM completo)

```tsx
// app/faro-client.tsx
'use client';
import { useEffect } from 'react';
import { usePathname, useSearchParams } from 'next/navigation';
import { initFaroClient, addBreadcrumb, setUser } from '@iaportafolio/nextjs/client';

export function FaroClient() {
  const pathname = usePathname();
  const search = useSearchParams();

  useEffect(() => {
    initFaroClient({
      endpoint: process.env.NEXT_PUBLIC_FARO_ENDPOINT!,
      token:    process.env.NEXT_PUBLIC_FARO_TOKEN!,
      service:  'mi-next-app-web',
      // release se autodetecta desde NEXT_PUBLIC_VERCEL_GIT_COMMIT_SHA si no la pasas
      scrubUrlQuery: true,   // default: remueve querystring+hash de la URL reportada (PII)
      sessionReplaySampleRate: 1.0,  // 0-1, fracción de sesiones con replay
    });
  }, []);

  // Breadcrumb explícito por ruta — el SDK ya rastrea pushState/popstate,
  // pero esto da una entrada limpia con el pathname de Next.
  useEffect(() => {
    addBreadcrumb({ category: 'navigation', message: pathname, data: { pathname } });
  }, [pathname, search]);

  return null;
}

// app/layout.tsx
import { FaroClient } from './faro-client';
export default function RootLayout({ children }) {
  return (
    <html>
      <body>
        <FaroClient />
        {children}
      </body>
    </html>
  );
}
```

### Identificar al usuario

Tras hacer login:

```ts
import { setUser } from '@iaportafolio/nextjs/client';
setUser({ id: user.id, email: user.email });
```

Todos los eventos siguientes incluyen `user.id`, `user.email`.

### Feature flags

Calienta la cache en el cliente y evalúa localmente. El SDK vuelve a pedir flags cada 30s.

```tsx
'use client';
import { useEffect, useState } from 'react';
import { refreshFeatureFlags, isFeatureEnabled } from '@iaportafolio/nextjs/client';

export function CheckoutGate({ user }) {
  const [enabled, setEnabled] = useState(false);

  useEffect(() => {
    void refreshFeatureFlags().then(() => {
      setEnabled(isFeatureEnabled('new-checkout', {
        distinct_id: user.id,
        properties: { plan: user.plan },
      }));
    });
  }, [user.id, user.plan]);

  return enabled ? <NewCheckout /> : <Checkout />;
}
```

Las reglas de flags se descargan al navegador; no pongas secretos en `conditions`.

### React Error Boundary

Envuelve secciones críticas para capturar errores de render sin reventar la app entera:

```tsx
'use client';
import { FaroErrorBoundary } from '@iaportafolio/nextjs/client';

export default function CheckoutPage() {
  return (
    <FaroErrorBoundary
      tags={{ module: 'checkout' }}
      fallback={({ error, reset }) => (
        <div>
          <h1>Algo se rompió en el checkout</h1>
          <pre>{error.message}</pre>
          <button onClick={reset}>Reintentar</button>
        </div>
      )}
    >
      <Checkout />
    </FaroErrorBoundary>
  );
}
```

### Qué captura automáticamente

| Cosa | Cómo |
| --- | --- |
| Errores no atrapados | `window.onerror` y `unhandledrejection` |
| Errores de React | `<FaroErrorBoundary>` (manual) |
| **Web Vitals** | LCP, CLS, INP, FCP, TTFB enviados como logs con `metric.name`/`metric.value` |
| Clicks | Breadcrumb con tag + id + texto del elemento |
| Navegaciones | Breadcrumb en cada `history.pushState`/`popstate` |
| Contexto | `browser.url`, `browser.userAgent`, `user.*` (si llamas `setUser`) |
| Flush al cerrar tab | `navigator.sendBeacon` en `pagehide`/`visibilitychange=hidden` |
| Session replay (opt-in) | Graba el DOM con rrweb y lo asocia a errores por `session.id`. Ver [SESSION-REPLAY.md](./SESSION-REPLAY.md) |

### Auto-tracking de producto (opt-in)

Además de los breadcrumbs de RUM, el cliente puede emitir product events automáticamente:

```ts
initFaroClient({
  endpoint, token, service,
  autoCapture: {
    pageViews: true,        // init, pushState, replaceState, popstate, hashchange
    clicks: true,           // [data-faro], button, a
    formSubmissions: true,  // form[data-faro-form]
    rageClicks: true,       // 3+ clicks en <2s sobre el mismo elemento
    deadClicks: true,       // click elegible sin cambio de URL ni DOM
  },
});
```

Eventos emitidos:

| Opción | Evento |
| --- | --- |
| `pageViews` | `page(path, { navigation_type, url, path, referrer })` |
| `clicks` | `track('$autocapture', { type: 'click', tag, id, text, href, faro })` |
| `formSubmissions` | `track('$form_submit', { type: 'form_submit', id, faro_form })` |
| `rageClicks` | `track('$rage_click', { type: 'rage_click', click_count, ... })` |
| `deadClicks` | `track('$dead_click', { type: 'dead_click', wait_ms, ... })` |

`autoCapture` está apagado por defecto. `captureClicks` y `captureNavigation`
siguen siendo breadcrumbs; no se convierten en product events salvo que actives
`autoCapture`.

### Identidad estable

El cliente browser genera `anonymous_id` con `crypto.randomUUID()` y lo persiste
en `localStorage`. En el primer `identify('user_42')`, emite automáticamente
`$alias` con `{ from: anonymous_id, to: 'user_42' }`; desde ese punto, todos los
product events llevan `distinct_id='user_42'` y mantienen el `anonymous_id`
original para joins retrospectivos.

### Apagar comportamientos

```ts
initFaroClient({
  endpoint, token, service,
  captureWebVitals: false,
  captureClicks: false,
  captureNavigation: false,
  captureUnhandled: false, // si quieres reportar a mano
  captureConsole: true,    // por defecto false (puede meter ruido)
});
```

## Opciones cross-SDK

`warning()` (alias de `warn()`), `scrubFields`/`scrubHeaders`/`scrubPatterns` y el hook `beforeSend` están disponibles tanto en el cliente browser como (vía `@iaportafolio/node`) en el servidor, con la misma semántica que en el resto de SDKs. Ver [API uniforme entre SDKs](../README.md#api-uniforme-entre-sdks).

## Variables de entorno

| Var                               | Dónde              | Para qué                                  |
| --------------------------------- | ------------------ | ----------------------------------------- |
| `FARO_ENDPOINT`                   | solo servidor      | URL base                                  |
| `FARO_TOKEN`                      | solo servidor      | Token de proyecto (privado)               |
| `NEXT_PUBLIC_FARO_ENDPOINT`       | cliente + servidor | URL base para el navegador                |
| `NEXT_PUBLIC_FARO_TOKEN`          | cliente + servidor | **Mismo token de proyecto.** Queda expuesto en el bundle — es deliberado, igual que en Sentry: el token solo permite ingerir, no leer datos del dashboard. |

## Changelog

- **v0.3.0**: el RUM del cliente se vuelve a fusionar dentro de `@iaportafolio/nextjs`. Ya no hay que instalar `@iaportafolio/browser` por separado (ese paquete queda deprecado). API pública sin cambios — sigue funcionando lo que estaba en v0.2.x.
- **v0.2.x**: RUM completo en un paquete aparte `@iaportafolio/browser` (retirado, no usar).
- **v0.1.x**: captura básica de errores en el cliente.
