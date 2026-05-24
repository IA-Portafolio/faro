/**
 * Transport de Faro para [pino](https://github.com/pinojs/pino) (v7+).
 *
 * Se usa vía la API de transport con worker thread:
 *
 * ```ts
 * import pino from 'pino';
 *
 * const logger = pino({
 *   transport: {
 *     target: '@iaportafolio/node/pino',
 *     options: {
 *       endpoint: 'https://faro.iaportafolio.com',
 *       token: process.env.FARO_TOKEN!,
 *       service: 'pagos-api',
 *     },
 *   },
 * });
 *
 * logger.info({ orderId: '42' }, 'pedido creado');
 * ```
 *
 * Como pino lanza los transports en un worker thread aislado, este módulo
 * llama internamente a `faro.init()` con las opciones pasadas — el singleton
 * de Faro del thread principal NO se comparte. Si necesitas compartir estado
 * (atributos globales, scrubbing custom), pásalo todo aquí.
 *
 * Peer dep opcional: `pino-abstract-transport`. Instálalo si vas a usar este
 * subimport: `npm i pino pino-abstract-transport`.
 */

import type { Transform } from 'stream';
import { init, getClient, type FaroOptions, type Severity } from './index.js';

// Firma minimal de `build` (pino-abstract-transport usa `export = build`, así que
// `import('pino-abstract-transport').default` es la función entera vía esModuleInterop).
type PinoBuild = (
  fn: (source: AsyncIterable<unknown>) => Promise<void> | void,
  opts?: { close?: (err?: unknown, cb?: () => void) => void | Promise<void> },
) => Transform;

// Pino numeric levels → severidades OTel. Pino: 10 trace, 20 debug, 30 info, 40 warn, 50 error, 60 fatal.
function mapPinoLevel(n: number | string | undefined): Severity {
  const v = typeof n === 'string' ? parseInt(n, 10) : n;
  if (v == null || isNaN(v)) return 'INFO';
  if (v <= 10) return 'TRACE';
  if (v <= 20) return 'DEBUG';
  if (v <= 30) return 'INFO';
  if (v <= 40) return 'WARN';
  if (v <= 50) return 'ERROR';
  return 'FATAL';
}

// Campos que pino añade siempre — los excluimos del bag de atributos para no duplicar.
const PINO_RESERVED = new Set(['level', 'time', 'msg', 'pid', 'hostname', 'v']);

function extractAttrs(obj: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const k of Object.keys(obj)) {
    if (PINO_RESERVED.has(k)) continue;
    out[k] = obj[k];
  }
  return out;
}

/**
 * Default export — pino llama a esta función pasándole las `options` del transport.
 */
export default async function faroTransport(opts: FaroOptions): Promise<Transform> {
  init(opts);

  // Carga diferida: `pino-abstract-transport` solo se necesita cuando se usa este transport.
  // Si no está instalado, lanzamos un error claro en lugar de fallar críptico.
  let build: PinoBuild;
  try {
    const mod = await import('pino-abstract-transport');
    // `export = build` (CJS) → vía esModuleInterop el callable cuelga de `.default`.
    build = ((mod as { default?: unknown }).default ?? mod) as PinoBuild;
  } catch {
    throw new Error(
      '@iaportafolio/node/pino requiere `pino-abstract-transport` instalado como peer dep. ' +
        'Ejecuta: npm i pino-abstract-transport',
    );
  }

  return build(
    async (source) => {
      for await (const raw of source) {
        const obj = raw as Record<string, unknown>;
        try {
          getClient().log({
            level: mapPinoLevel(obj.level as number | string | undefined),
            message: typeof obj.msg === 'string' ? obj.msg : '',
            attributes: extractAttrs(obj),
            timestamp: typeof obj.time === 'number' ? new Date(obj.time) : undefined,
          });
        } catch {
          // Best-effort: nunca tumbar al logger del usuario por un error nuestro.
        }
      }
    },
    {
      async close() {
        // Pino llama a close() al final — vacía la cola pendiente.
        try {
          await getClient().close(5000);
        } catch {
          /* noop */
        }
      },
    },
  );
}
