/**
 * Core RUM para navegador (interno de @iaportafolio/nextjs).
 *
 * Captura errores no manejados, Web Vitals, navegaciones y clicks como
 * breadcrumbs, y envía todo en lotes a la API de ingesta usando
 * sendBeacon cuando el tab se cierra (sin perder eventos).
 *
 * Este archivo no se exporta directamente al usuario; el entrypoint
 * público es `@iaportafolio/nextjs/client`.
 */

import {
  initSessionReplay,
  getOrCreateSessionId,
  type SessionReplayController,
} from './browser-replay';

export type Severity = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR' | 'FATAL';

// Funciones puras compartidas cross-SDK (scrubbing + feature flags).
// Fuente canónica: sdks/_shared/sdk-core (inlineado en el bundle por tsup).
import {
  DEFAULT_SCRUB_FIELDS,
  HEADER_SCRUB_FIELDS,
  SCRUB_REGEXES,
  scrubWire,
  clampRollout,
  normalizeConditions,
  matchesFeatureConditions,
  stickyBucket,
  type ScrubPreset,
  type FeatureFlagContext,
  type FeatureFlagWire,
} from '@iaportafolio/sdk-core';

export type { ScrubPreset, FeatureFlagContext, FeatureFlagWire } from '@iaportafolio/sdk-core';

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
  /** Capturar Web Vitals LCP/CLS/INP/FCP/TTFB (default true) */
  captureWebVitals?: boolean;
  /** Capturar clicks como breadcrumbs (default true) */
  captureClicks?: boolean;
  /** Capturar navegaciones SPA (history.pushState/popstate) (default true) */
  captureNavigation?: boolean;
  /** Auto-tracking de product events. Opt-in; por defecto todo apagado. */
  autoCapture?: AutoCaptureOptions;
  /**
   * Habilita session replay con rrweb (default false). Requiere instalar
   * `rrweb` como peer dependency en el proyecto consumidor. Si está pero
   * rrweb no se pudo cargar, el SDK loggea un warning y sigue sin replay.
   */
  captureSessionReplay?: boolean;
  /** % de sesiones a grabar cuando captureSessionReplay=true (default 1.0) */
  sessionReplaySampleRate?: number;
  /**
   * Si true (DEFAULT), el session replay enmascara TODO el texto del DOM (no
   * sólo inputs), para no capturar PII visible en pantalla (emails, saldos,
   * nombres). Ponelo en `false` sólo si necesitás replays con texto legible y
   * ya controlás la PII con `.faro-mask` u otros medios.
   */
  sessionReplayMaskAllText?: boolean;
  /** Hook post-scrub; devolver null descarta el evento. */
  beforeSend?: (event: WireEvent) => WireEvent | null;
  /** Substrings case-insensitive: cualquier atributo cuya clave los contenga se redacta. Default: lista común de campos sensibles. */
  scrubFields?: string[];
  /** Si true, suma headers comunes (authorization, cookie, set-cookie) a scrubFields. Default: true. */
  scrubHeaders?: boolean;
  /** Presets de regex aplicados a values string y al message. Default: ['jwt','api-key']. */
  scrubPatterns?: ScrubPreset[];
  /** Cadencia de refresh de feature flags en ms (por defecto 30_000). */
  featureFlagRefreshIntervalMs?: number;
  /**
   * Si true (default), las URLs capturadas automáticamente (`browser.url`,
   * `page.url`, navegaciones, click props) se publican sin querystring ni
   * fragmento. Esto evita filtrar tokens de reset, emails y otros secretos
   * que las apps suelen poner en query params. Setealo en `false` cuando
   * realmente necesités la URL completa y sepas que no contiene PII.
   */
  scrubUrlQuery?: boolean;
}

export interface AutoCaptureOptions {
  /** Page events en init, History API, popstate y hashchange. */
  pageViews?: boolean;
  /** Clicks en [data-faro], <button> y <a>. */
  clicks?: boolean;
  /** Submit de form[data-faro-form]. */
  formSubmissions?: boolean;
  /** 3+ clicks en menos de 2s sobre el mismo elemento. */
  rageClicks?: boolean;
  /** Click elegible sin cambio de URL ni DOM en una ventana corta. */
  deadClicks?: boolean;
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

interface FeatureFlagsResponse {
  project?: string;
  flags?: FeatureFlagWire[];
}

/** Tipo de evento product API (Segment/PostHog-like). */
export type ProductEventType = 'track' | 'identify' | 'page' | 'screen' | 'alias';

export interface ProductEventWire {
  type: ProductEventType;
  name: string;
  timestamp: string;
  distinct_id: string;
  anonymous_id: string;
  session_id: string;
  properties: Record<string, unknown>;
  user_properties: Record<string, unknown>;
  context: Record<string, unknown>;
  source: string;
}

/** localStorage key — persiste el anonymous_id entre recargas para que la sesión
 *  pre-login pueda fusionarse con `alias` cuando el user se logee más tarde. */
const ANON_ID_KEY = 'faro.anon_id';

function getOrCreateAnonymousId(): string {
  if (typeof localStorage === 'undefined') {
    return randomAnonymousId();
  }
  try {
    const existing = localStorage.getItem(ANON_ID_KEY);
    if (existing) return existing;
    const next = randomAnonymousId();
    localStorage.setItem(ANON_ID_KEY, next);
    return next;
  } catch {
    return randomAnonymousId();
  }
}

function randomAnonymousId(): string {
  if (typeof crypto !== 'undefined') {
    if (typeof crypto.randomUUID === 'function') return crypto.randomUUID();
    // WebCrypto (getRandomValues está en todo navegador moderno y Node 15+);
    // evitamos Math.random, que no es criptográficamente seguro.
    const bytes = new Uint8Array(12);
    crypto.getRandomValues(bytes);
    return `anon_${Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')}`;
  }
  // Entorno sin WebCrypto (muy raro): es solo un id de analítica anónima, no un
  // secreto. Evitamos Math.random igual.
  return `anon_${Date.now().toString(36)}`;
}

class FaroBrowser {
  private opts: Required<Omit<FaroBrowserOptions, 'attributes' | 'environment' | 'release' | 'beforeSend'>> &
    Pick<FaroBrowserOptions, 'attributes' | 'environment' | 'release' | 'beforeSend'>;
  private queue: WireEvent[] = [];
  private eventsQueue: ProductEventWire[] = [];
  private breadcrumbs: Breadcrumb[] = [];
  private user: UserContext | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private featureFlagsTimer: ReturnType<typeof setInterval> | null = null;
  private cleanup: Array<() => void> = [];
  private closed = false;
  private scrubNeedles: string[];
  private scrubRegexes: RegExp[];
  private featureFlags = new Map<string, FeatureFlagWire>();
  private featureFlagsProject = '';
  private featureExposureSeen = new Set<string>();
  private aliasSeen = new Set<string>();
  private rageClickState: { key: string; times: number[] } | null = null;
  /** distinct_id (post-login). Vacío hasta que `identify()` se invoca; mientras
   *  tanto los eventos van con `anonymous_id` también como `distinct_id`. */
  private distinctId = '';
  /** anonymous_id persistido en localStorage. Sobrevive a reloads y permite
   *  que `alias()` fusione la sesión pre-login con el user post-login. */
  private anonymousId = '';
  private userProperties: Record<string, unknown> = {};
  /** Identifica la sesión actual (persistente en el tab). Se incluye en cada
   *  evento que sale del SDK para que el dashboard pueda saltar de un error
   *  al replay correspondiente. */
  private sessionId: string;
  private replay: SessionReplayController | null = null;

  constructor(opts: FaroBrowserOptions) {
    const scrubFields = opts.scrubFields ?? DEFAULT_SCRUB_FIELDS;
    const scrubHeaders = opts.scrubHeaders ?? true;
    const scrubPatterns = opts.scrubPatterns ?? (['jwt', 'api-key'] as ScrubPreset[]);
    this.scrubNeedles = Array.from(new Set([
      ...scrubFields.map((s) => s.toLowerCase()),
      ...(scrubHeaders ? HEADER_SCRUB_FIELDS : []),
    ]));
    this.scrubRegexes = scrubPatterns.map((p) => SCRUB_REGEXES[p]).filter(Boolean);
    this.opts = {
      endpoint: opts.endpoint.replace(/\/$/, ''),
      token: opts.token,
      service: opts.service,
      environment: opts.environment,
      release: opts.release,
      attributes: opts.attributes,
      // Perfil de defaults: "browser" (sdks/README.md → Perfiles de defaults).
      flushIntervalMs: opts.flushIntervalMs ?? 2000,
      maxBatchSize: opts.maxBatchSize ?? 100,
      maxQueueSize: opts.maxQueueSize ?? 2000,
      maxBreadcrumbs: opts.maxBreadcrumbs ?? 30,
      captureUnhandled: opts.captureUnhandled ?? true,
      captureConsole: opts.captureConsole ?? false,
      captureWebVitals: opts.captureWebVitals ?? true,
      captureClicks: opts.captureClicks ?? true,
      captureNavigation: opts.captureNavigation ?? true,
      autoCapture: {
        pageViews: opts.autoCapture?.pageViews ?? false,
        clicks: opts.autoCapture?.clicks ?? false,
        formSubmissions: opts.autoCapture?.formSubmissions ?? false,
        rageClicks: opts.autoCapture?.rageClicks ?? false,
        deadClicks: opts.autoCapture?.deadClicks ?? false,
      },
      captureSessionReplay: opts.captureSessionReplay ?? false,
      sessionReplaySampleRate: opts.sessionReplaySampleRate ?? 1.0,
      sessionReplayMaskAllText: opts.sessionReplayMaskAllText ?? true,
      beforeSend: opts.beforeSend,
      scrubFields,
      scrubHeaders,
      scrubPatterns,
      featureFlagRefreshIntervalMs: opts.featureFlagRefreshIntervalMs ?? 30_000,
      scrubUrlQuery: opts.scrubUrlQuery ?? true,
    };

    if (typeof window === 'undefined') {
      // SSR / Node: degradar a no-op silencioso.
      this.sessionId = '';
      return;
    }

    this.sessionId = getOrCreateSessionId();
    this.anonymousId = getOrCreateAnonymousId();

    this.timer = setInterval(() => void this.flush(), this.opts.flushIntervalMs);
    this.featureFlagsTimer = setInterval(
      () => void this.refreshFeatureFlags(),
      this.opts.featureFlagRefreshIntervalMs,
    );
    if (this.opts.captureUnhandled) this.installErrorHandlers();
    if (this.opts.captureConsole) this.installConsoleCapture();
    if (this.opts.captureWebVitals) this.installWebVitals();
    this.installAutoCapture();
    if (this.opts.captureClicks) this.installClickTracking();
    if (this.opts.captureNavigation) this.installNavigationTracking();
    this.installLifecycleHooks();

    if (this.opts.captureSessionReplay) {
      this.replay = initSessionReplay({
        endpoint: this.opts.endpoint,
        token: this.opts.token,
        service: this.opts.service,
        sessionId: this.sessionId,
        sampleRate: this.opts.sessionReplaySampleRate,
        getUserId: () => this.user?.id,
        scrubUrlQuery: this.opts.scrubUrlQuery,
        maskAllText: this.opts.sessionReplayMaskAllText,
      });
    }
  }

  /** Devuelve el id de sesión del tab actual (vacío en SSR). */
  getSessionId(): string {
    return this.sessionId;
  }

  setUser(user: UserContext | null): void {
    this.user = user;
  }

  // ---------- Product events API (Segment/PostHog-like) ----------

  /** Envía un evento custom de producto. */
  track(eventName: string, properties: Record<string, unknown> = {}): void {
    this.enqueueEvent({ type: 'track', name: eventName, properties });
  }

  /** Identifica al usuario; setea `distinct_id` para todos los eventos
   *  posteriores y actualiza `setUser` para que también enriquezca los logs. */
  identify(userId: string, traits: Record<string, unknown> = {}): void {
    if (!userId) return;
    if (this.anonymousId && this.anonymousId !== userId) {
      this.enqueueAliasOnce(this.anonymousId, userId);
    }
    this.distinctId = userId;
    this.userProperties = { ...this.userProperties, ...traits };
    this.setUser({
      id: userId,
      ...Object.fromEntries(
        Object.entries(traits).map(([k, v]) => [k, typeof v === 'string' ? v : JSON.stringify(v)]),
      ),
    });
    this.enqueueEvent({
      type: 'identify',
      name: '$identify',
      properties: {},
      userPropertiesOverride: traits,
    });
  }

  /** Marca un page view. Si no se pasa path, usa `window.location.pathname`. */
  page(path?: string, properties: Record<string, unknown> = {}): void {
    const p = path ?? (typeof window !== 'undefined' ? window.location.pathname : '');
    this.enqueueEvent({ type: 'page', name: p, properties });
  }

  /** Fusiona un anonymous_id previo con un user_id post-login. */
  alias(prevId: string, newId: string): void {
    if (!prevId || !newId) return;
    this.distinctId = newId;
    this.enqueueAliasOnce(prevId, newId);
  }

  private enqueueAliasOnce(prevId: string, newId: string): void {
    const key = `${prevId}\u0000${newId}`;
    if (this.aliasSeen.has(key)) return;
    this.aliasSeen.add(key);
    this.enqueueEvent({
      type: 'alias',
      name: '$alias',
      properties: { from: prevId, to: newId },
      anonymousIdOverride: prevId,
      distinctIdOverride: newId,
    });
  }

  async refreshFeatureFlags(): Promise<void> {
    if (this.closed || typeof fetch === 'undefined') return;
    try {
      const res = await fetch(`${this.opts.endpoint}/api/v1/ingest/feature-flags`, {
        method: 'GET',
        headers: { 'Authorization': `Bearer ${this.opts.token}` },
      });
      if (!res.ok) return;
      const body = (await res.json()) as FeatureFlagsResponse;
      if (!Array.isArray(body.flags)) return;
      const next = new Map<string, FeatureFlagWire>();
      for (const flag of body.flags) {
        if (!flag || typeof flag.key !== 'string' || flag.key.length === 0) continue;
        next.set(flag.key, {
          key: flag.key,
          rollout_percentage: clampRollout(flag.rollout_percentage),
          conditions: normalizeConditions(flag.conditions),
        });
      }
      this.featureFlags = next;
      this.featureFlagsProject = typeof body.project === 'string' ? body.project : '';
    } catch {
      // Mantener la cache anterior: feature flags no deben romper la app.
    }
  }

  isFeatureEnabled(key: string, context: FeatureFlagContext = {}): boolean {
    const flag = this.featureFlags.get(key);
    if (!flag) return false;
    const id = context.distinct_id || this.distinctId || this.anonymousId;
    if (!matchesFeatureConditions(flag, context)) return false;
    const rollout = clampRollout(flag.rollout_percentage);
    const enabled = rollout >= 100
      ? true
      : rollout > 0 && stickyBucket(`${this.featureFlagsProject}:${key}:${id}`) < rollout;
    this.trackFeatureExposure(key, id, enabled);
    return enabled;
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
  /** Alias de `warn` — paridad con `logging.WARNING` / SDK de Python. */
  warning(message: string, attrs?: Record<string, unknown>): void { this.log({ level: 'WARN', message, attributes: attrs }); }
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
    // Logs y events viajan a endpoints distintos; los flusheamos en paralelo
    // para no pagar dos RTTs cuando hay que cerrar la pestaña.
    await Promise.all([this.flushLogs(useBeacon), this.flushEvents(useBeacon)]);
  }

  /**
   * Re-encola un batch fallido al frente acotando la cola a `maxQueueSize`. Sin
   * esto, durante un outage del backend la cola podía exceder el tope (entran
   * registros nuevos mientras el batch espera el `await` y luego se reinsertan con
   * `unshift`) y crecer sin cota hasta agotar la memoria de la pestaña. Descartamos
   * los más viejos (frente), prefiriendo telemetría fresca.
   */
  private requeue<T>(queue: T[], batch: T[]): void {
    queue.unshift(...batch);
    const overflow = queue.length - this.opts.maxQueueSize;
    if (overflow > 0) queue.splice(0, overflow);
  }

  private async flushLogs(useBeacon = false): Promise<void> {
    if (this.queue.length === 0) return;
    const batch = this.queue.splice(0, this.opts.maxBatchSize);
    const body = JSON.stringify({ service: this.opts.service, logs: batch });
    const url = `${this.opts.endpoint}/api/v1/ingest/logs`;

    // En el path beacon (pagehide/visibilitychange) usamos `fetch keepalive` en
    // lugar de `navigator.sendBeacon` porque éste último NO permite custom
    // headers en Chromium/Firefox/WebKit, y meter el token en `?_token=` lo
    // filtra a access-logs, history, referer y proxies. `keepalive: true` le
    // pide al browser que complete el POST aunque la pestaña se cierre y
    // permite enviar `Authorization: Bearer` como cualquier otro request.
    if (useBeacon) {
      const result = await this.postWithKeepalive(url, body);
      if (result === 'sent') return;
      if (result === 'retry') {
        this.requeue(this.queue, batch);
        return;
      }
      // `unavailable`: caemos al POST normal de abajo.
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
        this.requeue(this.queue, batch);
      }
    } catch {
      this.requeue(this.queue, batch);
    }
  }

  private async flushEvents(useBeacon = false): Promise<void> {
    if (this.eventsQueue.length === 0) return;
    const batch = this.eventsQueue.splice(0, this.opts.maxBatchSize);
    const body = JSON.stringify({ service: this.opts.service, events: batch });
    const url = `${this.opts.endpoint}/api/v1/ingest/events`;

    // Ver flushLogs: misma justificación para evitar `?_token=` en URL.
    if (useBeacon) {
      const result = await this.postWithKeepalive(url, body);
      if (result === 'sent') return;
      if (result === 'retry') {
        this.requeue(this.eventsQueue, batch);
        return;
      }
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
        this.requeue(this.eventsQueue, batch);
      }
    } catch {
      this.requeue(this.eventsQueue, batch);
    }
  }

  /**
   * POST con `keepalive: true` para que el browser termine el envío aunque la
   * pestaña se esté cerrando.
   *
   * - `'sent'`        → el server respondió (2xx/3xx/4xx). El batch se dio por entregado.
   * - `'retry'`       → el server respondió 5xx. El caller re-encolará.
   * - `'unavailable'` → `fetch` no existe o el POST falló de red; el caller cae al
   *                     path normal (que también re-encola en 5xx / error).
   */
  private async postWithKeepalive(url: string, body: string): Promise<'sent' | 'retry' | 'unavailable'> {
    if (typeof fetch === 'undefined') return 'unavailable';
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
      if (res.status >= 500) return 'retry';
      return 'sent';
    } catch {
      return 'unavailable';
    }
  }

  private enqueueEvent(input: {
    type: ProductEventType;
    name: string;
    properties: Record<string, unknown>;
    userPropertiesOverride?: Record<string, unknown>;
    anonymousIdOverride?: string;
    distinctIdOverride?: string;
  }): void {
    if (this.closed) return;
    if (this.eventsQueue.length >= this.opts.maxQueueSize) return;
    const ctx: Record<string, unknown> = {};
    if (this.opts.environment) ctx.environment = this.opts.environment;
    if (this.opts.release) ctx.release = this.opts.release;
    if (this.opts.attributes) Object.assign(ctx, this.opts.attributes);
    if (typeof window !== 'undefined') {
      ctx['page.url'] = this.currentUrl();
      ctx['page.path'] = window.location.pathname;
      ctx['user_agent'] = navigator.userAgent;
    }
    this.eventsQueue.push({
      type: input.type,
      name: input.name,
      timestamp: new Date().toISOString(),
      distinct_id: input.distinctIdOverride ?? (this.distinctId || this.anonymousId),
      anonymous_id: input.anonymousIdOverride ?? this.anonymousId,
      session_id: this.sessionId,
      properties: input.properties,
      user_properties: input.userPropertiesOverride ?? this.userProperties,
      context: ctx,
      source: 'web',
    });
  }

  private trackFeatureExposure(flagKey: string, distinctId: string, enabled: boolean): void {
    const variant = enabled ? 'B' : 'A';
    const key = `${flagKey}:${distinctId}:${variant}`;
    if (this.featureExposureSeen.has(key)) return;
    this.featureExposureSeen.add(key);
    this.enqueueEvent({
      type: 'track',
      name: '$feature_exposure',
      distinctIdOverride: distinctId,
      properties: {
        flag_key: flagKey,
        variant,
        enabled,
      },
    });
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    if (this.featureFlagsTimer) clearInterval(this.featureFlagsTimer);
    this.featureFlagsTimer = null;
    for (const fn of [...this.cleanup].reverse()) fn();
    this.cleanup = [];
    void this.flush(true);
    this.replay?.stop();
  }

  private enqueue(evt: WireEvent): void {
    scrubWire(evt, this.scrubNeedles, this.scrubRegexes);
    const processed = this.opts.beforeSend ? this.opts.beforeSend(evt) : evt;
    if (!processed) return;
    if (this.queue.length >= this.opts.maxQueueSize) return;
    this.queue.push(processed);
  }

  /**
   * URL actual lista para publicar en telemetría. Por defecto se quita el
   * querystring y el hash para no filtrar tokens / emails / claves que
   * algunas apps ponen ahí. El cliente puede revertirlo seteando
   * `scrubUrlQuery: false` en las opciones.
   */
  private currentUrl(): string {
    if (typeof window === 'undefined') return '';
    const { location } = window;
    if (!this.opts.scrubUrlQuery) return location.href;
    return `${location.origin}${location.pathname}`;
  }

  private composeAttributes(extra?: Record<string, unknown>): Record<string, string> {
    const attrs: Record<string, string> = {};
    if (this.opts.attributes) {
      for (const [k, v] of Object.entries(this.opts.attributes)) attrs[k] = String(v);
    }
    if (this.sessionId) attrs['session.id'] = this.sessionId;
    if (this.opts.environment) attrs['deployment.environment'] = this.opts.environment;
    if (this.opts.release) attrs['service.version'] = this.opts.release;
    if (typeof window !== 'undefined') {
      attrs['browser.url'] = this.currentUrl();
      attrs['browser.userAgent'] = navigator.userAgent;
    }
    if (this.user) {
      if (this.user.id) attrs['user.id'] = this.user.id;
      if (this.user.email) attrs['user.email'] = this.user.email;
      if (this.user.username) attrs['user.name'] = this.user.username;
    }
    if (this.breadcrumbs.length > 0) {
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

  private installAutoCapture(): void {
    if (this.opts.autoCapture.pageViews) this.installAutoPageViews();
    if (
      this.opts.autoCapture.clicks ||
      this.opts.autoCapture.rageClicks ||
      this.opts.autoCapture.deadClicks
    ) {
      this.installAutoClickCapture();
    }
    if (this.opts.autoCapture.formSubmissions) this.installAutoFormSubmissions();
  }

  private installAutoPageViews(): void {
    this.emitAutoPageView('initial');

    const emit = (navigationType: string): void => this.emitAutoPageView(navigationType);
    const origPush = history.pushState;
    const origReplace = history.replaceState;

    history.pushState = function (this: History, ...args) {
      const ret = origPush.apply(this, args);
      emit('pushState');
      return ret;
    };
    history.replaceState = function (this: History, ...args) {
      const ret = origReplace.apply(this, args);
      emit('replaceState');
      return ret;
    };
    const onPop = (): void => emit('popstate');
    const onHash = (): void => emit('hashchange');
    window.addEventListener('popstate', onPop);
    window.addEventListener('hashchange', onHash);

    this.cleanup.push(() => {
      history.pushState = origPush;
      history.replaceState = origReplace;
      window.removeEventListener('popstate', onPop);
      window.removeEventListener('hashchange', onHash);
    });
  }

  private emitAutoPageView(navigationType: string): void {
    if (typeof window === 'undefined') return;
    this.page(window.location.pathname || '/', {
      url: this.currentUrl(),
      path: window.location.pathname || '/',
      referrer: typeof document !== 'undefined' ? document.referrer : '',
      navigation_type: navigationType,
    });
  }

  private installAutoClickCapture(): void {
    const onClick = (ev: MouseEvent): void => {
      const target = findAutoCaptureElement(ev.target);
      if (!target) return;
      const props = this.elementEventProperties(target);
      if (this.opts.autoCapture.clicks) {
        this.track('$autocapture', { type: 'click', ...props });
      }
      if (this.opts.autoCapture.rageClicks) {
        this.maybeTrackRageClick(target, props);
      }
      if (this.opts.autoCapture.deadClicks) {
        this.scheduleDeadClickCheck(target, props);
      }
    };
    document.addEventListener('click', onClick, { capture: true, passive: true });
    this.cleanup.push(() => document.removeEventListener('click', onClick, { capture: true }));
  }

  private installAutoFormSubmissions(): void {
    const onSubmit = (ev: Event): void => {
      const form = findFormWithFaro(ev.target);
      if (!form) return;
      const props = this.elementEventProperties(form);
      this.track('$form_submit', {
        type: 'form_submit',
        ...props,
        faro_form: safeGetAttribute(form, 'data-faro-form') ?? '',
      });
    };
    document.addEventListener('submit', onSubmit, { capture: true });
    this.cleanup.push(() => document.removeEventListener('submit', onSubmit, { capture: true }));
  }

  private maybeTrackRageClick(el: ElementLike, props: Record<string, unknown>): void {
    const key = elementFingerprint(el);
    const now = Date.now();
    const windowStart = now - 2000;
    const previous = this.rageClickState?.key === key ? this.rageClickState.times : [];
    const times = [...previous.filter((t) => t >= windowStart), now];
    this.rageClickState = { key, times };
    if (times.length < 3) return;
    this.track('$rage_click', {
      type: 'rage_click',
      ...props,
      click_count: times.length,
      window_ms: 2000,
    });
    this.rageClickState = { key, times: [] };
  }

  private scheduleDeadClickCheck(el: ElementLike, props: Record<string, unknown>): void {
    // Comparamos URL completas (incluyendo querystring) para detectar
    // navegaciones por SPA que sólo mutan query params — no es telemetría
    // saliente, así que no aplicamos el scrub.
    const startUrl = typeof window !== 'undefined' ? window.location.href : '';
    let mutated = false;
    let observer: MutationObserver | null = null;
    if (typeof MutationObserver !== 'undefined' && typeof document !== 'undefined' && document.body) {
      observer = new MutationObserver(() => { mutated = true; });
      observer.observe(document.body, { childList: true, subtree: true, attributes: true });
    }
    setTimeout(() => {
      observer?.disconnect();
      const currentUrl = typeof window !== 'undefined' ? window.location.href : '';
      if (mutated || currentUrl !== startUrl || this.closed) return;
      this.track('$dead_click', {
        type: 'dead_click',
        ...props,
        wait_ms: 700,
        target: elementFingerprint(el),
      });
    }, 700);
  }

  private elementEventProperties(el: ElementLike): Record<string, unknown> {
    const tag = (el.tagName ?? '').toLowerCase();
    const text = (el.textContent ?? '').trim().replace(/\s+/g, ' ').slice(0, 100);
    const href = typeof el.href === 'string' ? el.href : safeGetAttribute(el, 'href');
    const faro = safeGetAttribute(el, 'data-faro');
    const props: Record<string, unknown> = {
      tag,
      path: typeof window !== 'undefined' ? window.location.pathname : '',
      url: this.currentUrl(),
    };
    if (el.id) props.id = el.id;
    if (text) props.text = text;
    if (href) props.href = href;
    if (faro) props.faro = faro;
    return props;
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
    // Capturamos la URL respetando el scrub: las navegaciones SPA con
    // tokens en query string son un vector típico de leak vía breadcrumbs.
    const snapshot = (): string => this.currentUrl();
    const log = (from: string, to: string, method: string): void => {
      if (from === to) return;
      this.addBreadcrumb({ category: 'navigation', message: `${from} → ${to}`, data: { method, to } });
    };

    const origPush = history.pushState;
    const origReplace = history.replaceState;
    history.pushState = function (this: History, ...args) {
      const from = snapshot();
      const ret = origPush.apply(this, args);
      log(from, snapshot(), 'pushState');
      return ret;
    };
    history.replaceState = function (this: History, ...args) {
      const from = snapshot();
      const ret = origReplace.apply(this, args);
      log(from, snapshot(), 'replaceState');
      return ret;
    };
    const onPop = (): void => log('', snapshot(), 'popstate');
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

interface ElementLike {
  tagName?: string;
  id?: string;
  textContent?: string | null;
  href?: string;
  parentElement?: ElementLike | null;
  getAttribute?: (name: string) => string | null;
  hasAttribute?: (name: string) => boolean;
}

function asElementLike(value: unknown): ElementLike | null {
  if (!value || typeof value !== 'object') return null;
  const candidate = value as ElementLike;
  return typeof candidate.tagName === 'string' ? candidate : null;
}

function safeGetAttribute(el: ElementLike, name: string): string | null {
  try {
    return typeof el.getAttribute === 'function' ? el.getAttribute(name) : null;
  } catch {
    return null;
  }
}

function safeHasAttribute(el: ElementLike, name: string): boolean {
  try {
    return typeof el.hasAttribute === 'function'
      ? el.hasAttribute(name)
      : safeGetAttribute(el, name) !== null;
  } catch {
    return false;
  }
}

function findAutoCaptureElement(target: unknown): ElementLike | null {
  let el = asElementLike(target);
  while (el) {
    const tag = (el.tagName ?? '').toLowerCase();
    if (safeHasAttribute(el, 'data-faro') || tag === 'button' || tag === 'a') return el;
    el = el.parentElement ?? null;
  }
  return null;
}

function findFormWithFaro(target: unknown): ElementLike | null {
  let el = asElementLike(target);
  while (el) {
    if ((el.tagName ?? '').toLowerCase() === 'form' && safeHasAttribute(el, 'data-faro-form')) return el;
    el = el.parentElement ?? null;
  }
  return null;
}

function elementFingerprint(el: ElementLike): string {
  const tag = (el.tagName ?? '').toLowerCase();
  const id = el.id ?? '';
  const faro = safeGetAttribute(el, 'data-faro') ?? safeGetAttribute(el, 'data-faro-form') ?? '';
  const href = typeof el.href === 'string' ? el.href : safeGetAttribute(el, 'href') ?? '';
  const text = (el.textContent ?? '').trim().replace(/\s+/g, ' ').slice(0, 100);
  return [tag, id, faro, href, text].join('|');
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

let singleton: FaroBrowser | null = null;

export function init(opts: FaroBrowserOptions): FaroBrowser {
  // Paridad cross-SDK: Python/Go/Node/Flutter/Expo/Kotlin lanzan un error claro en el mismo caso.
  // Validamos aquí (antes del FaroBrowser) para que SSR tampoco trague el error silenciosamente.
  if (!opts || typeof opts.endpoint !== 'string' || opts.endpoint.length === 0) {
    throw new Error("faro.init: 'endpoint' es obligatorio (string no vacío)");
  }
  if (typeof opts.token !== 'string' || opts.token.length === 0) {
    throw new Error("faro.init: 'token' es obligatorio (string no vacío)");
  }
  if (typeof opts.service !== 'string' || opts.service.length === 0) {
    throw new Error("faro.init: 'service' es obligatorio (string no vacío)");
  }
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
export function warning(msg: string, attrs?: Record<string, unknown>): void { getClient().warning(msg, attrs); }
export function error(msg: string, attrs?: Record<string, unknown>): void { getClient().error(msg, attrs); }
export function captureException(err: unknown, ctx?: { tags?: Record<string, string>; message?: string }): void {
  getClient().captureException(err, ctx);
}
export function setUser(user: UserContext | null): void { getClient().setUser(user); }
export function addBreadcrumb(crumb: Omit<Breadcrumb, 'timestamp'>): void { getClient().addBreadcrumb(crumb); }
export function flush(): Promise<void> { return getClient().flush(); }
export function close(): void { getClient().close(); }
/** Identificador de la sesión actual del tab. Útil para enriquecer logs propios
 *  o construir un link al replay desde un mensaje de soporte. */
export function getSessionId(): string { return getClient().getSessionId(); }
export function track(event: string, properties?: Record<string, unknown>): void {
  getClient().track(event, properties);
}
export function identify(userId: string, traits?: Record<string, unknown>): void {
  getClient().identify(userId, traits);
}
export function page(path?: string, properties?: Record<string, unknown>): void {
  getClient().page(path, properties);
}
export function alias(prevId: string, newId: string): void {
  getClient().alias(prevId, newId);
}
export function refreshFeatureFlags(): Promise<void> { return getClient().refreshFeatureFlags(); }
export function isFeatureEnabled(key: string, context?: FeatureFlagContext): boolean {
  return getClient().isFeatureEnabled(key, context);
}

export { FaroBrowser };
