/**
 * Faro SDK for Expo / React Native.
 *
 * Uses fetch (RN ships it) and ErrorUtils.setGlobalHandler for native
 * uncaught-error capture. No native modules → works on Expo Go without
 * a custom development client.
 */

export type Severity = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR' | 'FATAL';

export interface FaroExpoOptions {
  endpoint: string;
  token: string;
  service: string;
  environment?: string;
  release?: string;
  attributes?: Record<string, string | number | boolean>;
  flushIntervalMs?: number;
  maxBatchSize?: number;
  maxQueueSize?: number;
  installGlobalHandlers?: boolean;
}

// ErrorUtils is a React Native global, not in normal TS types.
declare const ErrorUtils:
  | {
      setGlobalHandler: (h: (err: Error, isFatal?: boolean) => void) => void;
      getGlobalHandler: () => (err: Error, isFatal?: boolean) => void;
    }
  | undefined;

interface Wire {
  level: Severity;
  message: string;
  timestamp: string;
  attributes: Record<string, string>;
}

class FaroExpoClient {
  private queue: Wire[] = [];
  private timer: ReturnType<typeof setInterval> | null = null;
  private closed = false;
  private prevHandler: ((err: Error, isFatal?: boolean) => void) | null = null;

  constructor(private opts: Required<Omit<FaroExpoOptions, 'attributes' | 'environment' | 'release'>> & Pick<FaroExpoOptions, 'attributes' | 'environment' | 'release'>) {
    this.timer = setInterval(() => void this.flush(), this.opts.flushIntervalMs);
    if (this.opts.installGlobalHandlers) this.installHandlers();
  }

  log(entry: { level?: Severity; message: string; attributes?: Record<string, unknown> }): void {
    if (this.closed) return;
    const attrs: Record<string, string> = {};
    if (this.opts.attributes) {
      for (const [k, v] of Object.entries(this.opts.attributes)) attrs[k] = String(v);
    }
    if (this.opts.environment) attrs['deployment.environment'] = this.opts.environment;
    if (this.opts.release) attrs['service.version'] = this.opts.release;
    if (entry.attributes) {
      for (const [k, v] of Object.entries(entry.attributes)) {
        attrs[k] = typeof v === 'string' ? v : JSON.stringify(v);
      }
    }
    if (this.queue.length >= this.opts.maxQueueSize) return; // drop silently
    this.queue.push({
      level: entry.level ?? 'INFO',
      message: entry.message,
      timestamp: new Date().toISOString(),
      attributes: attrs,
    });
  }

  info(m: string, a?: Record<string, unknown>): void { this.log({ level: 'INFO', message: m, attributes: a }); }
  warn(m: string, a?: Record<string, unknown>): void { this.log({ level: 'WARN', message: m, attributes: a }); }
  error(m: string, a?: Record<string, unknown>): void { this.log({ level: 'ERROR', message: m, attributes: a }); }

  captureException(err: unknown, ctx?: { tags?: Record<string, string>; message?: string; isFatal?: boolean }): void {
    const e: Error = err instanceof Error ? err : new Error(typeof err === 'string' ? err : JSON.stringify(err));
    this.log({
      level: 'ERROR',
      message: ctx?.message ?? `${e.name}: ${e.message}`,
      attributes: {
        'exception.type': e.name,
        'exception.message': e.message,
        'exception.stacktrace': e.stack ?? '',
        'fatal': String(ctx?.isFatal ?? false),
        ...(ctx?.tags ?? {}),
      },
    });
  }

  async flush(): Promise<void> {
    if (this.queue.length === 0) return;
    const batch = this.queue.splice(0, this.opts.maxBatchSize);
    try {
      const res = await fetch(`${this.opts.endpoint}/api/v1/ingest/logs`, {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${this.opts.token}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ service: this.opts.service, logs: batch }),
      });
      if (!res.ok && res.status >= 500) this.queue.unshift(...batch);
    } catch (_e) {
      this.queue.unshift(...batch); // network fail — keep them for next tick
    }
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    if (this.timer) clearInterval(this.timer);
    if (this.prevHandler && typeof ErrorUtils !== 'undefined') {
      ErrorUtils.setGlobalHandler(this.prevHandler);
    }
    await this.flush();
  }

  private installHandlers(): void {
    if (typeof ErrorUtils === 'undefined') return;
    this.prevHandler = ErrorUtils.getGlobalHandler();
    ErrorUtils.setGlobalHandler((err, isFatal) => {
      this.captureException(err, { isFatal });
      // best effort sync flush — we can't await but the keepalive helps
      void this.flush();
      this.prevHandler?.(err, isFatal);
    });
  }
}

let singleton: FaroExpoClient | null = null;

export function init(opts: FaroExpoOptions): FaroExpoClient {
  if (singleton) singleton.close().catch(() => undefined);
  singleton = new FaroExpoClient({
    endpoint: opts.endpoint.replace(/\/$/, ''),
    token: opts.token,
    service: opts.service,
    environment: opts.environment,
    release: opts.release,
    attributes: opts.attributes,
    flushIntervalMs: opts.flushIntervalMs ?? 2500,
    maxBatchSize: opts.maxBatchSize ?? 80,
    maxQueueSize: opts.maxQueueSize ?? 2000,
    installGlobalHandlers: opts.installGlobalHandlers ?? true,
  });
  return singleton;
}

function need(): FaroExpoClient {
  if (!singleton) throw new Error('faro: init() must be called before use');
  return singleton;
}

export const info = (m: string, a?: Record<string, unknown>) => need().info(m, a);
export const warn = (m: string, a?: Record<string, unknown>) => need().warn(m, a);
export const error = (m: string, a?: Record<string, unknown>) => need().error(m, a);
export const captureException = (
  err: unknown,
  ctx?: { tags?: Record<string, string>; message?: string },
) => need().captureException(err, ctx);
export const flush = () => need().flush();
export const close = () => need().close();
