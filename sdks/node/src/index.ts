/**
 * SDK de Faro para Node.js / TypeScript.
 *
 * v0.2.0: el tracing pasa a estar respaldado por `@opentelemetry/sdk-node` +
 * `@opentelemetry/auto-instrumentations-node`. La API pública de spans
 * (`startSpan` / `withSpan` / `activeSpan` / `Span`) se conserva intacta pero
 * por dentro envuelve `@opentelemetry/api`. Esto desbloquea Service Map y la
 * pestaña Trazas para servicios que no instrumentaban manualmente: con sólo
 * llamar a `faro.init(...)` se obtienen spans de http, express, fastify, koa,
 * pg, mongodb, redis, ioredis, grpc, kafka, …
 *
 * Para que la auto-instrumentación funcione en Node, OTel debe inicializarse
 * antes de que se importen las librerías a instrumentar. El camino recomendado
 * es `node --import @iaportafolio/node/instrument server.js` (lee FARO_*
 * env-vars). Si el user llama a `faro.init(...)` en código, debe hacerlo en
 * la primera línea del entrypoint.
 *
 * Logs / errores / product events siguen viajando por HTTP nativo a
 * `/api/v1/ingest/*` — esa parte no cambia. Lo que cambia es que ahora trae
 * trace_id y span_id correlacionados con los spans OTel automáticos.
 */

import {
  trace,
  context as otelContext,
  SpanKind as OtelSpanKind,
  SpanStatusCode as OtelStatusCode,
  type Span as OtelSpan,
  type Context as OtelContext,
} from '@opentelemetry/api';

import {
  initTracing,
  shutdownTracing,
  flushTracing,
  getTracer,
} from './tracing.js';

export { initTracing, shutdownTracing, flushTracing, getTracer } from './tracing.js';

export type Severity = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR' | 'FATAL';

/** Tipo de span según OTLP. INTERNAL es el default — para operaciones in-process.
 *  SERVER/CLIENT son para bordes RPC/HTTP. PRODUCER/CONSUMER para colas. */
export type SpanKind = 'INTERNAL' | 'SERVER' | 'CLIENT' | 'PRODUCER' | 'CONSUMER';

/** Estado de un span según OTLP. UNSET es el default; setStatus('OK'/'ERROR') lo cambia. */
export type SpanStatusCode = 'UNSET' | 'OK' | 'ERROR';

const OTEL_SPAN_KIND: Record<SpanKind, OtelSpanKind> = {
  INTERNAL: OtelSpanKind.INTERNAL,
  SERVER: OtelSpanKind.SERVER,
  CLIENT: OtelSpanKind.CLIENT,
  PRODUCER: OtelSpanKind.PRODUCER,
  CONSUMER: OtelSpanKind.CONSUMER,
};

const OTEL_STATUS_CODE: Record<SpanStatusCode, OtelStatusCode> = {
  UNSET: OtelStatusCode.UNSET,
  OK: OtelStatusCode.OK,
  ERROR: OtelStatusCode.ERROR,
};

export type ScrubPreset = 'email' | 'jwt' | 'credit-card' | 'api-key';

export interface TraceContext {
  trace_id?: string;
  span_id?: string;
  traceparent?: string;
}

export type TraceContextProvider = () => TraceContext | string | null | undefined;

export interface FaroOptions {
  /** URL base de Faro, p. ej. https://faro.iaportafolio.com */
  endpoint: string;
  /** Token de ingesta del proyecto (desde la página /projects del dashboard de Faro) */
  token: string;
  /** service.name de OTel adjuntado a cada evento y span */
  service: string;
  /** p. ej. "production" / "staging" — se añade como atributo `deployment.environment` */
  environment?: string;
  /** Release / commit / tag — se añade como `service.version` */
  release?: string;
  /** Atributos por defecto que se mezclan en cada evento */
  attributes?: Record<string, string | number | boolean>;
  /** Cadencia de flush en ms para LOGS y EVENTS (por defecto 750). Spans usan el batch processor de OTel. */
  flushIntervalMs?: number;
  /** Máximo de eventos por lote HTTP (por defecto 200) */
  maxBatchSize?: number;
  /** Descarta nuevos eventos al superar este tamaño de cola en memoria (por defecto 10_000) */
  maxQueueSize?: number;
  /** Instala handlers de error a nivel de proceso (por defecto true). Desactivar en uso embebido. */
  installGlobalHandlers?: boolean;
  /** Logger para las advertencias internas del SDK. Por defecto console.warn. */
  diag?: (msg: string, err?: unknown) => void;
  /** Substrings case-insensitive: cualquier atributo cuya clave los contenga se redacta antes de salir.
   *  Default: ['password','token','secret','authorization','cookie','set-cookie','api_key','apikey']. */
  scrubFields?: string[];
  /** Si true, suma headers comunes (authorization, cookie, set-cookie) al matcheo de scrubFields. Default: true. */
  scrubHeaders?: boolean;
  /** Presets de regex aplicados a values string y al message. Default: ['jwt','api-key']. */
  scrubPatterns?: ScrubPreset[];
  /** Hook para muestrear/transformar/redactar tras el scrubbing. Devolver null descarta el evento. */
  beforeSend?: (entry: Wire) => Wire | null;
  /** Proveedor explícito para auto-correlación W3C tracecontext en logs/events. */
  traceContext?: TraceContextProvider;
  /** Cadencia de refresh de feature flags en ms (por defecto 30_000). */
  featureFlagRefreshIntervalMs?: number;
  /** Inicializa OpenTelemetry + auto-instrumentación al boot. Default: true.
   *  Cuando es true, los spans de HTTP/Express/Fastify/Koa/pg/mongo/redis/grpc/kafka… se exportan
   *  por OTLP a `${endpoint}/v1/traces` y Service Map se llena solo.
   *  Si OTel ya fue inicializado via `--import @iaportafolio/node/instrument`, esta llamada es no-op. */
  enableTracing?: boolean;
  /** Override del endpoint completo de traces. Default: `${endpoint}/v1/traces`. */
  tracesEndpoint?: string;
  /** Atributos extra para el Resource OTel (se mergean con service.name/version/environment). */
  resourceAttributes?: Record<string, string>;
  /** Nombres exactos de paquetes de instrumentación a desactivar.
   *  Por defecto el SDK desactiva fs/dns/net (ruidosos). */
  disabledInstrumentations?: string[];
}

// ---------- Tracing API ----------

export interface SpanOptions {
  /** Kind del span. Default 'INTERNAL'. */
  kind?: SpanKind;
  /** Atributos iniciales del span. */
  attributes?: Record<string, unknown>;
  /** Contexto padre explícito. Si se omite, se toma del current span en
   *  el ContextManager de OTel (AsyncHooks). Pasar `null` fuerza root span. */
  parent?: TraceContext | string | null;
  /** Timestamp de inicio. Default `new Date()`. */
  startTime?: Date;
}

export interface SpanEventInput {
  attributes?: Record<string, unknown>;
  timestamp?: Date;
}

export interface SpanEndOptions {
  /** Atributos a mezclar antes de cerrar. */
  attributes?: Record<string, unknown>;
  /** Timestamp de fin. Default `new Date()`. */
  endTime?: Date;
}

/** Handle público devuelto por `startSpan`. */
export interface Span {
  /** trace_id (32 hex) + span_id (16 hex) — usar para propagación o correlación de logs. */
  spanContext(): { trace_id: string; span_id: string };
  /** W3C traceparent header listo para propagar a HTTP outbound. */
  traceparent(): string;
  setAttribute(key: string, value: unknown): void;
  setAttributes(attrs: Record<string, unknown>): void;
  addEvent(name: string, opts?: SpanEventInput): void;
  setStatus(code: SpanStatusCode, message?: string): void;
  /** Marca status=ERROR y añade attrs `exception.*`. No re-lanza. */
  recordException(err: unknown): void;
  /** Cierra el span. La exportación la maneja el BatchSpanProcessor de OTel. Idempotente. */
  end(opts?: SpanEndOptions): void;
  /** True si end() ya se llamó. */
  readonly ended: boolean;
}

export interface LogEntry {
  level?: Severity;
  message: string;
  attributes?: Record<string, unknown>;
  trace_id?: string;
  span_id?: string;
  timestamp?: Date;
}

// ---------- Metrics API ----------

export type MetricKind = 'counter' | 'sum' | 'gauge' | 'histogram';
export type MetricAttrs = Record<string, string | number | boolean>;

export interface MetricInstrumentOptions {
  /** Unidad p.ej. `ms`, `By`, `1`. Se guarda como `metric_unit` y sirve al frontend para formatear. */
  unit?: string;
}

/** Contador monotónico (sólo sube). Lo típico para "request count", "errors total", "bytes processed". */
export interface Counter {
  add(value: number, attributes?: MetricAttrs): void;
}

/** Contador no monotónico — `add()` puede ser negativo. Para "conexiones abiertas", "items en cola". */
export interface UpDownCounter {
  add(value: number, attributes?: MetricAttrs): void;
}

/** Gauge: valor puntual. Cada `set()` reemplaza, no acumula. */
export interface Gauge {
  set(value: number, attributes?: MetricAttrs): void;
}

/** Histograma: cada `record()` envía un data point individual al backend. El SDK NO agrega
 *  bucket counts en cliente — para eso usar OpenTelemetry SDK + OTLP. */
export interface Histogram {
  record(value: number, attributes?: MetricAttrs): void;
}

/** Payload wire para `POST /api/v1/ingest/metrics`. */
export interface MetricWire {
  name: string;
  kind: MetricKind;
  unit?: string;
  timestamp: string;
  service?: string;
  value?: number;
  count?: number;
  sum?: number;
  min?: number;
  max?: number;
  attributes: Record<string, string>;
}

/** Tipo de evento product (API tipo Segment/PostHog). */
export type ProductEventType = 'track' | 'identify' | 'page' | 'screen' | 'alias';

/** Payload wire para `POST /api/v1/ingest/events`. */
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
  trace_id?: string;
  span_id?: string;
}

export interface FeatureFlagContext {
  distinct_id?: string;
  properties?: Record<string, unknown>;
}

export interface FeatureFlagWire {
  key: string;
  rollout_percentage: number;
  conditions?: {
    properties?: Record<string, unknown>;
  } & Record<string, unknown>;
}

interface FeatureFlagsResponse {
  project?: string;
  flags?: FeatureFlagWire[];
}

/** Payload exacto que sale por la red (post-merge de atributos + post-scrub).
 *  Es lo que recibe `beforeSend` y lo que se puede mutar/descartar. */
export interface Wire {
  level: Severity;
  message: string;
  timestamp: string;
  service?: string;
  trace_id?: string;
  span_id?: string;
  attributes: Record<string, string>;
}

const DEFAULT_SCRUB_FIELDS = [
  'password', 'token', 'secret', 'authorization', 'cookie', 'set-cookie', 'api_key', 'apikey',
];
const HEADER_SCRUB_FIELDS = ['authorization', 'cookie', 'set-cookie'];
const REDACTED = '[REDACTED]';

const SCRUB_REGEXES: Record<ScrubPreset, RegExp> = {
  'email': /[\w.+-]+@[\w-]+(?:\.[\w-]+)+/g,
  'jwt': /\beyJ[\w-]+\.[\w-]+\.[\w-]+\b/g,
  // Sin Luhn; puede tener falsos positivos en IDs largos. Opt-in deliberadamente.
  'credit-card': /\b(?:\d[ -]?){13,19}\b/g,
  'api-key': /\b(?:sk-|ghp_|ghs_|gho_|github_pat_|xoxb-|xoxp-|xoxs-|AKIA|ASIA|AIza)[\w-]{12,}\b/g,
};

function scrubString(s: string, regexes: RegExp[]): string {
  let out = s;
  for (const re of regexes) out = out.replace(re, REDACTED);
  return out;
}

function scrubWire(wire: Wire, fieldNeedles: string[], regexes: RegExp[]): void {
  for (const key of Object.keys(wire.attributes)) {
    const kLower = key.toLowerCase();
    if (fieldNeedles.some((n) => kLower.includes(n))) {
      wire.attributes[key] = REDACTED;
    } else if (regexes.length > 0) {
      wire.attributes[key] = scrubString(wire.attributes[key], regexes);
    }
  }
  if (regexes.length > 0) wire.message = scrubString(wire.message, regexes);
}

export function parseTraceparent(traceparent: string): TraceContext | null {
  const match = traceparent.trim().match(
    /^[\da-fA-F]{2}-([\da-fA-F]{32})-([\da-fA-F]{16})-[\da-fA-F]{2}(?:-.+)?$/,
  );
  if (!match) return null;
  const traceId = match[1].toLowerCase();
  const spanId = match[2].toLowerCase();
  if (/^0+$/.test(traceId) || /^0+$/.test(spanId)) return null;
  return { trace_id: traceId, span_id: spanId };
}

function normalizeTraceContext(input: TraceContext | string | null | undefined): TraceContext | null {
  if (!input) return null;
  if (typeof input === 'string') return parseTraceparent(input);
  if (typeof input.traceparent === 'string') {
    const parsed = parseTraceparent(input.traceparent);
    if (parsed) return parsed;
  }
  const traceId = normalizeHex(input.trace_id, 32);
  const spanId = normalizeHex(input.span_id, 16);
  if (!traceId) return null;
  return spanId ? { trace_id: traceId, span_id: spanId } : { trace_id: traceId };
}

function normalizeHex(value: unknown, len: number): string | undefined {
  if (typeof value !== 'string') return undefined;
  const trimmed = value.trim().toLowerCase();
  if (!new RegExp(`^[\\da-f]{${len}}$`).test(trimmed) || /^0+$/.test(trimmed)) {
    return undefined;
  }
  return trimmed;
}

/** Lee el trace context activo en OTel (`@opentelemetry/api`). Cualquier span
 *  Faro o auto-instrumentado entra acá — son lo mismo desde v0.2.0. */
function currentOpenTelemetryTraceContext(): TraceContext | null {
  try {
    const span = trace.getActiveSpan();
    if (!span) return null;
    const sc = span.spanContext();
    if (!sc || !sc.traceId) return null;
    return normalizeTraceContext({
      trace_id: sc.traceId,
      span_id: sc.spanId,
    });
  } catch {
    return null;
  }
}

/** Adapter que expone la interfaz Span de Faro respaldada por un `@opentelemetry/api` Span.
 *  El BatchSpanProcessor de OTel se encarga del batching y la exportación a `/v1/traces`. */
class SpanImpl implements Span {
  public ended = false;
  constructor(private readonly otel: OtelSpan) {}

  spanContext(): { trace_id: string; span_id: string } {
    const sc = this.otel.spanContext();
    return { trace_id: sc.traceId, span_id: sc.spanId };
  }

  traceparent(): string {
    const sc = this.otel.spanContext();
    // bit 0 de traceFlags = sampled. Si el span es no-op (traceId=000...), igual
    // devolvemos un traceparent válido — quien lo reciba puede tomarlo o no.
    const flags = (sc.traceFlags & 1) ? '01' : '00';
    return `00-${sc.traceId}-${sc.spanId}-${flags}`;
  }

  setAttribute(key: string, value: unknown): void {
    if (this.ended) return;
    this.otel.setAttribute(key, stringifyAttr(value));
  }

  setAttributes(attrs: Record<string, unknown>): void {
    if (this.ended) return;
    for (const [k, v] of Object.entries(attrs)) {
      this.otel.setAttribute(k, stringifyAttr(v));
    }
  }

  addEvent(name: string, opts: SpanEventInput = {}): void {
    if (this.ended) return;
    const stringified: Record<string, string> = {};
    if (opts.attributes) {
      for (const [k, v] of Object.entries(opts.attributes)) stringified[k] = stringifyAttr(v);
    }
    this.otel.addEvent(name, stringified, opts.timestamp ?? new Date());
  }

  setStatus(code: SpanStatusCode, message?: string): void {
    if (this.ended) return;
    this.otel.setStatus({ code: OTEL_STATUS_CODE[code], ...(message ? { message } : {}) });
  }

  recordException(err: unknown): void {
    const e = toError(err);
    this.setStatus('ERROR', e.message);
    this.setAttribute('exception.type', e.name);
    this.setAttribute('exception.message', e.message);
    if (e.stack) this.setAttribute('exception.stacktrace', e.stack);
    // OTel también tiene recordException() — mantenemos los attrs custom por
    // paridad cross-SDK con Python/Go que escriben directamente esos attrs.
    try {
      this.otel.recordException(e);
    } catch {
      // best-effort
    }
  }

  end(opts: SpanEndOptions = {}): void {
    if (this.ended) return;
    this.ended = true;
    if (opts.attributes) this.setAttributes(opts.attributes);
    this.otel.end(opts.endTime ?? new Date());
  }
}

/** Convierte cualquier valor a string para uso en attributes (OTLP los quiere así). */
function stringifyAttr(v: unknown): string {
  if (typeof v === 'string') return v;
  if (v === null || v === undefined) return '';
  if (typeof v === 'number' || typeof v === 'boolean') return String(v);
  try {
    return JSON.stringify(v);
  } catch {
    return String(v);
  }
}

function resolveParentContext(parent: SpanOptions['parent']): OtelContext | undefined {
  if (parent === undefined) return undefined; // hereda current active context
  if (parent === null) return trace.deleteSpan(otelContext.active()); // fuerza root
  const ctx = normalizeTraceContext(parent);
  if (!ctx?.trace_id || !ctx?.span_id) return undefined;
  return trace.setSpanContext(otelContext.active(), {
    traceId: ctx.trace_id,
    spanId: ctx.span_id,
    traceFlags: 1, // SAMPLED
    isRemote: true,
  });
}

class FaroClient {
  private opts: Required<Omit<FaroOptions, 'attributes' | 'environment' | 'release' | 'diag' | 'beforeSend' | 'traceContext' | 'tracesEndpoint' | 'resourceAttributes' | 'disabledInstrumentations'>> &
    Pick<FaroOptions, 'attributes' | 'environment' | 'release' | 'diag' | 'beforeSend' | 'traceContext' | 'tracesEndpoint' | 'resourceAttributes' | 'disabledInstrumentations'>;
  private queue: Wire[] = [];
  private eventsQueue: ProductEventWire[] = [];
  private metricsQueue: MetricWire[] = [];
  private timer: ReturnType<typeof setInterval> | null = null;
  private featureFlagsTimer: ReturnType<typeof setInterval> | null = null;
  private closed = false;
  private installedHandlers: Array<() => void> = [];
  private scrubNeedles: string[];
  private scrubRegexes: RegExp[];
  private featureFlags = new Map<string, FeatureFlagWire>();
  private featureFlagsProject = '';
  private featureExposureSeen = new Set<string>();
  /** ID estable post-login; lo setea `identify` y lo pisa `alias`. Vacío hasta identify. */
  private distinctId = '';
  /** ID generado al boot del cliente para correlacionar eventos pre-login. */
  private anonymousId: string;
  /** Traits acumulados del último identify. Acompañan a cada evento en `user_properties`
   *  para que el dashboard pueda joinear sin un lookup adicional contra `product_users`. */
  private userProperties: Record<string, unknown> = {};

  constructor(opts: FaroOptions) {
    // Paridad cross-SDK: Python lanza ValueError y Go retorna error en el mismo
    // caso; aquí también un mensaje claro vence al críptico "Cannot read
    // properties of undefined" que saldría del trim/replace de más abajo.
    if (!opts || typeof opts.endpoint !== 'string' || opts.endpoint.length === 0) {
      throw new Error("faro.init: 'endpoint' es obligatorio (string no vacío)");
    }
    if (typeof opts.token !== 'string' || opts.token.length === 0) {
      throw new Error("faro.init: 'token' es obligatorio (string no vacío)");
    }
    if (typeof opts.service !== 'string' || opts.service.length === 0) {
      throw new Error("faro.init: 'service' es obligatorio (string no vacío)");
    }
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
      // Perfil de defaults: "server" (sdks/README.md → Perfiles de defaults).
      flushIntervalMs: opts.flushIntervalMs ?? 750,
      maxBatchSize: opts.maxBatchSize ?? 200,
      maxQueueSize: opts.maxQueueSize ?? 10_000,
      installGlobalHandlers: opts.installGlobalHandlers ?? true,
      featureFlagRefreshIntervalMs: opts.featureFlagRefreshIntervalMs ?? 30_000,
      enableTracing: opts.enableTracing ?? true,
      diag: opts.diag,
      scrubFields,
      scrubHeaders,
      scrubPatterns,
      beforeSend: opts.beforeSend,
      traceContext: opts.traceContext,
      tracesEndpoint: opts.tracesEndpoint,
      resourceAttributes: opts.resourceAttributes,
      disabledInstrumentations: opts.disabledInstrumentations,
    };
    this.anonymousId = randomId();

    // Bootstrap OTel tracing si no fue inicializado todavía via `--import …/instrument`.
    if (this.opts.enableTracing) {
      initTracing({
        endpoint: this.opts.endpoint,
        token: this.opts.token,
        service: this.opts.service,
        tracesEndpoint: this.opts.tracesEndpoint,
        environment: this.opts.environment,
        release: this.opts.release,
        resourceAttributes: this.opts.resourceAttributes,
        disabledInstrumentations: this.opts.disabledInstrumentations,
        diag: this.opts.diag,
      });
    }

    this.timer = setInterval(() => void this.flush(), this.opts.flushIntervalMs);
    // Permite a Node salir aunque el timer sea lo único que quede.
    if (typeof (this.timer as { unref?: () => void }).unref === 'function') {
      (this.timer as { unref: () => void }).unref();
    }
    this.featureFlagsTimer = setInterval(
      () => void this.refreshFeatureFlags(),
      this.opts.featureFlagRefreshIntervalMs,
    );
    if (typeof (this.featureFlagsTimer as { unref?: () => void }).unref === 'function') {
      (this.featureFlagsTimer as { unref: () => void }).unref();
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
    // Auto-correlación: si el caller no pasó trace/span_id, leemos el span activo
    // del context manager de OTel. Esto cubre tanto spans Faro como cualquier
    // span auto-instrumentado (express, http, pg, …).
    let traceId = entry.trace_id;
    let spanId = entry.span_id;
    if (!traceId) {
      const ctx = this.currentTraceContext();
      if (ctx?.trace_id) {
        traceId = ctx.trace_id;
        if (ctx.span_id) spanId = ctx.span_id;
      }
    }
    const wire: Wire = {
      level: entry.level ?? 'INFO',
      message: entry.message,
      timestamp: (entry.timestamp ?? new Date()).toISOString(),
      trace_id: traceId,
      span_id: spanId,
      attributes: attrs,
    };
    scrubWire(wire, this.scrubNeedles, this.scrubRegexes);
    const final = this.opts.beforeSend ? this.opts.beforeSend(wire) : wire;
    if (!final) return;
    if (this.queue.length >= this.opts.maxQueueSize) {
      this.diagLog('cola llena, evento descartado');
      return;
    }
    this.queue.push(final);
  }

  info(message: string, attributes?: Record<string, unknown>): void {
    this.log({ level: 'INFO', message, attributes });
  }
  warn(message: string, attributes?: Record<string, unknown>): void {
    this.log({ level: 'WARN', message, attributes });
  }
  /** Alias de `warn` — por simetría con `logging.WARNING` y el SDK de Python. */
  warning(message: string, attributes?: Record<string, unknown>): void {
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

  // ---------- Product events API (track / identify / alias) ----------
  // No incluye page() / screen() porque este SDK está pensado para Node.js
  // server-side — no hay routing de cliente que reportar. Para web ver
  // @iaportafolio/nextjs (client) y para mobile @iaportafolio/expo / kotlin / flutter.

  /** Envía un evento custom. Equivalente conceptual a `analytics.track()` de Segment/PostHog. */
  track(eventName: string, properties: Record<string, unknown> = {}): void {
    this.enqueueEvent({
      type: 'track',
      name: eventName,
      properties,
    });
  }

  /** Identifica al usuario actual. Setea `distinct_id` para los eventos siguientes
   *  y emite un `$identify` con los traits para que el backend actualice product_users. */
  identify(userId: string, traits: Record<string, unknown> = {}): void {
    if (!userId) return;
    this.distinctId = userId;
    this.userProperties = { ...this.userProperties, ...traits };
    this.enqueueEvent({
      type: 'identify',
      name: '$identify',
      properties: {},
      userPropertiesOverride: traits,
    });
  }

  /** Fusiona una sesión pre-login (anonymousId) con un usuario post-login.
   *  El backend usa esto para que los eventos pre-login se atribuyan al user. */
  alias(prevId: string, newId: string): void {
    if (!prevId || !newId) return;
    this.distinctId = newId;
    this.enqueueEvent({
      type: 'alias',
      name: '$alias',
      properties: {},
      anonymousIdOverride: prevId,
    });
  }

  async refreshFeatureFlags(): Promise<void> {
    if (this.closed) return;
    try {
      const res = await fetch(`${this.opts.endpoint}/api/v1/ingest/feature-flags`, {
        method: 'GET',
        headers: { 'Authorization': `Bearer ${this.opts.token}` },
      });
      if (!res.ok) {
        const txt = await res.text().catch(() => '');
        this.diagLog(`feature flags HTTP ${res.status}: ${txt.slice(0, 200)}`);
        return;
      }
      const body = (await res.json()) as FeatureFlagsResponse;
      if (!Array.isArray(body.flags)) {
        this.diagLog('feature flags response inválida');
        return;
      }
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
    } catch (e) {
      this.diagLog('falló el refresh de feature flags', e);
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

  // ---------- Tracing (respaldado por OTel) ----------

  /** Crea un span nuevo (sin entrar en su contexto). Llamar `end()` para encolar.
   *  Para auto-activarlo + cerrarlo, ver `withSpan`. */
  startSpan(name: string, opts: SpanOptions = {}): Span {
    const tracer = getTracer();
    const otelAttrs: Record<string, string> = {};
    if (opts.attributes) {
      for (const [k, v] of Object.entries(opts.attributes)) otelAttrs[k] = stringifyAttr(v);
    }
    const parentCtx = resolveParentContext(opts.parent);
    const otelSpan = tracer.startSpan(
      name,
      {
        kind: OTEL_SPAN_KIND[opts.kind ?? 'INTERNAL'],
        attributes: otelAttrs,
        ...(opts.startTime ? { startTime: opts.startTime } : {}),
      },
      parentCtx,
    );
    return new SpanImpl(otelSpan);
  }

  /** Ejecuta `fn` con `span` activo en el ContextManager. El span se cierra
   *  automáticamente al terminar (con `setStatus('ERROR')` y `recordException`
   *  si `fn` lanza). Pensado para envolver handlers o bloques async. */
  withSpan<T>(name: string, fn: (span: Span) => Promise<T> | T, opts: SpanOptions = {}): Promise<T> {
    const tracer = getTracer();
    const otelAttrs: Record<string, string> = {};
    if (opts.attributes) {
      for (const [k, v] of Object.entries(opts.attributes)) otelAttrs[k] = stringifyAttr(v);
    }
    const spanOpts = {
      kind: OTEL_SPAN_KIND[opts.kind ?? 'INTERNAL'],
      attributes: otelAttrs,
      ...(opts.startTime ? { startTime: opts.startTime } : {}),
    };
    const parentCtx = resolveParentContext(opts.parent) ?? otelContext.active();

    return new Promise<T>((resolve, reject) => {
      tracer.startActiveSpan(name, spanOpts, parentCtx, async (otelSpan) => {
        const span = new SpanImpl(otelSpan);
        try {
          const r = await fn(span);
          if (!span.ended) span.end();
          resolve(r);
        } catch (e) {
          span.recordException(e);
          if (!span.ended) span.end();
          reject(e);
        }
      });
    });
  }

  /** Devuelve el span activo del ContextManager o null. Ojo: el wrapper es
   *  efímero; si lo guardás y el span ya terminó, los setters son no-op. */
  activeSpan(): Span | null {
    const otelSpan = trace.getActiveSpan();
    if (!otelSpan) return null;
    return new SpanImpl(otelSpan);
  }

  // ---------- Métricas ----------

  /** Counter monotónico. Cada `add(value, attrs)` añade un data point al batch. */
  counter(name: string, opts: MetricInstrumentOptions = {}): Counter {
    return {
      add: (value, attributes) => this.recordMetric(name, 'counter', value, opts.unit, attributes),
    };
  }

  /** Counter no monotónico (delta puede ser negativo). */
  upDownCounter(name: string, opts: MetricInstrumentOptions = {}): UpDownCounter {
    return {
      add: (value, attributes) => this.recordMetric(name, 'sum', value, opts.unit, attributes),
    };
  }

  /** Gauge: cada `set(value)` reemplaza. */
  gauge(name: string, opts: MetricInstrumentOptions = {}): Gauge {
    return {
      set: (value, attributes) => this.recordMetric(name, 'gauge', value, opts.unit, attributes),
    };
  }

  /** Histograma: cada `record(value)` envía un data point individual (count=1,
   *  sum=min=max=value). El backend agrega; el SDK no — para agregación
   *  cliente-side con buckets explícitos, usar el SDK OTel. */
  histogram(name: string, opts: MetricInstrumentOptions = {}): Histogram {
    return {
      record: (value, attributes) =>
        this.recordMetric(name, 'histogram', value, opts.unit, attributes),
    };
  }

  private recordMetric(
    name: string,
    kind: MetricKind,
    value: number,
    unit: string | undefined,
    attributes: MetricAttrs | undefined,
  ): void {
    if (this.closed) return;
    if (!name || typeof name !== 'string') return;
    if (!Number.isFinite(value)) return;
    if (this.metricsQueue.length >= this.opts.maxQueueSize) return;

    const attrs: Record<string, string> = {};
    if (this.opts.attributes) {
      for (const [k, v] of Object.entries(this.opts.attributes)) attrs[k] = String(v);
    }
    if (this.opts.environment) attrs['deployment.environment'] = this.opts.environment;
    if (this.opts.release) attrs['service.version'] = this.opts.release;
    if (attributes) {
      for (const [k, v] of Object.entries(attributes)) attrs[k] = String(v);
    }

    const wire: MetricWire = {
      name,
      kind,
      timestamp: new Date().toISOString(),
      service: this.opts.service,
      attributes: attrs,
    };
    if (unit) wire.unit = unit;
    if (kind === 'histogram') {
      // SDK no agrega: un data point por record(), count=1 sum=min=max=value.
      wire.count = 1;
      wire.sum = value;
      wire.min = value;
      wire.max = value;
    } else {
      wire.value = value;
    }
    this.metricsQueue.push(wire);
  }

  async flush(): Promise<void> {
    // Flusheamos logs + events + métricas propios y los pending spans del
    // BatchSpanProcessor de OTel en paralelo. Spans van por una pipeline
    // aparte (OTLP/HTTP).
    await Promise.all([
      this.flushLogs(),
      this.flushEvents(),
      this.flushMetrics(),
      flushTracing(2000),
    ]);
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
      if (!res.ok) {
        const txt = await res.text().catch(() => '');
        this.diagLog(`ingest HTTP ${res.status}: ${txt.slice(0, 200)}`);
        // 5xx → re-encolar para reintentar; 4xx → descartar (probablemente
        // batch malformado / auth inválida — re-encolar acumularía basura).
        if (res.status >= 500) this.queue.unshift(...batch);
      }
    } catch (e) {
      // Re-encola ante fallos transitorios de red para no perder datos.
      this.queue.unshift(...batch);
      this.diagLog('falló el flush', e);
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
      if (!res.ok) {
        const txt = await res.text().catch(() => '');
        this.diagLog(`ingest events HTTP ${res.status}: ${txt.slice(0, 200)}`);
        if (res.status >= 500) this.eventsQueue.unshift(...batch);
      }
    } catch (e) {
      this.eventsQueue.unshift(...batch);
      this.diagLog('falló el flush de events', e);
    }
  }

  private async flushMetrics(): Promise<void> {
    if (this.metricsQueue.length === 0) return;
    const batch = this.metricsQueue.splice(0, this.opts.maxBatchSize);
    try {
      const res = await fetch(`${this.opts.endpoint}/api/v1/ingest/metrics`, {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${this.opts.token}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ service: this.opts.service, metrics: batch }),
      });
      if (!res.ok) {
        const txt = await res.text().catch(() => '');
        this.diagLog(`ingest metrics HTTP ${res.status}: ${txt.slice(0, 200)}`);
        if (res.status >= 500) this.metricsQueue.unshift(...batch);
      }
    } catch (e) {
      this.metricsQueue.unshift(...batch);
      this.diagLog('falló el flush de metrics', e);
    }
  }

  private enqueueEvent(input: {
    type: ProductEventType;
    name: string;
    properties: Record<string, unknown>;
    /** Sobrescribe `user_properties` (lo usa `identify` para emitir solo los traits del request). */
    userPropertiesOverride?: Record<string, unknown>;
    /** Sobrescribe `anonymous_id` (lo usa `alias` para llevar el id PREVIO). */
    anonymousIdOverride?: string;
    /** Sobrescribe `distinct_id` (lo usa feature exposure con context.distinct_id). */
    distinctIdOverride?: string;
  }): void {
    if (this.closed) return;
    if (this.eventsQueue.length >= this.opts.maxQueueSize) {
      this.diagLog('cola de events llena, evento descartado');
      return;
    }
    const ctx: Record<string, unknown> = {};
    if (this.opts.environment) ctx.environment = this.opts.environment;
    if (this.opts.release) ctx.release = this.opts.release;
    if (this.opts.attributes) Object.assign(ctx, this.opts.attributes);
    const event: ProductEventWire = {
      type: input.type,
      name: input.name,
      timestamp: new Date().toISOString(),
      distinct_id: input.distinctIdOverride ?? (this.distinctId || this.anonymousId),
      anonymous_id: input.anonymousIdOverride ?? this.anonymousId,
      session_id: '',
      properties: input.properties,
      user_properties: input.userPropertiesOverride ?? this.userProperties,
      context: ctx,
      source: 'backend',
    };
    const tc = this.currentTraceContext();
    if (tc?.trace_id) event.trace_id = tc.trace_id;
    if (tc?.span_id) event.span_id = tc.span_id;
    this.eventsQueue.push(event);
  }

  private currentTraceContext(): TraceContext | null {
    const explicit = normalizeTraceContext(this.opts.traceContext?.());
    if (explicit) return explicit;
    return currentOpenTelemetryTraceContext();
  }

  /**
   * Drena las colas y cierra. Pensado para envolver en hooks `SIGTERM`/`SIGINT`
   * del usuario: `process.on('SIGTERM', () => faro.close().finally(() => process.exit(0)))`.
   * El `timeoutMs` acota el peor caso (red caída + cola llena).
   * También apaga el NodeSDK de OTel para drenar los spans pending.
   */
  async close(timeoutMs = 5000): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    if (this.featureFlagsTimer) clearInterval(this.featureFlagsTimer);
    this.featureFlagsTimer = null;
    for (const off of this.installedHandlers) off();
    this.installedHandlers = [];
    // Drena nuestras colas con cota dura.
    const deadline = Date.now() + timeoutMs;
    while (
      (this.queue.length > 0 || this.eventsQueue.length > 0) &&
      Date.now() < deadline
    ) {
      const before = this.queue.length + this.eventsQueue.length;
      await Promise.all([this.flushLogs(), this.flushEvents()]);
      const after = this.queue.length + this.eventsQueue.length;
      if (after >= before) break; // probablemente la red esté caída; rendirse
    }
    // Cierra OTel — drena el BatchSpanProcessor con su propio timeout interno.
    const remaining = Math.max(500, deadline - Date.now());
    await shutdownTracing(remaining);
  }

  private installHandlers(): void {
    if (typeof process === 'undefined') return;
    const onUncaught = (err: Error): void => {
      this.captureException(err, { message: '[uncaughtException] ' + (err?.message ?? '') });
      // Drena en best-effort; NO tragar — dejar que Node imprima y salga.
      void this.flush();
    };
    const onRejection = (reason: unknown): void => {
      this.captureException(reason, { message: '[unhandledRejection]' });
      void this.flush();
    };
    const onExit = (): void => {
      // No podemos hacer await aquí. Lanza un último fetch lo más síncrono posible.
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

/** Genera un anonymous_id pseudo-aleatorio para correlacionar eventos pre-login.
 *  Suficiente para evitar colisiones en un proceso Node; no es criptográfico. */
function randomId(): string {
  const a = Math.random().toString(36).slice(2, 12);
  const b = Date.now().toString(36);
  return `anon_${b}_${a}`;
}

function clampRollout(value: unknown): number {
  const n = typeof value === 'number' && Number.isFinite(value) ? Math.trunc(value) : 0;
  return Math.max(0, Math.min(100, n));
}

function normalizeConditions(value: FeatureFlagWire['conditions']): FeatureFlagWire['conditions'] {
  return value && typeof value === 'object' ? value : {};
}

function matchesFeatureConditions(flag: FeatureFlagWire, context: FeatureFlagContext): boolean {
  const required = flag.conditions?.properties;
  if (!required || typeof required !== 'object') return true;
  const props = context.properties ?? {};
  for (const [key, expected] of Object.entries(required)) {
    if (props[key] !== expected) return false;
  }
  return true;
}

function stickyBucket(input: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0) % 100;
}

let singleton: FaroClient | null = null;

export function init(opts: FaroOptions): FaroClient {
  if (singleton) singleton.close().catch(() => undefined);
  singleton = new FaroClient(opts);
  return singleton;
}

export function getClient(): FaroClient {
  if (!singleton) throw new Error('faro: hay que llamar a init() antes de usarlo');
  return singleton;
}

// Helpers a nivel de módulo para que los usuarios no tengan que ir pasando el cliente.
export function log(entry: LogEntry): void { getClient().log(entry); }
export function info(msg: string, attrs?: Record<string, unknown>): void { getClient().info(msg, attrs); }
export function warn(msg: string, attrs?: Record<string, unknown>): void { getClient().warn(msg, attrs); }
export function warning(msg: string, attrs?: Record<string, unknown>): void { getClient().warning(msg, attrs); }
export function error(msg: string, attrs?: Record<string, unknown>): void { getClient().error(msg, attrs); }
export function captureException(err: unknown, ctx?: { tags?: Record<string, string>; message?: string }): void {
  getClient().captureException(err, ctx);
}
export function track(event: string, properties?: Record<string, unknown>): void {
  getClient().track(event, properties);
}
export function identify(userId: string, traits?: Record<string, unknown>): void {
  getClient().identify(userId, traits);
}
export function alias(prevId: string, newId: string): void {
  getClient().alias(prevId, newId);
}
export function refreshFeatureFlags(): Promise<void> { return getClient().refreshFeatureFlags(); }
export function isFeatureEnabled(key: string, context?: FeatureFlagContext): boolean {
  return getClient().isFeatureEnabled(key, context);
}
export function startSpan(name: string, opts?: SpanOptions): Span {
  return getClient().startSpan(name, opts);
}
export function withSpan<T>(name: string, fn: (span: Span) => Promise<T> | T, opts?: SpanOptions): Promise<T> {
  return getClient().withSpan(name, fn, opts);
}
export function activeSpan(): Span | null { return getClient().activeSpan(); }
export function counter(name: string, opts?: MetricInstrumentOptions): Counter {
  return getClient().counter(name, opts);
}
export function upDownCounter(name: string, opts?: MetricInstrumentOptions): UpDownCounter {
  return getClient().upDownCounter(name, opts);
}
export function gauge(name: string, opts?: MetricInstrumentOptions): Gauge {
  return getClient().gauge(name, opts);
}
export function histogram(name: string, opts?: MetricInstrumentOptions): Histogram {
  return getClient().histogram(name, opts);
}
export function flush(): Promise<void> { return getClient().flush(); }
export function close(timeoutMs?: number): Promise<void> { return getClient().close(timeoutMs); }
