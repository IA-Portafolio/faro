/**
 * SDK de Faro para Expo / React Native.
 *
 * Usa fetch (RN lo incluye) y ErrorUtils.setGlobalHandler para captura nativa
 * de errores no atrapados. Sin módulos nativos → funciona en Expo Go sin
 * un development client personalizado.
 */

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

export interface FaroPersistenceOptions {
  /** Clave AsyncStorage. Default: `@faro/queue/{service}` */
  key?: string;
  /** Tiempo máximo que se conservan eventos en disco. Default 24h. */
  ttlMs?: number;
  /** Tamaño máximo del payload persistido en bytes. Si se excede al persistir,
   *  se recortan los eventos más antiguos. Default 256 KB. */
  maxBytes?: number;
}

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
  /** Cadencia de refresh de feature flags en ms (por defecto 30_000). */
  featureFlagRefreshIntervalMs?: number;
  /** Substrings case-insensitive: cualquier atributo cuya clave los contenga se redacta. Default: lista común de campos sensibles. */
  scrubFields?: string[];
  /** Si true, suma headers comunes (authorization, cookie, set-cookie) a scrubFields. Default: true. */
  scrubHeaders?: boolean;
  /** Presets de regex aplicados a values string y al message. Default: ['jwt','api-key']. */
  scrubPatterns?: ScrubPreset[];
  /** Hook post-scrub; devolver null descarta el evento. */
  beforeSend?: (event: Wire) => Wire | null;
  /** Persistir cola en AsyncStorage para sobrevivir kill agresivo del SO.
   *  `true` (default) usa los parámetros por defecto; `false` desactiva.
   *  Requiere `@react-native-async-storage/async-storage` como peer dep. */
  persistence?: boolean | FaroPersistenceOptions;
}

// ErrorUtils es un global de React Native, no está en los tipos TS normales.
declare const ErrorUtils:
  | {
      setGlobalHandler: (h: (err: Error, isFatal?: boolean) => void) => void;
      getGlobalHandler: () => (err: Error, isFatal?: boolean) => void;
    }
  | undefined;

/** Payload exacto que sale por la red (post-scrub). Lo que recibe `beforeSend`. */
export interface Wire {
  level: Severity;
  message: string;
  timestamp: string;
  attributes: Record<string, string>;
}

/** Tipo de evento product API (Segment/PostHog-like). */
export type ProductEventType = 'track' | 'identify' | 'screen' | 'alias';

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

// ---------- Feature flags ----------

interface FeatureFlagsResponse {
  project?: string;
  flags?: FeatureFlagWire[];
}

// ---- Persistencia en AsyncStorage ----
//
// El SO de mobile puede matar la app sin preaviso (memoria baja, swipe del
// task switcher, OOM). Si la cola está en memoria volátil, se pierde todo.
// Esta capa serializa la cola a AsyncStorage en momentos clave (background,
// close, fatal) y la recarga en el próximo init.

interface AsyncStorageLike {
  getItem(key: string): Promise<string | null>;
  setItem(key: string, value: string): Promise<void>;
  removeItem(key: string): Promise<void>;
}

interface PersistedEnvelope {
  savedAt: number;
  items: Wire[];
}

function loadAsyncStorage(): AsyncStorageLike | null {
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports, @typescript-eslint/no-var-requires
    const mod = require('@react-native-async-storage/async-storage');
    // El paquete exporta default = AsyncStorage instance.
    const candidate: unknown = mod?.default ?? mod;
    if (candidate && typeof (candidate as AsyncStorageLike).getItem === 'function') {
      return candidate as AsyncStorageLike;
    }
  } catch {
    /* peer dep no instalada */
  }
  return null;
}

class Persistence {
  readonly storage: AsyncStorageLike;
  readonly key: string;
  readonly ttlMs: number;
  readonly maxBytes: number;

  constructor(
    storage: AsyncStorageLike,
    key: string,
    opts: FaroPersistenceOptions | undefined,
  ) {
    this.storage = storage;
    this.key = opts?.key ?? key;
    this.ttlMs = opts?.ttlMs ?? 24 * 60 * 60 * 1000;
    this.maxBytes = opts?.maxBytes ?? 256 * 1024;
  }

  /** Recupera lo persistido, descartando si superó el TTL. Vacía la entry tras leer. */
  async load(): Promise<Wire[]> {
    try {
      const raw = await this.storage.getItem(this.key);
      if (!raw) return [];
      const env = JSON.parse(raw) as PersistedEnvelope;
      // Limpieza inmediata: si el flush falla, lo re-persiste el siguiente background hook;
      // y si la app cae antes, mejor perder un batch que mandar logs de hace una semana.
      await this.storage.removeItem(this.key);
      if (!env || typeof env.savedAt !== 'number' || !Array.isArray(env.items)) return [];
      if (Date.now() - env.savedAt > this.ttlMs) return [];
      return env.items;
    } catch {
      return [];
    }
  }

  /** Persiste la cola actual. Recorta los más antiguos si excede maxBytes. */
  async save(items: Wire[]): Promise<void> {
    if (items.length === 0) {
      await this.clear();
      return;
    }
    let pending = items;
    let serialized = JSON.stringify({ savedAt: Date.now(), items: pending });
    // Reduce a la mitad iterativamente — barato y limita el peor caso.
    while (serialized.length > this.maxBytes && pending.length > 1) {
      pending = pending.slice(Math.ceil(pending.length / 2));
      serialized = JSON.stringify({ savedAt: Date.now(), items: pending });
    }
    try {
      await this.storage.setItem(this.key, serialized);
    } catch {
      /* AsyncStorage lleno o IO error — no podemos hacer mucho */
    }
  }

  async clear(): Promise<void> {
    try {
      await this.storage.removeItem(this.key);
    } catch {
      /* noop */
    }
  }
}

interface ResolvedOptions
  extends Required<
    Omit<FaroExpoOptions, 'attributes' | 'environment' | 'release' | 'beforeSend' | 'persistence'>
  > {
  attributes?: FaroExpoOptions['attributes'];
  environment?: FaroExpoOptions['environment'];
  release?: FaroExpoOptions['release'];
  beforeSend?: FaroExpoOptions['beforeSend'];
  persistence: boolean | FaroPersistenceOptions;
}

class FaroExpoClient {
  private queue: Wire[] = [];
  private eventsQueue: ProductEventWire[] = [];
  private timer: ReturnType<typeof setInterval> | null = null;
  private featureFlagsTimer: ReturnType<typeof setInterval> | null = null;
  private closed = false;
  private prevHandler: ((err: Error, isFatal?: boolean) => void) | null = null;
  private scrubNeedles: string[];
  private scrubRegexes: RegExp[];
  /** Cleanup del listener de AppState (RN 0.65+ devuelve un subscription con remove()) */
  private appStateSub: { remove: () => void } | null = null;
  private persistence: Persistence | null = null;
  /** Promise de la carga inicial — close()/flush() esperan a que termine para no
   *  perder los eventos restaurados que aún no han llegado a la cola. */
  private restorePromise: Promise<void> | null = null;
  private distinctId = '';
  /** anonymous_id estable. En Expo no hay localStorage; lo generamos en boot. */
  private anonymousId = `anon_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`;
  private userProperties: Record<string, unknown> = {};
  /** userId del último identify/alias. Si está seteado, los logs llevan `user.id`. */
  private currentUserId = '';
  private featureFlags = new Map<string, FeatureFlagWire>();
  private featureFlagsProject = '';
  private featureExposureSeen = new Set<string>();

  constructor(private opts: ResolvedOptions) {
    this.scrubNeedles = Array.from(new Set([
      ...this.opts.scrubFields.map((s) => s.toLowerCase()),
      ...(this.opts.scrubHeaders ? HEADER_SCRUB_FIELDS : []),
    ]));
    this.scrubRegexes = this.opts.scrubPatterns.map((p) => SCRUB_REGEXES[p]).filter(Boolean);

    if (this.opts.persistence !== false) {
      const storage = loadAsyncStorage();
      if (storage) {
        const persistOpts = typeof this.opts.persistence === 'object' ? this.opts.persistence : undefined;
        this.persistence = new Persistence(storage, `@faro/queue/${this.opts.service}`, persistOpts);
        this.restorePromise = this.restoreFromStorage();
      }
    }

    this.timer = setInterval(() => void this.flush(), this.opts.flushIntervalMs);
    // En Node (tests, SSR) permite al proceso salir aunque el timer sea lo único que quede.
    // En React Native es no-op (timer no implementa unref). Paridad con SDK Node.
    if (typeof (this.timer as { unref?: () => void }).unref === 'function') {
      (this.timer as { unref: () => void }).unref();
    }
    // Refresh periódico de feature flags. A diferencia del SDK Node NO llamamos
    // unref (el timer de RN no lo implementa) ni hacemos un fetch inicial — la
    // primera evaluación tras boot usa el snapshot vacío hasta el primer tick.
    this.featureFlagsTimer = setInterval(
      () => void this.refreshFeatureFlags(),
      this.opts.featureFlagRefreshIntervalMs,
    );
    if (this.opts.installGlobalHandlers) this.installHandlers();
  }

  /** Drena los eventos persistidos en el constructor anterior (o crash previo). */
  private async restoreFromStorage(): Promise<void> {
    if (!this.persistence) return;
    const restored = await this.persistence.load();
    if (restored.length === 0) return;
    // Prepend: los pendientes son más viejos que los del runtime actual.
    // Respeta el cap de cola — si excede, descartamos los más viejos.
    const room = Math.max(0, this.opts.maxQueueSize - this.queue.length);
    const toPrepend = restored.slice(-room);
    this.queue = [...toPrepend, ...this.queue];
    // No esperamos al próximo tick: drenar ahora para que un flush rápido los saque.
    void this.flush();
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
    // identify(userId) enriquece los logs siguientes con user.id (contrato
    // sdks/README.md). El valor explícito del caller gana: solo lo seteamos
    // si aún no vino en attrs.
    if (this.currentUserId && !('user.id' in attrs)) {
      attrs['user.id'] = this.currentUserId;
    }
    const wire: Wire = {
      level: entry.level ?? 'INFO',
      message: entry.message,
      timestamp: new Date().toISOString(),
      attributes: attrs,
    };
    scrubWire(wire, this.scrubNeedles, this.scrubRegexes);
    const final = this.opts.beforeSend ? this.opts.beforeSend(wire) : wire;
    if (!final) return;
    if (this.queue.length >= this.opts.maxQueueSize) return; // descartar silenciosamente
    this.queue.push(final);
  }

  info(m: string, a?: Record<string, unknown>): void { this.log({ level: 'INFO', message: m, attributes: a }); }
  warn(m: string, a?: Record<string, unknown>): void { this.log({ level: 'WARN', message: m, attributes: a }); }
  /** Alias de `warn` — paridad con `logging.WARNING` / SDK de Python. */
  warning(m: string, a?: Record<string, unknown>): void { this.log({ level: 'WARN', message: m, attributes: a }); }
  error(m: string, a?: Record<string, unknown>): void { this.log({ level: 'ERROR', message: m, attributes: a }); }

  // ---------- Product events API (Segment/PostHog-like) ----------

  track(eventName: string, properties: Record<string, unknown> = {}): void {
    this.enqueueEvent({ type: 'track', name: eventName, properties });
  }

  identify(userId: string, traits: Record<string, unknown> = {}): void {
    if (!userId) return;
    this.distinctId = userId;
    this.currentUserId = userId;
    this.userProperties = { ...this.userProperties, ...traits };
    this.enqueueEvent({
      type: 'identify',
      name: '$identify',
      properties: {},
      userPropertiesOverride: traits,
    });
  }

  /** Mobile-only: marca una transición de pantalla (equivalente al `page()` web). */
  screen(screenName: string, properties: Record<string, unknown> = {}): void {
    this.enqueueEvent({ type: 'screen', name: screenName, properties });
  }

  alias(prevId: string, newId: string): void {
    if (!prevId || !newId) return;
    this.distinctId = newId;
    this.currentUserId = newId;
    this.enqueueEvent({
      type: 'alias',
      name: '$alias',
      properties: {},
      anonymousIdOverride: prevId,
    });
  }

  private enqueueEvent(input: {
    type: ProductEventType;
    name: string;
    properties: Record<string, unknown>;
    userPropertiesOverride?: Record<string, unknown>;
    anonymousIdOverride?: string;
    /** Sobrescribe `distinct_id` (lo usa feature exposure con context.distinct_id). */
    distinctIdOverride?: string;
  }): void {
    if (this.closed) return;
    if (this.eventsQueue.length >= this.opts.maxQueueSize) return;
    const ctx: Record<string, unknown> = {};
    if (this.opts.environment) ctx.environment = this.opts.environment;
    if (this.opts.release) ctx.release = this.opts.release;
    if (this.opts.attributes) Object.assign(ctx, this.opts.attributes);
    this.eventsQueue.push({
      type: input.type,
      name: input.name,
      timestamp: new Date().toISOString(),
      distinct_id: input.distinctIdOverride ?? (this.distinctId || this.anonymousId),
      anonymous_id: input.anonymousIdOverride ?? this.anonymousId,
      session_id: '',
      properties: input.properties,
      user_properties: input.userPropertiesOverride ?? this.userProperties,
      context: ctx,
      source: 'mobile',
    });
  }

  // ---------- Feature flags (port idéntico al SDK Node) ----------

  async refreshFeatureFlags(): Promise<void> {
    if (this.closed) return;
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
    } catch (_e) {
      /* fallo de red — reintenta en el próximo tick */
    }
  }

  isFeatureEnabled(key: string, context: FeatureFlagContext = {}): boolean {
    const flag = this.featureFlags.get(key);
    if (!flag) return false;
    if (!matchesFeatureConditions(flag, context)) return false;
    const rollout = clampRollout(flag.rollout_percentage);
    const id = context.distinct_id || this.distinctId || this.anonymousId;
    const enabled = rollout >= 100
      ? true
      : rollout > 0 && stickyBucket(`${this.featureFlagsProject}:${key}:${id}`) < rollout;
    this.trackFeatureExposure(key, id, enabled);
    return enabled;
  }

  private trackFeatureExposure(flagKey: string, distinctId: string, enabled: boolean): void {
    const variant = enabled ? 'B' : 'A';
    const key = `${this.featureFlagsProject}:${flagKey}:${distinctId}:${variant}`;
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
    await Promise.all([this.flushLogs(), this.flushEvents()]);
  }

  private async flushLogs(): Promise<void> {
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
      this.queue.unshift(...batch); // fallo de red — los conservamos para el siguiente tick
    }
  }

  private async flushEvents(): Promise<void> {
    if (this.eventsQueue.length === 0) return;
    const batch = this.eventsQueue.splice(0, this.opts.maxBatchSize);
    try {
      const res = await fetch(`${this.opts.endpoint}/api/v1/ingest/events`, {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${this.opts.token}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ service: this.opts.service, events: batch }),
      });
      if (!res.ok && res.status >= 500) this.eventsQueue.unshift(...batch);
    } catch (_e) {
      this.eventsQueue.unshift(...batch);
    }
  }

  /**
   * Drena las colas y cierra. El `timeoutMs` acota el peor caso (red caída +
   * cola llena): el loop de drenado rompe si vence el deadline o si la cola no
   * progresa entre flushes. Limpia TODOS los timers (flush + feature flags) y
   * desinstala los handlers (ErrorUtils/AppState).
   */
  async close(timeoutMs = 5000): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    if (this.featureFlagsTimer) clearInterval(this.featureFlagsTimer);
    this.featureFlagsTimer = null;
    if (this.prevHandler && typeof ErrorUtils !== 'undefined') {
      ErrorUtils.setGlobalHandler(this.prevHandler);
    }
    if (this.appStateSub) {
      this.appStateSub.remove();
      this.appStateSub = null;
    }
    // Espera a que termine la restauración para no descartar eventos persistidos.
    if (this.restorePromise) {
      try { await this.restorePromise; } catch { /* noop */ }
    }
    // Drena nuestras colas con cota dura — igual que el SDK Node, pero RN no
    // siempre soporta AbortController en fetch, así que cada iteración corre
    // contra el tiempo restante con un Promise.race para que un fetch colgado
    // no bloquee el close más allá del deadline.
    const deadline = Date.now() + timeoutMs;
    while (
      (this.queue.length > 0 || this.eventsQueue.length > 0) &&
      Date.now() < deadline
    ) {
      const before = this.queue.length + this.eventsQueue.length;
      const remaining = Math.max(0, deadline - Date.now());
      const flushed = await Promise.race([
        Promise.all([this.flushLogs(), this.flushEvents()]).then(() => true),
        new Promise<boolean>((resolve) => setTimeout(() => resolve(false), remaining)),
      ]);
      if (!flushed) break; // venció el deadline con un flush en vuelo
      const after = this.queue.length + this.eventsQueue.length;
      if (after >= before) break; // probablemente la red esté caída; rendirse
    }
    // Lo que quede sin enviar (red caída, server 5xx) se persiste para el próximo arranque.
    if (this.persistence) {
      await this.persistence.save(this.queue);
    }
  }

  /** Persiste el estado actual SIN cerrar — usado al pasar a background. */
  async persistNow(): Promise<void> {
    if (!this.persistence) return;
    await this.persistence.save(this.queue);
  }

  private installHandlers(): void {
    if (typeof ErrorUtils !== 'undefined') {
      this.prevHandler = ErrorUtils.getGlobalHandler();
      ErrorUtils.setGlobalHandler((err, isFatal) => {
        this.captureException(err, { isFatal });
        // En fatal, persistir antes que flushear: el flush puede no llegar a completarse
        // si el runtime cae de inmediato, pero persistNow() escribe a AsyncStorage de
        // forma atómica (Android: SharedPreferences; iOS: NSUserDefaults file).
        if (isFatal) {
          void this.persistNow();
        }
        // flush síncrono best-effort — no podemos hacer await, pero el keepalive ayuda
        void this.flush();
        this.prevHandler?.(err, isFatal);
      });
    }
    this.installAppStateHook();
  }

  /** AppState 'background'/'inactive': el SO puede congelar/matar la app en cualquier
   *  momento. Flush primero (en best-effort) y luego persistimos lo que no salió. */
  private installAppStateHook(): void {
    // Resolución dinámica para no depender de react-native en tipo: el SDK debe
    // poder cargar también en entornos donde RN no está (tests, web Expo).
    let RNAppState: {
      addEventListener: (
        type: 'change',
        h: (s: string) => void,
      ) => { remove: () => void } | undefined;
    } | undefined;
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports, @typescript-eslint/no-var-requires
      RNAppState = require('react-native').AppState;
    } catch {
      return; // RN no disponible — saltarse el hook silenciosamente
    }
    if (!RNAppState) return;
    const sub = RNAppState.addEventListener('change', (state) => {
      if (state === 'background' || state === 'inactive') {
        // flush primero (red posiblemente disponible aún), persistir el resto.
        void this.flush().then(() => this.persistNow());
      }
    });
    // RN ≥0.65 devuelve subscription con .remove(); APIs antiguas devolvían void.
    if (sub && typeof sub.remove === 'function') {
      this.appStateSub = sub;
    }
  }
}

let singleton: FaroExpoClient | null = null;

export function init(opts: FaroExpoOptions): FaroExpoClient {
  // Paridad cross-SDK: Python/Go/Node/Flutter lanzan un error claro en el mismo caso.
  if (!opts || typeof opts.endpoint !== 'string' || opts.endpoint.length === 0) {
    throw new Error("faro.init: 'endpoint' es obligatorio (string no vacío)");
  }
  if (typeof opts.token !== 'string' || opts.token.length === 0) {
    throw new Error("faro.init: 'token' es obligatorio (string no vacío)");
  }
  if (typeof opts.service !== 'string' || opts.service.length === 0) {
    throw new Error("faro.init: 'service' es obligatorio (string no vacío)");
  }
  if (singleton) singleton.close().catch(() => undefined);
  singleton = new FaroExpoClient({
    endpoint: opts.endpoint.replace(/\/$/, ''),
    token: opts.token,
    service: opts.service,
    environment: opts.environment,
    release: opts.release,
    attributes: opts.attributes,
    // Perfil de defaults: "mobile" (sdks/README.md → Perfiles de defaults).
    // Algo más conservador que el baseline mobile por el coste del bridge JS↔nativo y batería.
    flushIntervalMs: opts.flushIntervalMs ?? 2500,
    maxBatchSize: opts.maxBatchSize ?? 80,
    maxQueueSize: opts.maxQueueSize ?? 2000,
    installGlobalHandlers: opts.installGlobalHandlers ?? true,
    featureFlagRefreshIntervalMs: opts.featureFlagRefreshIntervalMs ?? 30_000,
    scrubFields: opts.scrubFields ?? DEFAULT_SCRUB_FIELDS,
    scrubHeaders: opts.scrubHeaders ?? true,
    scrubPatterns: opts.scrubPatterns ?? (['jwt', 'api-key'] as ScrubPreset[]),
    beforeSend: opts.beforeSend,
    persistence: opts.persistence ?? true,
  });
  return singleton;
}

function need(): FaroExpoClient {
  if (!singleton) throw new Error('faro: hay que llamar a init() antes de usarlo');
  return singleton;
}

export const info = (m: string, a?: Record<string, unknown>) => need().info(m, a);
export const warn = (m: string, a?: Record<string, unknown>) => need().warn(m, a);
export const warning = (m: string, a?: Record<string, unknown>) => need().warning(m, a);
export const error = (m: string, a?: Record<string, unknown>) => need().error(m, a);
export const captureException = (
  err: unknown,
  ctx?: { tags?: Record<string, string>; message?: string },
) => need().captureException(err, ctx);
export const track = (event: string, properties?: Record<string, unknown>) =>
  need().track(event, properties);
export const identify = (userId: string, traits?: Record<string, unknown>) =>
  need().identify(userId, traits);
export const screen = (name: string, properties?: Record<string, unknown>) =>
  need().screen(name, properties);
export const alias = (prevId: string, newId: string) => need().alias(prevId, newId);
export const refreshFeatureFlags = (): Promise<void> => need().refreshFeatureFlags();
export const isFeatureEnabled = (key: string, context?: FeatureFlagContext): boolean =>
  need().isFeatureEnabled(key, context);
export const flush = () => need().flush();
export const close = (timeoutMs?: number) => need().close(timeoutMs);
