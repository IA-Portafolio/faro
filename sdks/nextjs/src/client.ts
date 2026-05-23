/**
 * Faro for Next.js — client side (runs in the browser).
 *
 * Usage:
 *
 *   // app/faro-client.tsx
 *   'use client';
 *   import { useEffect } from 'react';
 *   import { initFaroClient } from '@iaportafolio/nextjs/client';
 *
 *   export function FaroClient() {
 *     useEffect(() => {
 *       initFaroClient({
 *         endpoint: process.env.NEXT_PUBLIC_FARO_ENDPOINT!,
 *         token:    process.env.NEXT_PUBLIC_FARO_TOKEN!,
 *         service:  'mi-next-app-web',
 *       });
 *     }, []);
 *     return null;
 *   }
 *
 *   // and render <FaroClient /> inside app/layout.tsx
 */

import type { FaroOptions } from '@iaportafolio/node';

let client: ReturnType<typeof newClient> | null = null;

interface BrowserClient {
  log(entry: { level?: string; message: string; attributes?: Record<string, unknown> }): void;
  info(msg: string, attrs?: Record<string, unknown>): void;
  warn(msg: string, attrs?: Record<string, unknown>): void;
  error(msg: string, attrs?: Record<string, unknown>): void;
  captureException(err: unknown, ctx?: { tags?: Record<string, string>; message?: string }): void;
  flush(): Promise<void>;
}

function newClient(opts: FaroOptions): BrowserClient {
  const endpoint = opts.endpoint.replace(/\/$/, '');
  const queue: unknown[] = [];
  const maxBatch = opts.maxBatchSize ?? 100;
  let pendingTimer: ReturnType<typeof setTimeout> | null = null;

  function scheduleFlush(): void {
    if (pendingTimer) return;
    pendingTimer = setTimeout(() => {
      pendingTimer = null;
      void flush();
    }, opts.flushIntervalMs ?? 1500);
  }

  async function flush(): Promise<void> {
    if (queue.length === 0) return;
    const batch = queue.splice(0, maxBatch);
    try {
      // sendBeacon copes with `pagehide` better than fetch.
      const body = JSON.stringify({ service: opts.service, logs: batch });
      const ok =
        typeof navigator !== 'undefined' &&
        typeof navigator.sendBeacon === 'function' &&
        document.visibilityState === 'hidden'
          ? navigator.sendBeacon(
              `${endpoint}/api/v1/ingest/logs?_token=${encodeURIComponent(opts.token)}`,
              new Blob([body], { type: 'application/json' }),
            )
          : false;
      if (ok) return;
      const res = await fetch(`${endpoint}/api/v1/ingest/logs`, {
        method: 'POST',
        keepalive: true,
        headers: {
          'Authorization': `Bearer ${opts.token}`,
          'Content-Type': 'application/json',
        },
        body,
      });
      if (!res.ok) {
        // Best-effort re-queue if not a 4xx (the request is unrecoverable then).
        if (res.status >= 500) queue.unshift(...batch);
      }
    } catch (_e) {
      queue.unshift(...batch);
    }
  }

  function enqueue(level: string, message: string, attributes?: Record<string, unknown>): void {
    const attrs: Record<string, string> = {};
    if (opts.attributes) {
      for (const [k, v] of Object.entries(opts.attributes)) attrs[k] = String(v);
    }
    if (opts.environment) attrs['deployment.environment'] = opts.environment;
    if (opts.release) attrs['service.version'] = opts.release;
    if (typeof window !== 'undefined') {
      attrs['browser.url'] = window.location.href;
      attrs['browser.userAgent'] = navigator.userAgent;
    }
    if (attributes) {
      for (const [k, v] of Object.entries(attributes)) {
        attrs[k] = typeof v === 'string' ? v : JSON.stringify(v);
      }
    }
    queue.push({
      level,
      message,
      timestamp: new Date().toISOString(),
      attributes: attrs,
    });
    if (queue.length >= maxBatch) void flush();
    else scheduleFlush();
  }

  return {
    log: (e) => enqueue(e.level ?? 'INFO', e.message, e.attributes),
    info: (m, a) => enqueue('INFO', m, a),
    warn: (m, a) => enqueue('WARN', m, a),
    error: (m, a) => enqueue('ERROR', m, a),
    captureException: (err, ctx) => {
      const e = err instanceof Error ? err : new Error(typeof err === 'string' ? err : JSON.stringify(err));
      enqueue('ERROR', ctx?.message ?? `${e.name}: ${e.message}`, {
        'exception.type': e.name,
        'exception.message': e.message,
        'exception.stacktrace': e.stack ?? '',
        ...(ctx?.tags ?? {}),
      });
    },
    flush,
  };
}

export function initFaroClient(opts: FaroOptions): BrowserClient {
  if (typeof window === 'undefined') {
    // Server-side render — return a no-op so the same import works.
    return {
      log() {}, info() {}, warn() {}, error() {}, captureException() {}, flush: async () => undefined,
    };
  }
  client = newClient(opts);

  // Capture unhandled errors in the browser.
  window.addEventListener('error', (ev) => {
    client?.captureException(ev.error ?? ev.message, { tags: { origin: 'window.error' } });
  });
  window.addEventListener('unhandledrejection', (ev) => {
    client?.captureException(ev.reason, { tags: { origin: 'unhandledrejection' } });
  });
  // Flush when the tab hides.
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') void client?.flush();
  });
  window.addEventListener('pagehide', () => void client?.flush());

  return client;
}

export function faroClient(): BrowserClient {
  if (!client) throw new Error('initFaroClient() must be called before use');
  return client;
}
