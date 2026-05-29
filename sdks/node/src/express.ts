/**
 * Middleware Express para crear un span por request automáticamente.
 *
 *   import express from 'express';
 *   import * as faro from '@iaportafolio/node';
 *   import { expressTracer } from '@iaportafolio/node/express';
 *
 *   faro.init({ endpoint, token, service });
 *   const app = express();
 *   app.use(expressTracer());
 *
 * El span hereda el W3C traceparent del request si está presente, y propaga al
 * response como `traceparent`. Logs emitidos dentro del handler se auto-correlacionan
 * con este span via AsyncLocalStorage.
 */

import { getClient, parseTraceparent, type Span, type SpanOptions } from './index.js';

type ReqLike = {
  method: string;
  url?: string;
  originalUrl?: string;
  path?: string;
  route?: { path?: string };
  ip?: string;
  socket?: { remoteAddress?: string };
  headers: Record<string, string | string[] | undefined>;
};
type ResLike = {
  statusCode: number;
  setHeader: (name: string, value: string) => void;
  on: (event: string, cb: () => void) => void;
};
type NextLike = (err?: unknown) => void;

export interface ExpressTracerOptions {
  /** Override del nombre del span. Default: `<METHOD> <route>` o el path. */
  spanName?: (req: ReqLike) => string;
  /** Si `false`, no inyecta el traceparent del span en la respuesta. Default `true`. */
  propagateResponse?: boolean;
}

export function expressTracer(opts: ExpressTracerOptions = {}): (req: ReqLike, res: ResLike, next: NextLike) => void {
  const propagateResponse = opts.propagateResponse ?? true;
  return function faroExpressTracerMiddleware(req, res, next) {
    let client;
    try {
      client = getClient();
    } catch {
      // Faro no inicializado — no rompemos el handler.
      return next();
    }

    const tp = headerFirst(req.headers['traceparent']);
    const parent = tp ? parseTraceparent(tp) : null;
    const route = req.route?.path || req.path || req.originalUrl || req.url || '';
    const spanName = opts.spanName ? opts.spanName(req) : `${req.method} ${route}`.trim();

    const spanOpts: SpanOptions = {
      kind: 'SERVER',
      attributes: {
        'http.method': req.method,
        'http.target': req.originalUrl || req.url || '',
        'http.route': route,
        'net.peer.ip': req.ip || req.socket?.remoteAddress || '',
      },
    };
    if (parent) spanOpts.parent = parent;

    // Usamos withSpan para que AsyncLocalStorage active el span dentro del handler
    // y los logs/llamadas anidadas lo hereden.
    void client.withSpan(spanName, async (span: Span) => {
      // (spanOpts pasado abajo como 3er arg — incluye kind, attrs, parent)
      if (propagateResponse) res.setHeader('traceparent', span.traceparent());
      res.on('finish', () => {
        span.setAttribute('http.status_code', res.statusCode);
        if (res.statusCode >= 500) span.setStatus('ERROR', `HTTP ${res.statusCode}`);
        else if (res.statusCode >= 400) span.setStatus('OK'); // 4xx no es span error (cliente)
        else span.setStatus('OK');
      });
      await new Promise<void>((resolve) => {
        res.on('close', resolve);
        res.on('finish', resolve);
        next();
      });
    }, spanOpts);
  };
}

function headerFirst(h: string | string[] | undefined): string | undefined {
  if (Array.isArray(h)) return h[0];
  return h;
}
