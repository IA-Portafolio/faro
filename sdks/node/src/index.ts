/**
 * Faro SDK for Node.js / TypeScript.
 *
 * One file, no runtime deps. Uses globalThis.fetch (Node 18+ ships it).
 */

export type Severity = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR' | 'FATAL';

export interface FaroOptions {
  /** Faro base URL, e.g. https://faro.iaportafolio.com */
  endpoint: string;
  /** Project ingest token (from the Faro dashboard /projects page) */
  token: string;
  /** OTel service.name attached to every event */
  service: string;
  /** e.g. "production" / "staging" — added as attribute `deployment.environment` */
  environment?: string;
  /** Release / commit / tag — added as `service.version` */
  release?: string;
  /** Default attributes merged into every event */
  attributes?: Record<string, string | number | boolean>;
  /** Flush cadence in ms (default 750) */
  flushIntervalMs?: number;
  /** Max events per HTTP batch (default 200) */
  maxBatchSize?: number;
  /** Drop new events past this in-memory queue size (default 10_000) */
  maxQueueSize?: number;
  /** Install process-level error handlers (default true). Disable for embedded use. */
  installGlobalHandlers?: boolean;
  /** Logger for the SDK's own warnings. Defaults to console.warn. */
  diag?: (msg: string, err?: unknown) => void;
}

export interface LogEntry {
  level?: Severity;
  message: string;
  attributes?: Record<string, unknown>;
  trace_id?: string;
  span_id?: string;
  timestamp?: Date;
}

interface Wire {
  level: Severity;
  message: string;
  timestamp: string;
  service?: string;
  trace_id?: string;
  span_id?: string;
  attributes: Record<string, string>;
}

class FaroClient {
  private opts: Required<Omit<FaroOptions, 'attributes' | 'environment' | 'release' | 'diag'>> &
    Pick<FaroOptions, 'attributes' | 'environment' | 'release' | 'diag'>;
  private queue: Wire[] = [];
  private timer: ReturnType<typeof setInterval> | null = null;
  private closed = false;
  private installedHandlers: Array<() => void> = [];

  constructor(opts: FaroOptions) {
    this.opts = {
      endpoint: opts.endpoint.replace(/\/$/, ''),
      token: opts.token,
      service: opts.service,
      environment: opts.environment,
      release: opts.release,
      attributes: opts.attributes,
      flushIntervalMs: opts.flushIntervalMs ?? 750,
      maxBatchSize: opts.maxBatchSize ?? 200,
      maxQueueSize: opts.maxQueueSize ?? 10_000,
      installGlobalHandlers: opts.installGlobalHandlers ?? true,
      diag: opts.diag,
    };
    this.timer = setInterval(() => void this.flush(), this.opts.flushIntervalMs);
    // Allow Node to exit while the timer is the only thing left.
    if (typeof (this.timer as { unref?: () => void }).unref === 'function') {
      (this.timer as { unref: () => void }).unref();
    }
    if (this.opts.installGlobalHandlers) this.installHandlers();
  }

  log(entry: LogEntry): void {
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
    const wire: Wire = {
      level: entry.level ?? 'INFO',
      message: entry.message,
      timestamp: (entry.timestamp ?? new Date()).toISOString(),
      trace_id: entry.trace_id,
      span_id: entry.span_id,
      attributes: attrs,
    };
    if (this.queue.length >= this.opts.maxQueueSize) {
      this.warn('queue full, dropping event');
      return;
    }
    this.queue.push(wire);
  }

  info(message: string, attributes?: Record<string, unknown>): void {
    this.log({ level: 'INFO', message, attributes });
  }
  warn(message: string, attributes?: Record<string, unknown>): void {
    this.log({ level: 'WARN', message, attributes });
  }
  error(message: string, attributes?: Record<string, unknown>): void {
    this.log({ level: 'ERROR', message, attributes });
  }

  captureException(err: unknown, context?: { tags?: Record<string, string>; message?: string }): void {
    const e = toError(err);
    const attrs: Record<string, unknown> = {
      'exception.type': e.name,
      'exception.message': e.message,
      'exception.stacktrace': e.stack ?? '',
      ...(context?.tags ?? {}),
    };
    this.log({
      level: 'ERROR',
      message: context?.message ?? `${e.name}: ${e.message}`,
      attributes: attrs,
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
      if (!res.ok) {
        const txt = await res.text().catch(() => '');
        this.warn(`ingest HTTP ${res.status}: ${txt.slice(0, 200)}`);
      }
    } catch (e) {
      // Re-queue on transient network failures so we don't lose data.
      this.queue.unshift(...batch);
      this.warn('flush failed', e);
    }
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    for (const off of this.installedHandlers) off();
    this.installedHandlers = [];
    // One final flush attempt — drain everything pending.
    while (this.queue.length > 0) {
      const before = this.queue.length;
      await this.flush();
      if (this.queue.length >= before) break; // network probably down; give up
    }
  }

  private installHandlers(): void {
    if (typeof process === 'undefined') return;
    const onUncaught = (err: Error): void => {
      this.captureException(err, { message: '[uncaughtException] ' + (err?.message ?? '') });
      // Drain on best-effort basis; do NOT swallow — let Node print + exit.
      void this.flush();
    };
    const onRejection = (reason: unknown): void => {
      this.captureException(reason, { message: '[unhandledRejection]' });
      void this.flush();
    };
    const onExit = (): void => {
      // We can't await here. Fire one last fetch synchronously-ish.
      void this.flush();
    };
    process.on('uncaughtException', onUncaught);
    process.on('unhandledRejection', onRejection);
    process.on('beforeExit', onExit);
    this.installedHandlers.push(
      () => process.off('uncaughtException', onUncaught),
      () => process.off('unhandledRejection', onRejection),
      () => process.off('beforeExit', onExit),
    );
  }

  private diagLog(msg: string, err?: unknown): void {
    if (this.opts.diag) this.opts.diag(msg, err);
    else console.warn(`[faro] ${msg}`, err ?? '');
  }
}

function toError(err: unknown): Error {
  if (err instanceof Error) return err;
  if (typeof err === 'string') return new Error(err);
  try {
    return new Error(JSON.stringify(err));
  } catch {
    return new Error(String(err));
  }
}

let singleton: FaroClient | null = null;

export function init(opts: FaroOptions): FaroClient {
  if (singleton) singleton.close().catch(() => undefined);
  singleton = new FaroClient(opts);
  return singleton;
}

export function getClient(): FaroClient {
  if (!singleton) throw new Error('faro: init() must be called before use');
  return singleton;
}

// Module-level helpers so users don't need to pass the client around.
export function log(entry: LogEntry): void { getClient().log(entry); }
export function info(msg: string, attrs?: Record<string, unknown>): void { getClient().info(msg, attrs); }
export function warn(msg: string, attrs?: Record<string, unknown>): void { getClient().warn(msg, attrs); }
export function error(msg: string, attrs?: Record<string, unknown>): void { getClient().error(msg, attrs); }
export function captureException(err: unknown, ctx?: { tags?: Record<string, string>; message?: string }): void {
  getClient().captureException(err, ctx);
}
export function flush(): Promise<void> { return getClient().flush(); }
export function close(): Promise<void> { return getClient().close(); }
