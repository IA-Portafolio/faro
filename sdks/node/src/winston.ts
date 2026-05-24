/**
 * Transport de Faro para [winston](https://github.com/winstonjs/winston).
 *
 * ```ts
 * import winston from 'winston';
 * import { FaroTransport } from '@iaportafolio/node/winston';
 * import * as faro from '@iaportafolio/node';
 *
 * // Opción A: el transport inicializa Faro
 * const logger = winston.createLogger({
 *   transports: [
 *     new FaroTransport({
 *       endpoint: 'https://faro.iaportafolio.com',
 *       token: process.env.FARO_TOKEN!,
 *       service: 'pagos-api',
 *     }),
 *   ],
 * });
 *
 * // Opción B: ya llamaste a faro.init() en tu bootstrap → pasa solo el cliente
 * faro.init({ endpoint, token, service });
 * const logger2 = winston.createLogger({
 *   transports: [new FaroTransport({ client: faro.getClient() })],
 * });
 * ```
 *
 * Peer dep opcional: `winston-transport`. Instálalo si vas a usar este subimport:
 * `npm i winston winston-transport`.
 */

import { createRequire } from 'node:module';
import { init, getClient, type FaroOptions, type Severity } from './index.js';

// Niveles de winston (npm) → severidades OTel.
// winston npm levels: error=0, warn=1, info=2, http=3, verbose=4, debug=5, silly=6
// syslog (RFC5424): emerg=0, alert=1, crit=2, error=3, warning=4, notice=5, info=6, debug=7
function mapWinstonLevel(lvl: string | undefined): Severity {
  if (!lvl) return 'INFO';
  const k = lvl.toLowerCase();
  // Comunes a ambas escalas + casos que aparecen en la práctica.
  if (k === 'silly' || k === 'trace') return 'TRACE';
  if (k === 'debug' || k === 'verbose' || k === 'http') return 'DEBUG';
  if (k === 'info' || k === 'notice') return 'INFO';
  if (k === 'warn' || k === 'warning') return 'WARN';
  if (k === 'error' || k === 'err' || k === 'crit' || k === 'alert') return 'ERROR';
  if (k === 'emerg' || k === 'fatal') return 'FATAL';
  return 'INFO';
}

// Campos que winston añade siempre — los excluimos del bag de atributos.
const WINSTON_RESERVED = new Set([
  'level', 'message', 'timestamp',
  // Symbol-keyed `level`/`splat`/`message` de winston (formatters internos): se filtran solos.
]);

function extractAttrs(info: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const k of Object.keys(info)) {
    if (WINSTON_RESERVED.has(k)) continue;
    out[k] = info[k];
  }
  return out;
}

interface WinstonTransportStreamOptions {
  level?: string;
  silent?: boolean;
  handleExceptions?: boolean;
  handleRejections?: boolean;
}

interface WinstonTransportCtor {
  new (opts?: WinstonTransportStreamOptions): {
    emit: (event: string, info: unknown) => void;
  };
}

type FaroTransportOptions =
  | (FaroOptions & WinstonTransportStreamOptions)
  | ({ client: ReturnType<typeof getClient> } & WinstonTransportStreamOptions);

function loadWinstonTransport(): WinstonTransportCtor {
  try {
    // createRequire funciona también en bundles ESM (Node aporta `require` via createRequire).
    // winston-transport sigue siendo CJS, así que un require síncrono basta.
    const req = createRequire(import.meta.url);
    return req('winston-transport') as WinstonTransportCtor;
  } catch {
    throw new Error(
      '@iaportafolio/node/winston requiere `winston-transport` instalado como peer dep. ' +
        'Ejecuta: npm i winston winston-transport',
    );
  }
}

const Base = loadWinstonTransport();

/**
 * Custom `Transport` de winston que reenvía cada `info` a Faro.
 */
export class FaroTransport extends Base {
  private faroClient: ReturnType<typeof getClient>;

  constructor(opts: FaroTransportOptions) {
    // Reparto: opciones específicas de winston-transport hacia arriba; las nuestras se procesan abajo.
    const { level, silent, handleExceptions, handleRejections, ...rest } = opts as FaroTransportOptions & {
      [k: string]: unknown;
    };
    super({ level, silent, handleExceptions, handleRejections });

    if ('client' in opts && opts.client) {
      this.faroClient = opts.client;
    } else {
      init(rest as unknown as FaroOptions);
      this.faroClient = getClient();
    }
  }

  log(info: Record<string, unknown>, callback: () => void): void {
    // Notifica al pipeline de winston INMEDIATAMENTE para no bloquear su flujo.
    setImmediate(() => this.emit('logged', info));

    try {
      this.faroClient.log({
        level: mapWinstonLevel(info.level as string),
        message: typeof info.message === 'string' ? info.message : String(info.message ?? ''),
        attributes: extractAttrs(info),
      });
    } catch {
      // Best-effort: nunca tumbar al logger del usuario por un error nuestro.
    }
    callback();
  }
}
