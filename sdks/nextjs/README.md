# @iaportafolio/nextjs

SDK para Next.js — App Router y Pages Router. Cubre ambos lados:

- **Server**: captura errores de Route Handlers, Server Actions y SSR. Vive sobre [`@iaportafolio/node`](../node/).
- **Client (browser)**: RUM completo — Web Vitals (LCP/CLS/INP/FCP/TTFB), `window.error`, `unhandledrejection`, clicks/navegaciones como breadcrumbs, React `<ErrorBoundary>` y flush garantizado al cerrar el tab. Vive sobre [`@iaportafolio/browser`](../browser/).

```bash
npm install @iaportafolio/nextjs @iaportafolio/node @iaportafolio/browser
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

### Apagar comportamientos

`initFaroClient({ captureWebVitals: false, captureClicks: false, ... })` — todos los flags están en [`@iaportafolio/browser`](../browser/).

## Variables de entorno

| Var                               | Dónde              | Para qué                                  |
| --------------------------------- | ------------------ | ----------------------------------------- |
| `FARO_ENDPOINT`                   | solo servidor      | URL base                                  |
| `FARO_TOKEN`                      | solo servidor      | Token de proyecto (privado)               |
| `NEXT_PUBLIC_FARO_ENDPOINT`       | cliente + servidor | URL base para el navegador                |
| `NEXT_PUBLIC_FARO_TOKEN`          | cliente + servidor | **Mismo token de proyecto.** Queda expuesto en el bundle — es deliberado, igual que en Sentry: el token solo permite ingerir, no leer datos del dashboard. |

## Changelog

- **v0.2.0**: RUM completo (Web Vitals, breadcrumbs, ErrorBoundary, setUser, navigation tracking). El cliente ahora se apoya en `@iaportafolio/browser`.
- **v0.1.x**: captura básica de errores en el cliente.
