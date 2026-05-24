/**
 * Faro para Next.js — lado cliente (corre en el navegador).
 *
 * Punto de entrada público para el RUM en Next.js:
 *  - captura de window.error / unhandledrejection
 *  - Web Vitals (LCP/CLS/INP/FCP/TTFB)
 *  - breadcrumbs de clicks y navegaciones (history.pushState/popstate)
 *  - sendBeacon en pagehide / visibilitychange=hidden (no se pierden eventos)
 *  - ErrorBoundary React (`<FaroErrorBoundary>`)
 *  - auto-detección de release desde env vars típicas de Vercel/Next
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
 */

import {
  init as initBrowser,
  type FaroBrowser,
  type FaroBrowserOptions,
} from './browser-core';

export type {
  FaroBrowserOptions,
  UserContext,
  Breadcrumb,
  LogEntry,
  Severity,
  WireEvent,
  FaroBrowser,
} from './browser-core';

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
} from './browser-core';

export { FaroErrorBoundary } from './browser-react';
export type { FaroErrorBoundaryProps } from './browser-react';

/**
 * Inicializa el RUM en el navegador. Seguro de llamar en SSR — si `typeof window === 'undefined'`
 * el core no hace nada. Llámalo desde `useEffect` en un componente 'use client'.
 */
export function initFaroClient(opts: FaroBrowserOptions): FaroBrowser {
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
