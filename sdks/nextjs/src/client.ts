/**
 * Faro para Next.js — lado cliente (corre en el navegador).
 *
 * Es un wrapper fino sobre @iaportafolio/browser. El core (captura de
 * window.error, Web Vitals, breadcrumbs, batching, sendBeacon en pagehide,
 * ErrorBoundary React) vive en el paquete browser. Aquí sólo añadimos:
 *  - auto-detección de la release desde env vars típicas de Vercel/Next
 *  - re-exports para que sea ergonómico (`import {...} from '@iaportafolio/nextjs/client'`)
 *
 * Uso típico (App Router):
 *
 *   // app/faro-client.tsx
 *   'use client';
 *   import { useEffect } from 'react';
 *   import { usePathname, useSearchParams } from 'next/navigation';
 *   import { initFaroClient, addBreadcrumb } from '@iaportafolio/nextjs/client';
 *
 *   export function FaroClient() {
 *     const pathname = usePathname();
 *     const search = useSearchParams();
 *
 *     useEffect(() => {
 *       initFaroClient({
 *         endpoint: process.env.NEXT_PUBLIC_FARO_ENDPOINT!,
 *         token:    process.env.NEXT_PUBLIC_FARO_TOKEN!,
 *         service:  'mi-next-app-web',
 *       });
 *     }, []);
 *
 *     // (opcional) breadcrumb explícito en cada route change con el pathname limpio.
 *     // El SDK ya captura pushState, esto sólo es más legible en el dashboard.
 *     useEffect(() => {
 *       addBreadcrumb({ category: 'navigation', message: pathname, data: { pathname } });
 *     }, [pathname, search]);
 *
 *     return null;
 *   }
 *
 *   // app/layout.tsx
 *   import { FaroClient } from './faro-client';
 *   <body><FaroClient />{children}</body>
 *
 * Y opcionalmente envuelve secciones críticas con ErrorBoundary:
 *
 *   import { FaroErrorBoundary } from '@iaportafolio/nextjs/client';
 *   <FaroErrorBoundary fallback={...}><Checkout /></FaroErrorBoundary>
 */

import {
  init as initBrowser,
  type FaroBrowser,
  type FaroBrowserOptions,
} from '@iaportafolio/browser';

export type {
  FaroBrowserOptions,
  UserContext,
  Breadcrumb,
  LogEntry,
  Severity,
  WireEvent,
} from '@iaportafolio/browser';

export {
  log,
  info,
  warn,
  error,
  captureException,
  setUser,
  addBreadcrumb,
  flush,
  close,
  getClient,
} from '@iaportafolio/browser';

export { FaroErrorBoundary } from '@iaportafolio/browser/react';
export type { FaroErrorBoundaryProps } from '@iaportafolio/browser/react';

/**
 * Inicializa el SDK browser. Seguro de llamar en SSR — si `typeof window === 'undefined'`
 * el SDK subyacente no hace nada. Llámalo desde `useEffect` en un componente 'use client'.
 */
export function initFaroClient(opts: FaroBrowserOptions): FaroBrowser {
  // Auto-detect release desde env vars típicas de Vercel/Next si no se pasó explícita.
  let release = opts.release;
  if (!release && typeof process !== 'undefined' && process.env) {
    release =
      process.env.NEXT_PUBLIC_VERCEL_GIT_COMMIT_SHA ||
      process.env.NEXT_PUBLIC_GIT_COMMIT_SHA ||
      process.env.NEXT_PUBLIC_VERSION ||
      undefined;
  }
  return initBrowser({ ...opts, release });
}
