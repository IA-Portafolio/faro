/**
 * Test del middleware Express — usa mocks de req/res para no requerir la dep
 * `express` en devDependencies. Valida:
 *   1. crea un span SERVER por request, con http.method/route/status_code
 *   2. respeta el traceparent entrante y propaga uno saliente al response
 *   3. status_code >= 500 → status=ERROR; <500 → status=OK
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';

const faro = await import('../dist/index.js');
const { expressTracer } = await import('../dist/express.js');

function startServer(handler) {
  return new Promise((resolve) => {
    const server = http.createServer((req, res) => {
      let body = '';
      req.on('data', (c) => (body += c));
      req.on('end', () => handler(req, res, body));
    });
    server.listen(0, () => resolve({ server, port: server.address().port }));
  });
}

/** Mock minimal de req/res estilo Express (sin EventEmitter completo). */
function mockReq({ method = 'GET', url = '/foo', headers = {} } = {}) {
  return {
    method, url, originalUrl: url, path: url,
    route: { path: url },
    ip: '1.2.3.4', socket: { remoteAddress: '1.2.3.4' },
    headers,
  };
}
function mockRes(statusCode = 200) {
  const listeners = new Map();
  return {
    statusCode,
    headers: {},
    setHeader(k, v) { this.headers[k.toLowerCase()] = v; },
    on(ev, cb) {
      if (!listeners.has(ev)) listeners.set(ev, []);
      listeners.get(ev).push(cb);
    },
    _emit(ev) { (listeners.get(ev) || []).forEach((cb) => cb()); },
  };
}

test('expressTracer: crea span SERVER con http.* attrs y status OK', async () => {
  let captured = null;
  const { server, port } = await startServer((_req, res, body) => {
    captured = JSON.parse(body);
    res.writeHead(200); res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`, token: 'tk', service: 'web',
    installGlobalHandlers: false, flushIntervalMs: 100_000, diag: () => {},
  });
  try {
    const mw = expressTracer();
    const req = mockReq({ method: 'POST', url: '/checkout' });
    const res = mockRes(201);
    await new Promise((resolve) => {
      mw(req, res, () => {
        // handler "ejecuta" — emitimos finish desde aquí
        res._emit('finish');
        resolve();
      });
    });
    // Pequeña espera para que el withSpan resuelva via res.on('finish'/'close')
    await new Promise((r) => setTimeout(r, 10));
    await c.flush();

    const sp = captured.resourceSpans[0].scopeSpans[0].spans[0];
    assert.equal(sp.kind, 2, 'SERVER');
    assert.equal(sp.name, 'POST /checkout');
    const method = sp.attributes.find((a) => a.key === 'http.method').value.stringValue;
    const route = sp.attributes.find((a) => a.key === 'http.route').value.stringValue;
    const status = sp.attributes.find((a) => a.key === 'http.status_code').value.stringValue;
    assert.equal(method, 'POST');
    assert.equal(route, '/checkout');
    assert.equal(status, '201');
    assert.equal(sp.status.code, 1, 'OK');
    // traceparent propagado al response
    assert.match(res.headers['traceparent'], /^00-[0-9a-f]{32}-[0-9a-f]{16}-01$/);
  } finally {
    await c.close(500);
    server.close();
  }
});

test('expressTracer: traceparent entrante setea parent', async () => {
  let captured = null;
  const { server, port } = await startServer((_req, res, body) => {
    captured = JSON.parse(body);
    res.writeHead(200); res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`, token: 'tk', service: 'web',
    installGlobalHandlers: false, flushIntervalMs: 100_000, diag: () => {},
  });
  try {
    const incomingTrace = 'a'.repeat(32);
    const incomingSpan = 'b'.repeat(16);
    const req = mockReq({
      headers: { 'traceparent': `00-${incomingTrace}-${incomingSpan}-01` },
    });
    const res = mockRes(200);
    await new Promise((resolve) => {
      expressTracer()(req, res, () => { res._emit('finish'); resolve(); });
    });
    await new Promise((r) => setTimeout(r, 10));
    await c.flush();

    const sp = captured.resourceSpans[0].scopeSpans[0].spans[0];
    assert.equal(sp.traceId, incomingTrace);
    assert.equal(sp.parentSpanId, incomingSpan);
  } finally {
    await c.close(500);
    server.close();
  }
});

test('expressTracer: status >=500 → span status ERROR', async () => {
  let captured = null;
  const { server, port } = await startServer((_req, res, body) => {
    captured = JSON.parse(body);
    res.writeHead(200); res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`, token: 'tk', service: 'web',
    installGlobalHandlers: false, flushIntervalMs: 100_000, diag: () => {},
  });
  try {
    const req = mockReq();
    const res = mockRes(503);
    await new Promise((resolve) => {
      expressTracer()(req, res, () => { res._emit('finish'); resolve(); });
    });
    await new Promise((r) => setTimeout(r, 10));
    await c.flush();
    const sp = captured.resourceSpans[0].scopeSpans[0].spans[0];
    assert.equal(sp.status.code, 2, 'ERROR');
  } finally {
    await c.close(500);
    server.close();
  }
});
