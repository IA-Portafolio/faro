/**
 * Catálogo de referencia de los SDKs de Faro y de **todos** sus métodos
 * públicos. Es la fuente única que alimenta la página `/docs`.
 *
 * Cada entrada se extrajo del código fuente real del SDK (`sdks/<lang>`),
 * no del README, para que la firma y la disponibilidad de cada método sean
 * exactas. Si añades o renombras un método en un SDK, actualízalo aquí.
 *
 * Convención de firmas: se documentan tal cual las expone el lenguaje
 * (camelCase en TS/Kotlin/Go, snake_case en Python). El `?` marca argumentos
 * opcionales en los lenguajes que lo soportan en la firma.
 */

/** Perfil de defaults declarado por el SDK (ver `sdks/README.md`). */
export type SdkProfile = 'server' | 'mobile' | 'browser';

/** Capacidad de alto nivel que el SDK cubre — se renderiza como chip. */
export type SdkCapability =
  | 'Logs'
  | 'Errores'
  | 'Product analytics'
  | 'Tracing'
  | 'Métricas'
  | 'Feature flags'
  | 'RUM'
  | 'Session replay';

export type SdkMethod = {
  /** Firma tal como se llama, p. ej. `info(msg, attrs?)`. */
  signature: string;
  /** Qué hace, en una frase. */
  summary: string;
  /** Tipo/forma de retorno relevante (opcional). */
  returns?: string;
};

export type SdkMethodGroup = {
  title: string;
  /** Nota corta sobre el grupo (subimport, plataforma, etc.). */
  note?: string;
  methods: SdkMethod[];
};

export type SdkDoc = {
  id: string;
  /** Nombre visible, p. ej. "Node.js". */
  name: string;
  /** Lenguaje / runtime. */
  language: string;
  /** Nombre del paquete publicado. */
  pkg: string;
  /** Comando de instalación. */
  install: string;
  profile: SdkProfile;
  /** Una línea describiendo para qué sirve el SDK. */
  blurb: string;
  capabilities: SdkCapability[];
  /** Snippet de inicialización mínimo (copy-paste). */
  initExample: string;
  /** Lenguaje para el resaltado del bloque (clase CSS `lang-*`, informativo). */
  lang: string;
  groups: SdkMethodGroup[];
};

/** Defaults de flush / batch / queue por perfil (orden de magnitud). */
export const profileDefaults: Record<SdkProfile, { flushMs: number; batch: number; queue: number; label: string }> = {
  server: { flushMs: 750, batch: 200, queue: 10_000, label: 'Servidor' },
  mobile: { flushMs: 1500, batch: 100, queue: 5_000, label: 'Móvil' },
  browser: { flushMs: 2000, batch: 100, queue: 2_000, label: 'Navegador' }
};

/** Opciones de `init()` compartidas por todos los SDKs (sin lock-in OTel). */
export const commonOptions: { name: string; type: string; default: string; desc: string }[] = [
  { name: 'endpoint', type: 'string', default: '—', desc: 'URL base de tu instancia de Faro.' },
  { name: 'token', type: 'string', default: '—', desc: 'Token de ingesta del proyecto (visible en Configuración → Proyectos).' },
  { name: 'service', type: 'string', default: '—', desc: '`service.name` de OTel adjuntado a cada evento y span.' },
  { name: 'environment', type: 'string', default: '—', desc: 'dev / staging / production → `deployment.environment`.' },
  { name: 'release', type: 'string', default: '—', desc: 'Release / commit / tag → `service.version`.' },
  { name: 'attributes', type: 'map', default: '{}', desc: 'Atributos por defecto mezclados en cada evento.' },
  { name: 'flushIntervalMs', type: 'number', default: 'según perfil', desc: 'Cadencia de flush de logs y eventos.' },
  { name: 'maxBatchSize', type: 'number', default: 'según perfil', desc: 'Máximo de eventos por lote HTTP.' },
  { name: 'maxQueueSize', type: 'number', default: 'según perfil', desc: 'Descarta eventos nuevos al superar este tamaño de cola.' },
  { name: 'scrubFields', type: 'string[]', default: 'password, token, secret…', desc: 'Claves cuyo valor se redacta antes de salir del cliente.' },
  { name: 'scrubPatterns', type: 'preset[]', default: "['jwt','api-key']", desc: 'Presets de regex (email / jwt / api-key / credit-card) aplicados a valores y mensaje.' },
  { name: 'beforeSend', type: 'fn', default: '—', desc: 'Hook post-scrub para muestrear / transformar / descartar (devolver null descarta).' }
];

/** Contrato de severidades común (texto → número OTel). */
export const severities: { text: string; num: number; cls: string }[] = [
  { text: 'TRACE', num: 1, cls: 'trace' },
  { text: 'DEBUG', num: 5, cls: 'debug' },
  { text: 'INFO', num: 9, cls: 'info' },
  { text: 'WARN', num: 13, cls: 'warn' },
  { text: 'ERROR', num: 17, cls: 'error' },
  { text: 'FATAL', num: 21, cls: 'fatal' }
];

/** Matriz de disponibilidad de la API de producto (Segment/PostHog-style). */
export const productMatrix: { sdk: string; track: boolean; identify: boolean; page: boolean; screen: boolean; alias: boolean }[] = [
  { sdk: 'Node.js', track: true, identify: true, page: false, screen: false, alias: true },
  { sdk: 'Python', track: true, identify: true, page: false, screen: false, alias: true },
  { sdk: 'Go', track: true, identify: true, page: false, screen: false, alias: true },
  { sdk: 'Next.js (server)', track: true, identify: true, page: false, screen: false, alias: true },
  { sdk: 'Next.js (client)', track: true, identify: true, page: true, screen: false, alias: true },
  { sdk: 'Expo / RN', track: true, identify: true, page: false, screen: true, alias: true },
  { sdk: 'Flutter', track: true, identify: true, page: true, screen: true, alias: true },
  { sdk: 'Kotlin', track: true, identify: true, page: false, screen: true, alias: true }
];

export const sdks: SdkDoc[] = [
  {
    id: 'node',
    name: 'Node.js',
    language: 'TypeScript / JavaScript',
    pkg: '@iaportafolio/node',
    install: 'npm install @iaportafolio/node',
    profile: 'server',
    blurb: 'SDK de servidor con logs, errores, product analytics, tracing OTel y métricas.',
    capabilities: ['Logs', 'Errores', 'Product analytics', 'Tracing', 'Métricas', 'Feature flags'],
    lang: 'ts',
    initExample: `import * as faro from '@iaportafolio/node';

faro.init({
  endpoint: 'https://faro.iaportafolio.com',
  token: process.env.FARO_TOKEN!,
  service: 'mi-servicio',
  environment: 'production',
});

faro.info('arranque ok', { port: 8080 });`,
    groups: [
      {
        title: 'Inicialización y ciclo de vida',
        methods: [
          { signature: 'init(options)', summary: 'Configura el SDK e instala los handlers globales. Llamar una vez al arranque.', returns: 'FaroClient' },
          { signature: 'getClient()', summary: 'Devuelve la instancia singleton ya inicializada.', returns: 'FaroClient' },
          { signature: 'flush()', summary: 'Espera a que las colas de logs y eventos pendientes se envíen.', returns: 'Promise<void>' },
          { signature: 'close(timeoutMs?)', summary: 'Flush final acotado + desinstala los handlers. Úsalo en SIGTERM/SIGINT.', returns: 'Promise<void>' }
        ]
      },
      {
        title: 'Logging',
        methods: [
          { signature: 'log(entry)', summary: 'Envía un log estructurado { level, message, attributes }.' },
          { signature: 'info(msg, attrs?)', summary: 'Atajo para log con nivel INFO.' },
          { signature: 'warn(msg, attrs?)', summary: 'Atajo para nivel WARN.' },
          { signature: 'warning(msg, attrs?)', summary: 'Alias de warn() (paridad con loggers JVM/Python).' },
          { signature: 'error(msg, attrs?)', summary: 'Atajo para nivel ERROR.' }
        ]
      },
      {
        title: 'Errores',
        methods: [
          { signature: 'captureException(err, ctx?)', summary: 'Captura una excepción con stack trace y tags; el agrupador la convierte en issue.' }
        ]
      },
      {
        title: 'Product analytics',
        methods: [
          { signature: 'track(event, properties?)', summary: 'Emite un product event personalizado.' },
          { signature: 'identify(userId, traits?)', summary: 'Asocia los eventos futuros a un usuario y emite $identify.' },
          { signature: 'alias(prevId, newId)', summary: 'Fusiona la sesión anónima pre-login con el usuario post-login.' }
        ]
      },
      {
        title: 'Feature flags',
        methods: [
          { signature: 'isFeatureEnabled(key, context?)', summary: 'Evalúa un flag contra el snapshot local (sin red).', returns: 'boolean' },
          { signature: 'refreshFeatureFlags()', summary: 'Fuerza un refresh del snapshot de flags desde el backend.', returns: 'Promise<void>' }
        ]
      },
      {
        title: 'Tracing (OpenTelemetry)',
        methods: [
          { signature: 'startSpan(name, opts?)', summary: 'Abre un span manual.', returns: 'Span' },
          { signature: 'withSpan(name, fn, opts?)', summary: 'Ejecuta fn dentro de un span y lo cierra automáticamente.', returns: 'Promise<T>' },
          { signature: 'activeSpan()', summary: 'Devuelve el span activo en el contexto actual.', returns: 'Span | null' },
          { signature: 'initTracing(opts)', summary: 'Inicializa OTel + auto-instrumentación manualmente.', returns: 'boolean' },
          { signature: 'flushTracing(timeoutMs?)', summary: 'Vacía los spans pendientes del batch processor.', returns: 'Promise<void>' },
          { signature: 'shutdownTracing(timeoutMs?)', summary: 'Cierra el tracer provider de OTel.', returns: 'Promise<void>' },
          { signature: 'getTracer()', summary: 'Devuelve el Tracer de OTel para instrumentación avanzada.', returns: 'Tracer' },
          { signature: 'parseTraceparent(header)', summary: 'Parsea un header W3C traceparent a TraceContext.', returns: 'TraceContext | null' }
        ]
      },
      {
        title: 'Métricas',
        methods: [
          { signature: 'counter(name, opts?)', summary: 'Crea un contador monotónico (add).', returns: 'Counter' },
          { signature: 'upDownCounter(name, opts?)', summary: 'Crea un contador que sube y baja.', returns: 'UpDownCounter' },
          { signature: 'gauge(name, opts?)', summary: 'Crea un gauge de valor puntual (set).', returns: 'Gauge' },
          { signature: 'histogram(name, opts?)', summary: 'Crea un histograma de distribución (record).', returns: 'Histogram' }
        ]
      },
      {
        title: 'Integraciones',
        note: 'Subimports opcionales para frameworks y loggers.',
        methods: [
          { signature: 'expressTracer(opts?)', summary: 'Middleware de Express que abre un span por request.' },
          { signature: 'FaroTransport', summary: 'Transport de Winston que reenvía los logs a Faro (@iaportafolio/node/winston).' },
          { signature: '@iaportafolio/node/pino', summary: 'Transport de Pino para enviar logs a Faro.' },
          { signature: '--import @iaportafolio/node/instrument', summary: 'Auto-instrumentación OTel sin tocar código (flag de arranque de Node).' }
        ]
      }
    ]
  },
  {
    id: 'nextjs',
    name: 'Next.js',
    language: 'TypeScript · server + client',
    pkg: '@iaportafolio/nextjs',
    install: 'npm install @iaportafolio/nextjs @iaportafolio/node',
    profile: 'browser',
    blurb: 'RUM en el navegador (Web Vitals, breadcrumbs, replay) + telemetría de servidor que hereda el SDK de Node.',
    capabilities: ['RUM', 'Logs', 'Errores', 'Product analytics', 'Session replay', 'Feature flags'],
    lang: 'ts',
    initExample: `// instrumentation.ts (servidor)
export async function register() {
  const { registerFaro } = await import('@iaportafolio/nextjs/server');
  registerFaro({
    endpoint: process.env.FARO_ENDPOINT!,
    token: process.env.FARO_TOKEN!,
    service: 'mi-next-app',
  });
}

// app/faro-client.tsx (navegador)
'use client';
import { useEffect } from 'react';
import { initFaroClient } from '@iaportafolio/nextjs/client';

export function FaroClient() {
  useEffect(() => {
    initFaroClient({
      endpoint: process.env.NEXT_PUBLIC_FARO_ENDPOINT!,
      token: process.env.NEXT_PUBLIC_FARO_TOKEN!,
      service: 'mi-next-app-web',
    });
  }, []);
  return null;
}`,
    groups: [
      {
        title: 'Servidor',
        note: "Subimport '@iaportafolio/nextjs/server' — hereda toda la API de @iaportafolio/node.",
        methods: [
          { signature: 'registerFaro(options)', summary: 'Inicializa Faro en el runtime de servidor (instrumentation.ts).' },
          { signature: 'captureRequestError(err, request, context)', summary: 'Hook onRequestError de Next 15 para reportar errores de render/route.' }
        ]
      },
      {
        title: 'Cliente — inicialización',
        note: "Subimport '@iaportafolio/nextjs/client'.",
        methods: [
          { signature: 'initFaroClient(options)', summary: 'Inicializa el RUM en el navegador (Web Vitals, errores, breadcrumbs). Llamar en useEffect.', returns: 'FaroBrowser' },
          { signature: 'getClient()', summary: 'Devuelve la instancia de RUM ya inicializada.', returns: 'FaroBrowser' },
          { signature: 'getSessionId()', summary: 'ID de sesión RUM actual (persistido en el navegador).', returns: 'string' },
          { signature: 'flush()', summary: 'Vacía las colas pendientes (usa sendBeacon en pagehide).', returns: 'Promise<void>' },
          { signature: 'close()', summary: 'Desinstala los listeners del navegador.' }
        ]
      },
      {
        title: 'Cliente — logs y errores',
        methods: [
          { signature: 'log(entry)', summary: 'Envía un log estructurado desde el navegador.' },
          { signature: 'info / warn / warning / error(msg, attrs?)', summary: 'Atajos de logging por nivel.' },
          { signature: 'captureException(err, ctx?)', summary: 'Captura una excepción con stack trace y tags.' },
          { signature: 'setUser(user)', summary: 'Fija el usuario activo; enriquece logs y errores siguientes.' },
          { signature: 'addBreadcrumb(crumb)', summary: 'Añade un breadcrumb manual a la traza de errores.' }
        ]
      },
      {
        title: 'Cliente — product analytics',
        methods: [
          { signature: 'track(event, properties?)', summary: 'Emite un product event personalizado.' },
          { signature: 'identify(userId, traits?)', summary: 'Asocia los eventos futuros a un usuario.' },
          { signature: 'page(path?, properties?)', summary: 'Registra una vista de página (pageview de RUM web).' },
          { signature: 'alias(prevId, newId)', summary: 'Fusiona la sesión anónima con el usuario.' }
        ]
      },
      {
        title: 'Cliente — feature flags y replay',
        methods: [
          { signature: 'isFeatureEnabled(key, context?)', summary: 'Evalúa un flag contra el snapshot local.', returns: 'boolean' },
          { signature: 'refreshFeatureFlags()', summary: 'Refresca el snapshot de flags desde el backend.', returns: 'Promise<void>' },
          { signature: 'captureSessionReplay (opción de init)', summary: 'Activa la grabación de sesión (rrweb) pasando `captureSessionReplay: true` a `initFaroClient`. Ver también `sessionReplaySampleRate` y `sessionReplayMaskAllText`.' },
          { signature: 'getSessionId()', summary: 'Devuelve el ID de sesión actual del RUM.', returns: 'string' },
          { signature: '<FaroErrorBoundary>', summary: 'Componente React que captura errores de render del árbol hijo.' }
        ]
      }
    ]
  },
  {
    id: 'expo',
    name: 'Expo / React Native',
    language: 'TypeScript · móvil',
    pkg: '@iaportafolio/expo',
    install: 'npx expo install @iaportafolio/expo',
    profile: 'mobile',
    blurb: 'SDK móvil con logs, errores y product analytics; flush automático en background y captura de fatales.',
    capabilities: ['Logs', 'Errores', 'Product analytics', 'Feature flags'],
    lang: 'ts',
    initExample: `import * as faro from '@iaportafolio/expo';

faro.init({
  endpoint: 'https://faro.iaportafolio.com',
  token: process.env.EXPO_PUBLIC_FARO_TOKEN!,
  service: 'mi-app-mobile',
  environment: __DEV__ ? 'dev' : 'production',
});

faro.info('app montada');`,
    groups: [
      {
        title: 'Inicialización y ciclo de vida',
        methods: [
          { signature: 'init(options)', summary: 'Configura el SDK; instala AppState y ErrorUtils para flush y fatales.', returns: 'FaroExpoClient' },
          { signature: 'flush()', summary: 'Vacía las colas pendientes.', returns: 'Promise<void>' },
          { signature: 'close()', summary: 'Libera los listeners; necesario antes de re-init().' }
        ]
      },
      {
        title: 'Logging',
        methods: [
          { signature: 'info(msg, attrs?)', summary: 'Log de nivel INFO.' },
          { signature: 'warn(msg, attrs?)', summary: 'Log de nivel WARN.' },
          { signature: 'warning(msg, attrs?)', summary: 'Alias de warn().' },
          { signature: 'error(msg, attrs?)', summary: 'Log de nivel ERROR.' }
        ]
      },
      {
        title: 'Errores',
        methods: [
          { signature: 'captureException(err, ctx?)', summary: 'Captura una excepción con stack trace y tags.' }
        ]
      },
      {
        title: 'Product analytics',
        methods: [
          { signature: 'track(event, properties?)', summary: 'Emite un product event personalizado.' },
          { signature: 'identify(userId, traits?)', summary: 'Asocia los eventos futuros a un usuario.' },
          { signature: 'screen(name, properties?)', summary: 'Registra una vista de pantalla (screen view móvil).' },
          { signature: 'alias(prevId, newId)', summary: 'Fusiona la sesión anónima con el usuario.' }
        ]
      },
      {
        title: 'Feature flags',
        methods: [
          { signature: 'isFeatureEnabled(key, context?)', summary: 'Evalúa un flag contra el snapshot local (sin red).', returns: 'boolean' },
          { signature: 'refreshFeatureFlags()', summary: 'Fuerza un refresh del snapshot de flags desde el backend.', returns: 'Promise<void>' }
        ]
      }
    ]
  },
  {
    id: 'python',
    name: 'Python',
    language: 'Python 3 · servidor',
    pkg: 'faro-sdk',
    install: 'pip install faro-sdk',
    profile: 'server',
    blurb: 'SDK de servidor con logs, errores, product analytics, tracing OTel, handler de logging y middlewares WSGI/ASGI.',
    capabilities: ['Logs', 'Errores', 'Product analytics', 'Tracing', 'Feature flags'],
    lang: 'py',
    initExample: `import faro_sdk as faro

faro.init(
    endpoint='https://faro.iaportafolio.com',
    token='<token>',
    service='mi-servicio',
    environment='production',
)

faro.info('arranque ok', port=8080)`,
    groups: [
      {
        title: 'Inicialización y ciclo de vida',
        methods: [
          { signature: 'init(endpoint, token, service, …)', summary: 'Configura el SDK; registra atexit para el cierre. Cae en FARO_ENDPOINT/FARO_TOKEN si faltan.' },
          { signature: 'flush(timeout=5.0)', summary: 'Espera a que el buffer pendiente se envíe.' },
          { signature: 'close(timeout=5.0)', summary: 'Flush final + join del worker daemon.' }
        ]
      },
      {
        title: 'Logging',
        methods: [
          { signature: 'log(level, message, **attrs)', summary: 'Envía un log estructurado con nivel arbitrario.' },
          { signature: 'info(message, **attrs)', summary: 'Log de nivel INFO.' },
          { signature: 'warn(message, **attrs)', summary: 'Log de nivel WARN.' },
          { signature: 'warning(message, **attrs)', summary: 'Alias de warn() (paridad con logging.WARNING).' },
          { signature: 'error(message, **attrs)', summary: 'Log de nivel ERROR.' }
        ]
      },
      {
        title: 'Errores',
        methods: [
          { signature: 'capture_exception(exc, tags=None, message=None)', summary: 'Captura una excepción con stack trace y tags.' }
        ]
      },
      {
        title: 'Product analytics',
        methods: [
          { signature: 'track(event_name, properties=None)', summary: 'Emite un product event personalizado.' },
          { signature: 'identify(user_id, traits=None)', summary: 'Asocia los eventos futuros a un usuario.' },
          { signature: 'alias(prev_id, new_id)', summary: 'Fusiona la sesión anónima con el usuario.' }
        ]
      },
      {
        title: 'Feature flags',
        methods: [
          { signature: 'is_feature_enabled(key, distinct_id=None, properties=None)', summary: 'Evalúa un flag contra el snapshot local (sin red).', returns: 'bool' },
          { signature: 'refresh_feature_flags()', summary: 'Fuerza un refresh del snapshot de flags desde el backend.' }
        ]
      },
      {
        title: 'Tracing (OpenTelemetry)',
        methods: [
          { signature: 'start_span(name, …)', summary: 'Abre un span manual.', returns: 'Span' },
          { signature: 'use_span(name, …)', summary: 'Context manager: ejecuta un bloque dentro de un span.' },
          { signature: 'active_span()', summary: 'Devuelve el span activo.', returns: 'Span | None' },
          { signature: 'init_tracing(…)', summary: 'Inicializa OTel manualmente.', returns: 'bool' },
          { signature: 'flush_tracing(timeout_ms=5000)', summary: 'Vacía los spans pendientes.' },
          { signature: 'shutdown_tracing(timeout_ms=5000)', summary: 'Cierra el tracer provider.' },
          { signature: 'get_tracer()', summary: 'Devuelve el Tracer de OTel.' }
        ]
      },
      {
        title: 'Integraciones',
        methods: [
          { signature: 'FaroHandler', summary: 'logging.Handler que reenvía los registros del logging estándar a Faro.' },
          { signature: 'FaroWsgiMiddleware', summary: 'Middleware WSGI que abre un span por request (Flask/Django).' },
          { signature: 'FaroAsgiMiddleware', summary: 'Middleware ASGI que abre un span por request (FastAPI/Starlette).' }
        ]
      }
    ]
  },
  {
    id: 'go',
    name: 'Go',
    language: 'Go · servidor',
    pkg: 'github.com/IA-Portafolio/faro/sdks/go',
    install: 'go get github.com/IA-Portafolio/faro/sdks/go',
    profile: 'server',
    blurb: 'SDK de servidor con logs, errores, product analytics y tracing; variantes *Context para propagar el trace del request.',
    capabilities: ['Logs', 'Errores', 'Product analytics', 'Tracing', 'Feature flags'],
    lang: 'go',
    initExample: `import faro "github.com/IA-Portafolio/faro/sdks/go"

faro.Init(faro.Options{
    Endpoint:    "https://faro.iaportafolio.com",
    Token:       "<token>",
    Service:     "mi-servicio",
    Environment: "production",
})
defer faro.Close(context.Background())

faro.Info("arranque ok", map[string]any{"port": 8080})`,
    groups: [
      {
        title: 'Inicialización y ciclo de vida',
        methods: [
          { signature: 'Init(opts)', summary: 'Inicializa el cliente por defecto del paquete.', returns: 'error' },
          { signature: 'New(opts)', summary: 'Crea un cliente independiente (sin singleton).', returns: '(*Client, error)' },
          { signature: 'Default()', summary: 'Devuelve el cliente por defecto.', returns: '*Client' },
          { signature: 'Flush(timeout)', summary: 'Espera a que el buffer pendiente se envíe.', returns: 'error' },
          { signature: 'Close(ctx)', summary: 'Flush final acotado por el contexto. Úsalo con defer.', returns: 'error' }
        ]
      },
      {
        title: 'Logging',
        methods: [
          { signature: 'Log(level, msg, attrs)', summary: 'Envía un log estructurado con nivel arbitrario.' },
          { signature: 'Info / Warn / Warning / Error(msg, attrs)', summary: 'Atajos de logging por nivel (Warning es alias de Warn).' },
          { signature: 'LogContext(ctx, level, msg, attrs)', summary: 'Como Log, adjuntando trace_id/span_id del contexto.' },
          { signature: 'InfoContext / WarnContext / ErrorContext(ctx, msg, attrs)', summary: 'Atajos con propagación de trace desde el contexto.' }
        ]
      },
      {
        title: 'Errores',
        methods: [
          { signature: 'CaptureException(err, tags)', summary: 'Captura un error con stack trace y tags.' },
          { signature: 'Recover(tags)', summary: 'Helper para defer: captura el panic y lo reenvía como excepción.' }
        ]
      },
      {
        title: 'Product analytics',
        methods: [
          { signature: 'Track(eventName, properties)', summary: 'Emite un product event personalizado.' },
          { signature: 'TrackContext(ctx, eventName, properties)', summary: 'Como Track, correlacionando con el trace del request.' },
          { signature: 'Identify(userID, traits)', summary: 'Asocia los eventos futuros a un usuario.' },
          { signature: 'Alias(prevID, newID)', summary: 'Fusiona la sesión anónima con el usuario.' }
        ]
      },
      {
        title: 'Feature flags',
        methods: [
          { signature: 'IsFeatureEnabled(key, ctx)', summary: 'Evalúa un flag contra el snapshot local (sin red). ctx es faro.FlagContext{DistinctID, Properties}.', returns: 'bool' },
          { signature: 'RefreshFeatureFlags(ctx)', summary: 'Fuerza un refresh del snapshot de flags desde el backend.' }
        ]
      },
      {
        title: 'Tracing (OpenTelemetry)',
        methods: [
          { signature: 'StartSpan(ctx, name, opts)', summary: 'Abre un span manual y devuelve el contexto hijo.', returns: '(context.Context, *Span)' },
          { signature: 'WithSpan(ctx, name, fn, opts)', summary: 'Ejecuta fn dentro de un span y lo cierra al volver.', returns: 'error' },
          { signature: 'InitTracing(opts)', summary: 'Inicializa OTel manualmente.', returns: '(bool, error)' },
          { signature: 'FlushTracing(ctx) / ShutdownTracing(ctx)', summary: 'Vacía o cierra el tracer provider.', returns: 'error' },
          { signature: 'GetTracer()', summary: 'Devuelve el Tracer de OTel.', returns: 'trace.Tracer' },
          { signature: 'WithTraceparent(ctx, header)', summary: 'Inyecta un traceparent W3C en el contexto.' },
          { signature: 'SpanFromContext(ctx) / ContextWithSpan(ctx, span)', summary: 'Lee o coloca el span activo en el contexto.' }
        ]
      },
      {
        title: 'Span (métodos)',
        methods: [
          { signature: 'span.SetAttribute(k, v) / SetAttributes(map)', summary: 'Añade atributos al span.' },
          { signature: 'span.AddEvent(name, attrs)', summary: 'Registra un evento dentro del span.' },
          { signature: 'span.SetStatus(code, message)', summary: 'Fija el estado del span (OK/ERROR).' },
          { signature: 'span.RecordException(err)', summary: 'Adjunta una excepción al span.' },
          { signature: 'span.End()', summary: 'Cierra el span.' },
          { signature: 'span.TraceID() / SpanID() / Traceparent()', summary: 'Devuelve los identificadores del span.', returns: 'string' }
        ]
      },
      {
        title: 'Integraciones',
        methods: [
          { signature: 'HTTPMiddleware(next)', summary: 'Middleware net/http que abre un span por request.', returns: 'http.Handler' },
          { signature: 'ginfaro.Tracing()', summary: 'Middleware de Gin que abre un span SERVER por request (subpaquete ginfaro).' }
        ]
      }
    ]
  },
  {
    id: 'flutter',
    name: 'Flutter',
    language: 'Dart · móvil + web',
    pkg: 'faro_sdk',
    install: 'flutter pub add faro_sdk',
    profile: 'mobile',
    blurb: 'SDK Dart con logs, errores y la API de producto completa (track/identify/page/screen/alias); flush por lifecycle.',
    capabilities: ['Logs', 'Errores', 'Product analytics', 'Feature flags'],
    lang: 'dart',
    initExample: `import 'package:faro_sdk/faro_sdk.dart';

Faro.run(
  options: const FaroOptions(
    endpoint: 'https://faro.iaportafolio.com',
    token: '<token>',
    service: 'mi-app-mobile',
    environment: 'production',
  ),
  appRunner: () => runApp(const MyApp()),
);

Faro.instance.info('login ok', {'user_id': 42});`,
    groups: [
      {
        title: 'Inicialización y ciclo de vida',
        methods: [
          { signature: 'Faro.init(options)', summary: 'Configura el SDK manualmente.', returns: 'Faro' },
          { signature: 'Faro.run({options, appRunner})', summary: 'Inicializa y arranca la app en una zona que captura todos los errores.' },
          { signature: 'Faro.instance', summary: 'Accede al singleton tras init/run.', returns: 'Faro' },
          { signature: 'flush()', summary: 'Vacía las colas pendientes.', returns: 'Future<void>' },
          { signature: 'close()', summary: 'Flush final + libera el observer de lifecycle.', returns: 'Future<void>' }
        ]
      },
      {
        title: 'Logging',
        methods: [
          { signature: 'log({level, message, attrs})', summary: 'Envía un log estructurado.' },
          { signature: 'info(message, [attrs])', summary: 'Log de nivel INFO.' },
          { signature: 'warn(message, [attrs])', summary: 'Log de nivel WARN.' },
          { signature: 'warning(message, [attrs])', summary: 'Alias de warn().' },
          { signature: 'error(message, [attrs])', summary: 'Log de nivel ERROR.' }
        ]
      },
      {
        title: 'Errores',
        methods: [
          { signature: 'captureException(e, {stack, tags})', summary: 'Captura una excepción con stack trace y tags.' }
        ]
      },
      {
        title: 'Product analytics',
        methods: [
          { signature: 'track(eventName, [properties])', summary: 'Emite un product event personalizado.' },
          { signature: 'identify(userId, [traits])', summary: 'Asocia los eventos futuros a un usuario.' },
          { signature: 'page(path, [properties])', summary: 'Vista de página (Flutter web).' },
          { signature: 'screen(screenName, [properties])', summary: 'Vista de pantalla (móvil).' },
          { signature: 'alias(prevId, newId)', summary: 'Fusiona la sesión anónima con el usuario.' }
        ]
      },
      {
        title: 'Feature flags',
        methods: [
          { signature: 'isFeatureEnabled(key, {distinctId, properties})', summary: 'Evalúa un flag contra el snapshot local (sin red).', returns: 'bool' },
          { signature: 'refreshFeatureFlags()', summary: 'Fuerza un refresh del snapshot de flags desde el backend.', returns: 'Future<void>' }
        ]
      }
    ]
  },
  {
    id: 'kotlin',
    name: 'Kotlin / Android',
    language: 'Kotlin · Android + JVM',
    pkg: 'com.iaportafolio:faro',
    install: 'implementation("com.iaportafolio:faro:0.1.0")',
    profile: 'mobile',
    blurb: 'SDK para Android/JVM con logs, errores y product analytics; captura de excepciones no manejadas por defecto.',
    capabilities: ['Logs', 'Errores', 'Product analytics', 'Feature flags'],
    lang: 'kotlin',
    initExample: `import com.iaportafolio.faro.Faro
import com.iaportafolio.faro.FaroOptions

Faro.init(FaroOptions(
    endpoint = "https://faro.iaportafolio.com",
    token = "<token>",
    service = "android-app",
    environment = "production",
    release = BuildConfig.VERSION_NAME,
))

Faro.info("login ok", mapOf("user_id" to user.id))`,
    groups: [
      {
        title: 'Inicialización y ciclo de vida',
        methods: [
          { signature: 'Faro.init(options)', summary: 'Configura el SDK e instala el handler de excepciones no manejadas.' },
          { signature: 'Faro.flush(timeoutMs = 3000)', summary: 'Espera a que el buffer pendiente se envíe.' },
          { signature: 'Faro.close()', summary: 'Flush final + libera recursos.' }
        ]
      },
      {
        title: 'Logging',
        methods: [
          { signature: 'log(level, message, attrs)', summary: 'Envía un log estructurado con nivel arbitrario.' },
          { signature: 'info(message, attrs)', summary: 'Log de nivel INFO.' },
          { signature: 'warn(message, attrs)', summary: 'Log de nivel WARN.' },
          { signature: 'warning(message, attrs)', summary: 'Alias de warn().' },
          { signature: 'error(message, attrs)', summary: 'Log de nivel ERROR.' }
        ]
      },
      {
        title: 'Errores',
        methods: [
          { signature: 'captureException(e, tags)', summary: 'Captura un Throwable con stack trace y tags.' }
        ]
      },
      {
        title: 'Product analytics',
        methods: [
          { signature: 'track(eventName, properties)', summary: 'Emite un product event personalizado.' },
          { signature: 'identify(userId, traits)', summary: 'Asocia los eventos futuros a un usuario.' },
          { signature: 'screen(screenName, properties)', summary: 'Vista de pantalla (móvil).' },
          { signature: 'alias(prevId, newId)', summary: 'Fusiona la sesión anónima con el usuario.' }
        ]
      },
      {
        title: 'Feature flags',
        methods: [
          { signature: 'isFeatureEnabled(key, distinctId?, properties?)', summary: 'Evalúa un flag contra el snapshot local (sin red).', returns: 'Boolean' },
          { signature: 'refreshFeatureFlags()', summary: 'Fuerza un refresh del snapshot de flags desde el backend.' }
        ]
      }
    ]
  }
];

/** Total de métodos documentados (para el resumen de la cabecera). */
export function totalMethods(): number {
  return sdks.reduce((n, s) => n + s.groups.reduce((g, grp) => g + grp.methods.length, 0), 0);
}
