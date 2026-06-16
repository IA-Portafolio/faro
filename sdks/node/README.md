# @iaportafolio/node

SDK de Node.js / TypeScript para Faro.

> **Perfil de defaults:** `server` — flush 750ms · batch 200 · queue 10 000. Ver [perfiles](../README.md#perfiles-de-defaults).

## Instalación

```bash
npm install @iaportafolio/node
```

Requiere Node.js ≥ 18 (usa `fetch` nativo).

## Uso

```ts
import * as faro from '@iaportafolio/node';

faro.init({
  endpoint: 'https://faro.iaportafolio.com',
  token: process.env.FARO_TOKEN!,           // visible en /projects → SDK
  service: 'pagos-api',
  environment: 'production',
  release: process.env.GIT_COMMIT,
  attributes: { region: 'eu-west-1' },
});

faro.info('servidor arrancado', { port: 8080 });

try {
  await charge(order);
} catch (err) {
  faro.captureException(err, { tags: { order_id: order.id } });
  throw err;
}
```

## Captura automática

El SDK instala handlers globales para `uncaughtException`, `unhandledRejection` y `beforeExit` (drena el buffer al salir). Para desactivar:

```ts
faro.init({ ..., installGlobalHandlers: false });
```

## Opciones de tuning

| Opción | Default | Descripción |
| ------ | ------- | ----------- |
| `flushIntervalMs` | `750` | Cadencia de flush (ms). Más bajo = más RTT, más realtime. |
| `maxQueueSize` | `10000` | Cap de la cola. Al llenarse, descarta el evento más viejo (backpressure). |
| `batchSize` | `100` | Eventos por POST. El backend rechaza batches >100 con 400. |
| `installGlobalHandlers` | `true` | `uncaughtException`/`unhandledRejection`/`beforeExit`. |

```ts
faro.init({
  endpoint, token, service,
  flushIntervalMs: 2000,   // menos RTT en batch
  maxQueueSize: 5000,      // cap más bajo para lambdas con memoria limitada
});
```

## Cierre limpio

En scripts de corta duración (cron, CI, lambdas) llama a `flush()` o `close()` antes de salir para no perder eventos:

```ts
await faro.flush();
// o
await faro.close();
```

## Feature flags

Calienta la cache al arrancar y evalúa localmente. El SDK vuelve a pedir flags cada 30s.

```ts
await faro.refreshFeatureFlags();

if (faro.isFeatureEnabled('new-checkout', {
  distinct_id: 'user_42',
  properties: { plan: 'pro' },
})) {
  renderNewCheckout();
}
```

Las reglas de flags se descargan al proceso cliente; no pongas secretos en `conditions`.

## Product analytics

El SDK soporta la API de producto (estilo Segment/PostHog):

```ts
// Eventos de producto
faro.track('checkout_completed', { amount: 99.50, currency: 'USD' });

// Identificar usuario
faro.identify('user_42', { email: 'a@b.com', plan: 'pro' });

// Fusionar sesión anónima con usuario post-login
faro.alias('anon_abc123', 'user_42');
```

`track()` se correlaciona automáticamente con el span activo (incluye `trace_id`/`span_id` si hay OTel inicializado). Ver [API uniforme](../README.md#api-uniforme-entre-sdks) para la semántica de `anonymous_id`/`distinct_id`/`session_id`.

## Tracing (OpenTelemetry)

El SDK incluye auto-instrumentación OTel: con una sola llamada obtienes spans de
`http`, `express`, `fastify`, `koa`, `pg`, `mysql`, `mongodb`, `redis`, `ioredis`,
`grpc`, `kafka`, etc. Los spans se envían por OTLP/HTTP/JSON al endpoint de Faro
y alimentan el Service Map y la pestaña Trazas.

### Opción A — `--import` (recomendado)

La auto-instrumentación necesita registrarse **antes** de importar las librerías
a instrumentar. El flag `--import` lo garantiza:

```bash
node --import @iaportafolio/node/instrument server.js
```

Lee de env vars:

| Variable | Descripción |
| -------- | ----------- |
| `FARO_ENDPOINT` | URL de Faro. Requerida. |
| `FARO_INGEST_TOKEN` | Token de proyecto. Requerida. |
| `OTEL_SERVICE_NAME` | `service.name`. Requerida. |
| `OTEL_SERVICE_VERSION` | `service.version`. Opcional. |
| `DEPLOYMENT_ENVIRONMENT` | `deployment.environment`. Opcional. |
| `FARO_TRACES_ENDPOINT` | Override del endpoint completo de traces. Opcional. |
| `FARO_DISABLED_INSTRUMENTATIONS` | Lista separada por comas. Opcional. |

### Opción B — desde código

Si preferís inicializar en código, llamá a `initTracing` en la **primera línea**
del entrypoint, antes de cualquier otro import:

```ts
import { initTracing } from '@iaportafolio/node/tracing';

initTracing({
  endpoint: 'https://faro.iaportafolio.com',
  token: process.env.FARO_TOKEN!,
  service: 'pagos-api',
  environment: 'production',
  release: process.env.GIT_COMMIT,
});

// recién ahora importá express, pg, etc.
import express from 'express';
```

### Spans manuales

```ts
import * as faro from '@iaportafolio/node';

// Abrir y cerrar manualmente
const span = faro.startSpan('procesar-pago', { kind: 'INTERNAL' });
try {
  await charge(order);
  span.setStatus('OK');
} catch (err) {
  span.setStatus('ERROR', err.message);
  throw err;
} finally {
  span.end();
}

// O con withSpan (cierra solo)
await faro.withSpan('db-query', async (span) => {
  span.setAttribute('db.system', 'postgresql');
  return db.query('SELECT 1');
});
```

API disponible: `startSpan(name, opts?)`, `withSpan(name, fn, opts?)`,
`activeSpan()`, `initTracing(opts)`, `flushTracing(timeoutMs?)`,
`shutdownTracing(timeoutMs?)`, `getTracer()`, `parseTraceparent(header)`.

## Métricas

El SDK expone instrumentos de métricas nativas (gauges, counters, histograms) que
se envían al endpoint de ingesta nativa de Faro:

```ts
import * as faro from '@iaportafolio/node';

const orders = faro.counter('orders_total', { description: 'Pedidos procesados' });
orders.add(1, { status: 'success' });

const queueDepth = faro.gauge('queue_depth');
queueDepth.set(42);

const latency = faro.histogram('http_request_duration_ms');
latency.record(127.5);
```

API: `counter(name, opts?)`, `upDownCounter(name, opts?)`, `gauge(name, opts?)`,
`histogram(name, opts?)`.

## Middleware Express

`@iaportafolio/node/express` abre un span SERVER por request y propaga el
`traceparent` al response. Logs emitidos dentro del handler se auto-correlacionan
con el span via `AsyncLocalStorage`:

```ts
import express from 'express';
import * as faro from '@iaportafolio/node';
import { expressTracer } from '@iaportafolio/node/express';

faro.init({ endpoint: '...', token: '...', service: 'api' });

const app = express();
app.use(expressTracer());

app.get('/charge', (req, res) => {
  faro.info('procesando cobro'); // auto-trae trace_id del span del middleware
  res.json({ ok: true });
});
```

## Auto-correlación con traces

`track()` adjunta `trace_id`/`span_id` automáticamente cuando hay un span activo de OpenTelemetry (`@opentelemetry/api` instalado en la app) o cuando defines un provider explícito. El provider puede devolver un header W3C `traceparent` o los IDs normalizados:

```ts
faro.init({
  endpoint: 'https://faro.iaportafolio.com',
  token: process.env.FARO_TOKEN!,
  service: 'pagos-api',
  traceContext: () => '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01',
});

faro.track('checkout_completed'); // incluye trace_id + span_id en /ingest/events
```

## Integración con loggers

El paquete incluye bridges para los dos loggers dominantes del ecosistema Node, así no tienes que llamar a `logger.info(...)` y a `faro.info(...)` por separado.

### pino

Transport oficial (pino 7+, corre en worker thread):

```bash
npm i pino pino-abstract-transport
```

```ts
import pino from 'pino';

const logger = pino({
  transport: {
    target: '@iaportafolio/node/pino',
    options: {
      endpoint: 'https://faro.iaportafolio.com',
      token: process.env.FARO_TOKEN!,
      service: 'pagos-api',
      environment: 'production',
    },
  },
});

logger.info({ orderId: '42' }, 'pedido creado');
// → llega a Faro como { level: 'INFO', message: 'pedido creado', attributes: { orderId: '42' } }
```

El transport vive en un worker thread aislado, así que el `faro.init()` del thread principal **no** se comparte — pásale todas las opciones (incluidos `attributes`, `scrubFields`, `beforeSend`) en `options`. Pino llama a `close()` automáticamente al cerrar.

Mapeo de niveles: `10→TRACE`, `20→DEBUG`, `30→INFO`, `40→WARN`, `50→ERROR`, `60→FATAL`.

### winston

Custom `Transport` class:

```bash
npm i winston winston-transport
```

```ts
import winston from 'winston';
import { FaroTransport } from '@iaportafolio/node/winston';

// Opción A: el transport hace init() por ti
const logger = winston.createLogger({
  transports: [
    new FaroTransport({
      endpoint: 'https://faro.iaportafolio.com',
      token: process.env.FARO_TOKEN!,
      service: 'pagos-api',
    }),
  ],
});

// Opción B: ya hiciste faro.init() en tu bootstrap → pasa el cliente y comparte estado
import * as faro from '@iaportafolio/node';
faro.init({ endpoint, token, service });
const logger2 = winston.createLogger({
  transports: [new FaroTransport({ client: faro.getClient() })],
});

logger.warn('rate limit cerca', { tenant: 'acme' });
// → llega a Faro como { level: 'WARN', message: 'rate limit cerca', attributes: { tenant: 'acme' } }
```

Mapeo de niveles cubre tanto la escala `npm` de winston (`error/warn/info/http/verbose/debug/silly`) como `syslog` (`emerg/alert/crit/error/warning/notice/info/debug`).

### Cuándo NO usar los transports

Si tu logger emite a muchos destinos (consola + fichero + Faro), los transports añaden serialización extra. Para volúmenes muy altos, prefiere `faro.log()` directamente desde los puntos críticos y deja `logger.info()` solo para consola/disco.
