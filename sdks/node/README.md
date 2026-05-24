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
