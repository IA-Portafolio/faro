# @iaportafolio/node

SDK de Node.js / TypeScript para Faro.

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

## Logger integrado (pino / winston)

`@iaportafolio/node` no instala hooks en otros loggers; usa los atajos `info/warn/error` directamente o llama desde un transport custom:

```ts
// pino transport simplificado
import pino from 'pino';
const logger = pino({
  hooks: {
    logMethod(args, method, level) {
      faro.log({ level: level.toUpperCase() as faro.Severity, message: args[0], attributes: args[1] });
      method.apply(this, args);
    },
  },
});
```
