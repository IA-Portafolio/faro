/**
 * OTLP tracing setup para el SDK de Node.
 *
 * Inicializa `NodeTracerProvider` con un BatchSpanProcessor + OTLP/HTTP/JSON
 * exporter apuntando al endpoint de Faro. Habilita auto-instrumentación de las
 * librerías comunes (http, fetch, express, fastify, koa, pg, mysql, mongodb,
 * redis, ioredis, grpc, kafka, …) para que el Service Map y la pestaña Trazas
 * se llenen sin instrumentación manual.
 *
 * Diseño:
 *   - Singleton: una sola inicialización por proceso, las siguientes son no-op.
 *   - El provider se guarda en la closure del módulo para poder llamar a
 *     `forceFlush()` desde `flushTracing()` mid-lifetime — esto es necesario
 *     para `faro.flush()` y para los tests, que no pueden esperar al scheduled
 *     export del BatchSpanProcessor (5s por default).
 *   - `registerInstrumentations` se llama UNA vez por proceso; la segunda
 *     re-inicialización del provider reutiliza las instrumentaciones ya
 *     registradas (OTel JS las re-engancha al provider current).
 */

import { trace, type Tracer } from '@opentelemetry/api';
import { NodeTracerProvider, BatchSpanProcessor } from '@opentelemetry/sdk-trace-node';
import { OTLPTraceExporter } from '@opentelemetry/exporter-trace-otlp-http';
import { getNodeAutoInstrumentations } from '@opentelemetry/auto-instrumentations-node';
import { registerInstrumentations } from '@opentelemetry/instrumentation';
import { resourceFromAttributes } from '@opentelemetry/resources';
import {
  ATTR_SERVICE_NAME,
  ATTR_SERVICE_VERSION,
} from '@opentelemetry/semantic-conventions';

/** Nombre del tracer expuesto por getTracer(). Aparece como `scope.name` en los spans. */
const TRACER_NAME = '@iaportafolio/node';

export interface TracingOptions {
  /** URL base de Faro (sin `/v1/traces`). p. ej. https://faro.iaportafolio.com */
  endpoint: string;
  /** Bearer token de ingesta del proyecto. */
  token: string;
  /** service.name reportado en cada span. */
  service: string;
  /** Override del endpoint completo de traces. Default: `${endpoint}/v1/traces`. */
  tracesEndpoint?: string;
  /** Mapeado a `deployment.environment` (y `.name` por compat con OTel 1.27+). */
  environment?: string;
  /** Mapeado a `service.version`. */
  release?: string;
  /** Atributos extra a poner en el Resource del SDK. */
  resourceAttributes?: Record<string, string>;
  /**
   * Nombres exactos de los paquetes de instrumentación a desactivar. Se suman
   * a los noisy defaults del SDK (`fs`, `dns`, `net`).
   */
  disabledInstrumentations?: string[];
  /** Logger para advertencias internas. Default: console.warn con prefijo. */
  diag?: (msg: string, err?: unknown) => void;
}

let provider: NodeTracerProvider | null = null;
let instrumentationsRegistered = false;
let cachedTracer: Tracer | null = null;

/**
 * Inicializa el provider OTel apuntando a Faro. Idempotente: la segunda
 * llamada es no-op (devuelve `false`).
 *
 * IMPORTANTE: en Node, la auto-instrumentación debe registrarse ANTES de que
 * se importen las librerías a instrumentar (http, express, …). En la práctica
 * eso significa:
 *   - Recomendado: `node --import @iaportafolio/node/instrument server.js`
 *     (lee FARO_ENDPOINT / FARO_INGEST_TOKEN / OTEL_SERVICE_NAME del entorno).
 *   - Inline: llamar a `faro.init(...)` o `initTracing(...)` en la primera
 *     línea del entrypoint, antes de cualquier otro import.
 */
export function initTracing(opts: TracingOptions): boolean {
  if (provider) return false;
  if (!opts.endpoint || !opts.token || !opts.service) return false;

  const diag = opts.diag ?? ((msg, err) => console.warn(`[faro/tracing] ${msg}`, err ?? ''));

  const base = opts.endpoint.replace(/\/$/, '');
  const url = opts.tracesEndpoint ?? `${base}/v1/traces`;

  const exporter = new OTLPTraceExporter({
    url,
    headers: { Authorization: `Bearer ${opts.token}` },
  });

  const attrs: Record<string, string> = {
    [ATTR_SERVICE_NAME]: opts.service,
  };
  if (opts.release) attrs[ATTR_SERVICE_VERSION] = opts.release;
  // OTel 1.27+: el atributo recomendado es `deployment.environment.name`. El
  // backend de Faro indexa por `deployment.environment` (sin `.name`) por
  // compat con SDKs viejos — emitimos ambos para que ambos caminos funcionen.
  if (opts.environment) {
    attrs['deployment.environment.name'] = opts.environment;
    attrs['deployment.environment'] = opts.environment;
  }
  if (opts.resourceAttributes) {
    for (const [k, v] of Object.entries(opts.resourceAttributes)) attrs[k] = String(v);
  }

  try {
    provider = new NodeTracerProvider({
      resource: resourceFromAttributes(attrs),
      spanProcessors: [new BatchSpanProcessor(exporter)],
    });
    provider.register();
  } catch (e) {
    diag('initTracing falló creando el provider', e);
    provider = null;
    return false;
  }

  // Defaults ruidosos. El user puede sumar más vía `disabledInstrumentations`.
  const NOISY_DEFAULTS = [
    '@opentelemetry/instrumentation-fs',
    '@opentelemetry/instrumentation-dns',
    '@opentelemetry/instrumentation-net',
  ];
  const disabled = new Set([...NOISY_DEFAULTS, ...(opts.disabledInstrumentations ?? [])]);
  const instrumentationsConfig: Record<string, { enabled?: boolean }> = {};
  for (const name of disabled) instrumentationsConfig[name] = { enabled: false };

  if (!instrumentationsRegistered) {
    try {
      registerInstrumentations({
        instrumentations: [getNodeAutoInstrumentations(instrumentationsConfig)],
      });
      instrumentationsRegistered = true;
    } catch (e) {
      diag('initTracing falló registrando instrumentaciones', e);
      // No re-throwing — el provider sí quedó ok y los spans manuales funcionan.
    }
  }

  return true;
}

/**
 * Drena los spans pending del BatchSpanProcessor sin apagar el SDK.
 * Lo usa `FaroClient.flush()` y los tests. Si OTel no fue inicializado, no-op.
 */
export async function flushTracing(timeoutMs = 5000): Promise<void> {
  const current = provider;
  if (!current) return;
  try {
    await Promise.race([
      current.forceFlush(),
      new Promise<void>((resolve) => {
        const t = setTimeout(resolve, timeoutMs);
        if (typeof (t as { unref?: () => void }).unref === 'function') {
          (t as { unref: () => void }).unref();
        }
      }),
    ]);
  } catch {
    // best-effort
  }
}

/**
 * Drena pending spans y apaga el SDK. Pensado para SIGTERM/SIGINT del usuario,
 * o como parte de `faro.close()`. Timeout duro de seguridad — si la red cae
 * mientras se intenta drenar, no nos quedamos colgados.
 */
export async function shutdownTracing(timeoutMs = 5000): Promise<void> {
  const current = provider;
  if (!current) return;
  provider = null;
  cachedTracer = null;
  try {
    await Promise.race([
      current.shutdown(),
      new Promise<void>((resolve) => {
        const t = setTimeout(resolve, timeoutMs);
        if (typeof (t as { unref?: () => void }).unref === 'function') {
          (t as { unref: () => void }).unref();
        }
      }),
    ]);
  } catch {
    // best-effort
  }
  // CRÍTICO: @opentelemetry/api refusa re-registrar el global tracer provider
  // (`setGlobalTracerProvider` chequea si ya hay uno y devuelve false). Sin
  // llamar a `trace.disable()` aquí, una segunda `initTracing()` con un endpoint
  // distinto crearía el provider pero `.register()` sería no-op y los spans
  // seguirían intentando exportar al destino viejo. Esto rompe los tests que
  // hacen init/close repetidos, y también un eventual hot-reload en dev.
  trace.disable();
}

/**
 * Devuelve un tracer con el nombre del SDK. Si OTel no está inicializado,
 * devuelve un tracer no-op del provider global — así el código que usa spans
 * manuales no rompe cuando `enableTracing` está en false o cuando el SDK
 * todavía no se llamó a `init()`.
 */
export function getTracer(): Tracer {
  if (!cachedTracer) cachedTracer = trace.getTracer(TRACER_NAME);
  return cachedTracer;
}

/** Solo para testing — resetea los singletons para que `initTracing` pueda volver a correr. */
export function _resetTracingForTests(): void {
  provider = null;
  instrumentationsRegistered = false;
  cachedTracer = null;
}
