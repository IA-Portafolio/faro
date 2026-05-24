/**
 * Faro para Next.js — lado servidor (instrumentation.ts).
 *
 * Uso (Next.js 13.4+ App Router):
 *
 *   // next.config.mjs
 *   export default { experimental: { instrumentationHook: true } };  // Next 14- only
 *
 *   // instrumentation.ts
 *   export async function register() {
 *     const { registerFaro } = await import('@iaportafolio/nextjs/server');
 *     registerFaro({
 *       endpoint: process.env.FARO_ENDPOINT!,
 *       token: process.env.FARO_TOKEN!,
 *       service: 'mi-next-app',
 *       environment: process.env.NODE_ENV,
 *       release: process.env.VERCEL_GIT_COMMIT_SHA,
 *     });
 *   }
 *
 *   export async function onRequestError(err: unknown, request: { path: string; method: string }) {
 *     const { captureRequestError } = await import('@iaportafolio/nextjs/server');
 *     captureRequestError(err, request);
 *   }
 */

import * as faro from '@iaportafolio/node';

export type ServerOptions = faro.FaroOptions;

let installed = false;

export function registerFaro(opts: ServerOptions): void {
  if (installed) return;
  // El Edge Runtime es un proceso distinto; solo inicializamos en el runtime de Node.
  // Next expone process.env.NEXT_RUNTIME = 'nodejs' | 'edge'.
  if (process.env.NEXT_RUNTIME && process.env.NEXT_RUNTIME !== 'nodejs') {
    return;
  }
  faro.init(opts);
  installed = true;
}

/**
 * Engánchalo al hook de instrumentación `onRequestError` de Next.js (Next 15+).
 * Reporta el error con el contexto de la request adjunto como tags.
 */
export function captureRequestError(
  err: unknown,
  request: { path?: string; method?: string; routerKind?: string },
): void {
  if (!installed) return;
  faro.captureException(err, {
    tags: {
      'http.path': request.path ?? '',
      'http.method': request.method ?? '',
      'next.router': request.routerKind ?? '',
    },
  });
}

export { faro };

// Helpers de tracking accesibles a través del namespace `faro`:
//   import { faro } from '@iaportafolio/nextjs/server';
//   faro.track('checkout_completed', { amount: 99.5 });
// No los re-exportamos como `track`/`identify`/`alias` nombrados aquí porque
// chocarían con los re-exports del cliente cuando alguien importe el barrel
// principal (`from '@iaportafolio/nextjs'`). El cliente sí los expone flat
// porque es el caso de uso más frecuente (RUM en componentes).
