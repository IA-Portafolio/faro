/**
 * Faro for Next.js — server side (instrumentation.ts).
 *
 * Usage (Next.js 13.4+ App Router):
 *
 *   // next.config.mjs
 *   export default { experimental: { instrumentationHook: true } };  // Next 14- only
 *
 *   // instrumentation.ts
 *   export async function register() {
 *     const { registerFaro } = await import('@faro/nextjs/server');
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
 *     const { captureRequestError } = await import('@faro/nextjs/server');
 *     captureRequestError(err, request);
 *   }
 */

import * as faro from '@faro/node';

export type ServerOptions = faro.FaroOptions;

let installed = false;

export function registerFaro(opts: ServerOptions): void {
  if (installed) return;
  // The Edge Runtime is a different process; we only initialise in Node runtime.
  // Next exposes process.env.NEXT_RUNTIME = 'nodejs' | 'edge'.
  if (process.env.NEXT_RUNTIME && process.env.NEXT_RUNTIME !== 'nodejs') {
    return;
  }
  faro.init(opts);
  installed = true;
}

/**
 * Bind this to Next.js's `onRequestError` instrumentation hook (Next 15+).
 * Reports the error with the request context attached as tags.
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
