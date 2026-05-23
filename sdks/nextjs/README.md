# @faro/nextjs

SDK para Next.js (App Router y Pages Router). Tiene dos mitades: **server** (Node runtime, captura errores de Route Handlers / Server Actions / SSR) y **client** (browser, captura `window.onerror` + `unhandledrejection` + Web Vitals manuales).

```bash
npm install @faro/nextjs @faro/node
```

## Server-side

```ts
// instrumentation.ts (en la raíz del proyecto, junto a app/)
export async function register() {
  const { registerFaro } = await import('@faro/nextjs/server');
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
  const { captureRequestError } = await import('@faro/nextjs/server');
  captureRequestError(err, request);
}
```

En cualquier ruta server:

```ts
import { faro } from '@faro/nextjs/server';

export async function POST(req: Request) {
  try {
    return await procesar(req);
  } catch (e) {
    faro.captureException(e, { tags: { route: '/api/charge' } });
    return new Response('failed', { status: 500 });
  }
}
```

## Client-side (browser)

```tsx
// app/faro-client.tsx
'use client';
import { useEffect } from 'react';
import { initFaroClient } from '@faro/nextjs/client';

export function FaroClient() {
  useEffect(() => {
    initFaroClient({
      endpoint: process.env.NEXT_PUBLIC_FARO_ENDPOINT!,
      token:    process.env.NEXT_PUBLIC_FARO_TOKEN!,
      service:  'mi-next-app-web',
    });
  }, []);
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

El cliente:
- Registra `window.onerror` y `unhandledrejection`.
- Hace `flush` automático al `visibilitychange=hidden` y `pagehide` (usa `navigator.sendBeacon` cuando está disponible).
- Adjunta `browser.url` y `browser.userAgent` a cada evento.

## Variables de entorno

| Var                               | Dónde         | Para qué                                  |
| --------------------------------- | ------------- | ----------------------------------------- |
| `FARO_ENDPOINT`                   | server only   | Base URL                                  |
| `FARO_TOKEN`                      | server only   | Token de proyecto (privado)               |
| `NEXT_PUBLIC_FARO_ENDPOINT`       | client+server | Base URL para el browser                  |
| `NEXT_PUBLIC_FARO_TOKEN`          | client+server | **Mismo token de proyecto**. Sí queda expuesto en el bundle — es deliberado, igual que en Sentry: el token solo permite ingerir, no leer datos del dashboard. |
