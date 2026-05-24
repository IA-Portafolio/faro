/**
 * @iaportafolio/browser
 *
 * SDK browser para Faro. Captura errores no manejados, Web Vitals, navegaciones
 * y clicks como breadcrumbs, y envía todo en lotes a la API de ingesta usando
 * sendBeacon cuando el tab se cierra (sin perder eventos).
 *
 * Uso mínimo:
 *
 *   import { init } from '@iaportafolio/browser';
 *
 *   init({
 *     endpoint: 'https://faro.iaportafolio.com',
 *     token:    'tu-token-de-proyecto',
 *     service:  'mi-app-web',
 *   });
 *
 *   // a partir de aquí, errores no atrapados se reportan solos
 */

export type Severity = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR' | 'FATAL';

export interface FaroBrowserOptions {
  endpoint: string;
  token: string;
  service: string;
  environment?: string;
  release?: string;
  /** Atributos por defecto adjuntados a cada evento */
  attributes?: Record<string, string | number | boolean>;
  /** Cadencia de flush en ms (default 2000) */
  flushIntervalMs?: number;
  /** Tamaño máximo de batch por POST (default 100) */
  maxBatchSize?: number;
  /** Cola en memoria máxima (default 2000) */
  maxQueueSize?: number;
  /** Tamaño del ring buffer de breadcrumbs (default 30) */
  maxBreadcrumbs?: number;
  /** Capturar window.onerror + unhandledrejection (default true) */
  captureUnhandled?: boolean;
  /** Capturar console.error y console.warn (default false — puede meter ruido) */
  captureConsole?: boolean;
  /** Capturar Web Vitals LCP/CLS/INP/FID/TTFB (default true) */
  captureWebVitals?: boolean;
  /** Capturar clicks como breadcrumbs (default true) */
  captureClicks?: boolean;
  /** Capturar navegaciones SPA (history.pushState/popstate) (default true) */
  captureNavigation?: boolean;
  /** Hook para muestrear o redactar eventos antes de enviar; devolver null descarta */
  beforeSend?: (event: WireEvent) => WireEvent | null;
}

export interface UserContext {
  id?: string;
  email?: string;
  username?: string;
  [key: string]: string | undefined;
}

export interface Breadcrumb {
  category: 'click' | 'navigation' | 'console' | 'fetch' | 'custom';
  message: string;
  timestamp: number;
  data?: Record<string, string | number | boolean | undefined>;
}

export interface LogEntry {
  level?: Severity;
  message: string;
  attributes?: Record<string, unknown>;
  trace_id?: string;
  span_id?: string;
}

export interface WireEvent {
  level: Severity;
  message: string;
  timestamp: string;
  attributes: Record<string, string>;
  trace_id?: string;
  span_id?: string;
}

class FaroBrowser {
  private opts: Required<Omit<FaroBrowserOptions, 'attributes' | 'environment' | 'release' | 'beforeSend'>> &
    Pick<FaroBrowserOptions, 'attributes' | 'environment' | 'release' | 'beforeSend'>;
  private queue: WireEvent[] = [];
  private breadcrumbs: Breadcrumb[] = [];
  private user: UserContext | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private cleanup: Array<() => void> = [];
  private closed = false;

  constructor(opts: FaroBrowserOptions) {
    this.opts = {
      endpoint: opts.endpoint.replace(/\/$/, ''),
      token: opts.token,
      service: opts.service,
      environment: opts.environment,
      release: opts.release,
      attributes: opts.attributes,
      flushIntervalMs: opts.flushIntervalMs ?? 2000,
      maxBatchSize: opts.maxBatchSize ?? 100,
      maxQueueSize: opts.maxQueueSize ?? 2000,
      maxBreadcrumbs: opts.maxBreadcrumbs ?? 30,
      captureUnhandled: opts.captureUnhandled ?? true,
      captureConsole: opts.captureConsole ?? false,
      captureWebVitals: opts.captureWebVitals ?? true,
      captureClicks: opts.captureClicks ?? true,
      captureNavigation: opts.captureNavigation ?? true,
      beforeSend: opts.beforeSend,
    };

    if (typeof window === 'undefined') {
      // SSR / Node: degradar a no-op silencioso.
      return;
    }

    this.timer = setInterval(() => void this.flush(), this.opts.flushIntervalMs);
    if (this.opts.captureUnhandled) this.installErrorHandlers();
    if (this.opts.captureConsole) this.installConsoleCapture();
    if (this.opts.captureWebVitals) this.installWebVitals();
    if (this.opts.captureClicks) this.installClickTracking();
    if (this.opts.captureNavigation) this.installNavigationTracking();
    this.installLifecycleHooks();
  }

  // ---------- API pública ----------

  setUser(user: UserContext | null): void {
    this.user = user;
  }

  addBreadcrumb(crumb: Omit<Breadcrumb, 'timestamp'>): void {
    if (this.breadcrumbs.length >= this.opts.maxBreadcrumbs) {
      this.breadcrumbs.shift();
    }
    this.breadcrumbs.push({ ...crumb, timestamp: Date.now() });
  }

  log(entry: LogEntry): void {
    if (this.closed) return;
    const attrs = this.composeAttributes(entry.attributes);
    const evt: WireEvent = {
      level: entry.level ?? 'INFO',
      message: entry.message,
      timestamp: new Date().toISOString(),
      attributes: attrs,
      trace_id: entry.trace_id,
      span_id: entry.span_id,
    };
    this.enqueue(evt);
  }

  info(message: string, attrs?: Record<string, unknown>): void { this.log({ level: 'INFO', message, attributes: attrs }); }
  warn(message: string, attrs?: Record<string, unknown>): void { this.log({ level: 'WARN', message, attributes: attrs }); }
  error(message: string, attrs?: Record<string, unknown>): void { this.log({ level: 'ERROR', message, attributes: attrs }); }

  captureException(err: unknown, ctx?: { tags?: Record<string, string>; message?: string }): void {
    const e = toError(err);
    this.log({
      level: 'ERROR',
      message: ctx?.message ?? `${e.name}: ${e.message}`,
      attributes: {
        'exception.type': e.name,
        'exception.message': e.message,
        'exception.stacktrace': e.stack ?? '',
        ...(ctx?.tags ?? {}),
      },
    });
  }

  async flush(useBeacon = false): Promise<void> {
    if (this.queue.length === 0) return;
    const batch = this.queue.splice(0, this.opts.maxBatchSize);
    const body = JSON.stringify({ service: this.opts.service, logs: batch });
    const url = `${this.opts.endpoint}/api/v1/ingest/logs`;

    // Si la página se está cerrando, sendBeacon es la única vía fiable
    // (fetch con keepalive también, pero sendBeacon es la apuesta segura).
    if (useBeacon && typeof navigator !== 'undefined' && typeof navigator.sendBeacon === 'function') {
      // sendBeacon no soporta headers personalizados; pasamos el token como query param
      const beaconUrl = `${url}?_token=${encodeURIComponent(this.opts.token)}`;
      const ok = navigator.sendBeacon(beaconUrl, new Blob([body], { type: 'application/json' }));
      if (ok) return;
    }

    try {
      const res = await fetch(url, {
        method: 'POST',
        keepalive: true,
        headers: {
          'Authorization': `Bearer ${this.opts.token}`,
          'Content-Type': 'application/json',
        },
        body,
      });
      if (!res.ok && res.status >= 500) {
        // Re-encola si el servidor está caído (no 4xx — esos son irrecuperables)
        this.queue.unshift(...batch);
      }
    } catch {
      this.queue.unshift(...batch);
    }
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    for (const fn of this.cleanup) fn();
    this.cleanup = [];
    void this.flush(true);
  }

  // ---------- Internals ----------

  private enqueue(evt: WireEvent): void {
    const processed = this.opts.beforeSend ? this.opts.beforeSend(evt) : evt;
    if (!processed) return;
    if (this.queue.length >= this.opts.maxQueueSize) return;
    this.queue.push(processed);
  }

  private composeAttributes(extra?: Record<string, unknown>): Record<string, string> {
    const attrs: Record<string, string> = {};
    if (this.opts.attributes) {
      for (const [k, v] of Object.entries(this.opts.attributes)) attrs[k] = String(v);
    }
    if (this.opts.environment) attrs['deployment.environment'] = this.opts.environment;
    if (this.opts.release) attrs['service.version'] = this.opts.release;
    if (typeof window !== 'undefined') {
      attrs['browser.url'] = window.location.href;
      attrs['browser.userAgent'] = navigator.userAgent;
    }
    if (this.user) {
      if (this.user.id) attrs['user.id'] = this.user.id;
      if (this.user.email) attrs['user.email'] = this.user.email;
      if (this.user.username) attrs['user.name'] = this.user.username;
    }
    if (this.breadcrumbs.length > 0) {
      // Sólo serializamos breadcrumbs en eventos ERROR/WARN para no inflar logs INFO normales
      // (los caller pueden pasar `.breadcrumbs` manual si quieren forzar).
      attrs['breadcrumbs'] = JSON.stringify(this.breadcrumbs.slice(-this.opts.maxBreadcrumbs));
    }
    if (extra) {
      for (const [k, v] of Object.entries(extra)) {
        attrs[k] = typeof v === 'string' ? v : JSON.stringify(v);
      }
    }
    return attrs;
  }

  private installErrorHandlers(): void {
    const onError = (ev: ErrorEvent): void => {
      this.captureException(ev.error ?? ev.message, {
        tags: { origin: 'window.error', 'source.file': ev.filename ?? '', 'source.line': String(ev.lineno ?? 0) },
      });
    };
    const onRejection = (ev: PromiseRejectionEvent): void => {
      this.captureException(ev.reason, { tags: { origin: 'unhandledrejection' } });
    };
    window.addEventListener('error', onError);
    window.addEventListener('unhandledrejection', onRejection);
    this.cleanup.push(() => window.removeEventListener('error', onError));
    this.cleanup.push(() => window.removeEventListener('unhandledrejection', onRejection));
  }

  private installConsoleCapture(): void {
    const orig = { error: console.error, warn: console.warn };
    console.error = (...args: unknown[]) => {
      this.addBreadcrumb({ category: 'console', message: String(args[0] ?? ''), data: { level: 'error' } });
      this.log({ level: 'ERROR', message: stringifyArgs(args), attributes: { 'console.method': 'error' } });
      orig.error.apply(console, args);
    };
    console.warn = (...args: unknown[]) => {
      this.addBreadcrumb({ category: 'console', message: String(args[0] ?? ''), data: { level: 'warn' } });
      orig.warn.apply(console, args);
    };
    this.cleanup.push(() => { console.error = orig.error; console.warn = orig.warn; });
  }

  private installWebVitals(): void {
    // Importación dinámica para no inflar el bundle cuando captureWebVitals=false
    void import('web-vitals')
      .then(({ onLCP, onCLS, onINP, onFCP, onTTFB }) => {
        const report = (name: string) => (m: { value: number; rating: string; id: string }) => {
          this.log({
            level: 'INFO',
            message: `web-vital ${name}`,
            attributes: {
              'metric.name': name,
              'metric.value': m.value,
              'metric.rating': m.rating,
              'metric.id': m.id,
            },
          });
        };
        onLCP(report('LCP'));
        onCLS(report('CLS'));
        onINP(report('INP'));
        onFCP(report('FCP'));
        onTTFB(report('TTFB'));
      })
      .catch(() => {
        // web-vitals no instalado o falló el dynamic import — silencio.
      });
  }

  private installClickTracking(): void {
    const onClick = (ev: MouseEvent): void => {
      const target = ev.target as Element | null;
      if (!target) return;
      const tag = target.tagName?.toLowerCase() ?? '';
      const id = (target as HTMLElement).id;
      const text = (target.textContent ?? '').trim().slice(0, 60);
      const data: Record<string, string | number> = { tag };
      if (id) data.id = id;
      if (text) data.text = text;
      this.addBreadcrumb({ category: 'click', message: `${tag}${id ? '#' + id : ''}`, data });
    };
    window.addEventListener('click', onClick, { capture: true, passive: true });
    this.cleanup.push(() => window.removeEventListener('click', onClick, { capture: true }));
  }

  private installNavigationTracking(): void {
    const log = (from: string, to: string, method: string): void => {
      if (from === to) return;
      this.addBreadcrumb({ category: 'navigation', message: `${from} → ${to}`, data: { method, to } });
    };

    const origPush = history.pushState;
    const origReplace = history.replaceState;
    history.pushState = function (this: History, ...args) {
      const from = location.href;
      const ret = origPush.apply(this, args);
      log(from, location.href, 'pushState');
      return ret;
    };
    history.replaceState = function (this: History, ...args) {
      const from = location.href;
      const ret = origReplace.apply(this, args);
      log(from, location.href, 'replaceState');
      return ret;
    };
    const onPop = (): void => log('', location.href, 'popstate');
    window.addEventListener('popstate', onPop);

    this.cleanup.push(() => {
      history.pushState = origPush;
      history.replaceState = origReplace;
      window.removeEventListener('popstate', onPop);
    });
  }

  private installLifecycleHooks(): void {
    const onHide = (): void => {
      if (document.visibilityState === 'hidden') void this.flush(true);
    };
    const onPageHide = (): void => void this.flush(true);
    document.addEventListener('visibilitychange', onHide);
    window.addEventListener('pagehide', onPageHide);
    this.cleanup.push(() => document.removeEventListener('visibilitychange', onHide));
    this.cleanup.push(() => window.removeEventListener('pagehide', onPageHide));
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

function stringifyArgs(args: unknown[]): string {
  return args
    .map((a) => (typeof a === 'string' ? a : a instanceof Error ? a.stack ?? a.message : safeJson(a)))
    .join(' ');
}

function safeJson(v: unknown): string {
  try { return JSON.stringify(v); } catch { return String(v); }
}

// ---------- Singleton helpers ----------

let singleton: FaroBrowser | null = null;

export function init(opts: FaroBrowserOptions): FaroBrowser {
  if (singleton) singleton.close();
  singleton = new FaroBrowser(opts);
  return singleton;
}

export function getClient(): FaroBrowser {
  if (!singleton) throw new Error('faro: init() must be called before use');
  return singleton;
}

export function log(entry: LogEntry): void { getClient().log(entry); }
export function info(msg: string, attrs?: Record<string, unknown>): void { getClient().info(msg, attrs); }
export function warn(msg: string, attrs?: Record<string, unknown>): void { getClient().warn(msg, attrs); }
export function error(msg: string, attrs?: Record<string, unknown>): void { getClient().error(msg, attrs); }
export function captureException(err: unknown, ctx?: { tags?: Record<string, string>; message?: string }): void {
  getClient().captureException(err, ctx);
}
export function setUser(user: UserContext | null): void { getClient().setUser(user); }
export function addBreadcrumb(crumb: Omit<Breadcrumb, 'timestamp'>): void { getClient().addBreadcrumb(crumb); }
export function flush(): Promise<void> { return getClient().flush(); }
export function close(): void { getClient().close(); }

export { FaroBrowser };
