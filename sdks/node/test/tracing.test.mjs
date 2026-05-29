/**
 * Tests del SDK de tracing — invariantes públicas (v0.2.0):
 *   1. startSpan / end → flush manda OTLP/JSON a /v1/traces con service.name correcto
 *   2. parent context explícito: hijo hereda trace_id y setea parentSpanId
 *   3. withSpan: cierra solo y propaga ERROR si fn lanza
 *   4. Context manager: spans anidados sin pasar context (auto-parent)
 *   5. logs dentro de withSpan auto-heredan trace_id/span_id
 *   6. traceparent() formato W3C
 *   7. recordException popula attributes y status=ERROR
 *
 * NOTA: en v0.2.0 el tracing está respaldado por @opentelemetry/sdk-trace-node.
 * El BatchSpanProcessor se drena via faro.flush() (que llama a forceFlush()).
 * No miramos la cola interna — el proceso de export es opaque.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';

const faro = await import('../dist/index.js');

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

/** Acumula los bodies recibidos en /v1/traces para inspección. */
function makeTraceCollector() {
  const captured = [];
  let lastPath = null;
  const handler = (req, res, body) => {
    lastPath = req.url;
    if (req.url === '/v1/traces' && body) {
      try {
        captured.push(JSON.parse(body));
      } catch {
        // ignore
      }
    }
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end('{"partialSuccess":{}}');
  };
  return {
    handler,
    captured,
    lastPath: () => lastPath,
    /** Devuelve todos los spans recibidos planos. */
    allSpans: () => captured.flatMap((req) =>
      (req.resourceSpans ?? []).flatMap((rs) =>
        (rs.scopeSpans ?? []).flatMap((ss) => ss.spans ?? []),
      ),
    ),
    /** Devuelve los resource attributes del primer batch. */
    firstResourceAttrs: () => captured[0]?.resourceSpans?.[0]?.resource?.attributes ?? [],
  };
}

function getStringAttr(attrs, key) {
  const a = attrs?.find((x) => x.key === key);
  return a?.value?.stringValue ?? a?.value?.intValue ?? a?.value?.boolValue;
}

test('startSpan: emite OTLP/JSON a /v1/traces con service.name correcto', async () => {
  const c = makeTraceCollector();
  const { server, port } = await startServer(c.handler);

  const f = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk', service: 'trace-test',
    installGlobalHandlers: false, flushIntervalMs: 100_000,
    environment: 'prod', release: '1.2.3',
    diag: () => {},
  });
  try {
    const span = f.startSpan('checkout', {
      kind: 'SERVER',
      attributes: { 'http.method': 'POST' },
    });
    span.setAttribute('user.id', 42);
    span.addEvent('cache.miss', { attributes: { key: 'abc' } });
    span.setStatus('OK');
    span.end();
    await f.flush();

    assert.equal(c.lastPath(), '/v1/traces', 'pega a /v1/traces');
    const resAttrs = c.firstResourceAttrs();
    assert.equal(getStringAttr(resAttrs, 'service.name'), 'trace-test');
    assert.equal(getStringAttr(resAttrs, 'service.version'), '1.2.3');
    assert.ok(
      getStringAttr(resAttrs, 'deployment.environment') === 'prod' ||
      getStringAttr(resAttrs, 'deployment.environment.name') === 'prod',
      'environment presente en el resource',
    );

    const spans = c.allSpans();
    const sp = spans.find((s) => s.name === 'checkout');
    assert.ok(sp, 'span "checkout" presente');
    assert.equal(sp.kind, 2, 'SERVER=2');
    assert.match(sp.traceId, /^[0-9a-f]{32}$/);
    assert.match(sp.spanId, /^[0-9a-f]{16}$/);
    assert.ok(!sp.parentSpanId || sp.parentSpanId === '', 'root span no debe tener parentSpanId');
    assert.equal(sp.status?.code, 1, 'OK=1');
    assert.equal(getStringAttr(sp.attributes, 'user.id'), '42');
    assert.equal(sp.events?.[0]?.name, 'cache.miss');
  } finally {
    await f.close(2000);
    server.close();
  }
});

test('parent context explícito: hijo hereda trace_id y setea parentSpanId', async () => {
  const c = makeTraceCollector();
  const { server, port } = await startServer(c.handler);

  const f = faro.init({
    endpoint: `http://127.0.0.1:${port}`, token: 'tk', service: 't-parent',
    installGlobalHandlers: false, flushIntervalMs: 100_000, diag: () => {},
  });
  try {
    const parent = f.startSpan('parent');
    const parentCtx = parent.spanContext();
    const child = f.startSpan('child', { parent: parentCtx });
    child.end();
    parent.end();
    await f.flush();

    const spans = c.allSpans();
    const parentSp = spans.find((s) => s.name === 'parent');
    const childSp = spans.find((s) => s.name === 'child');
    assert.ok(parentSp && childSp);
    assert.equal(childSp.traceId, parentSp.traceId, 'hijo hereda trace_id');
    assert.equal(childSp.parentSpanId, parentSp.spanId);
  } finally {
    await f.close(2000);
    server.close();
  }
});

test('withSpan: cierra solo y propaga ERROR si fn lanza', async () => {
  const c = makeTraceCollector();
  const { server, port } = await startServer(c.handler);

  const f = faro.init({
    endpoint: `http://127.0.0.1:${port}`, token: 'tk', service: 't-throws',
    installGlobalHandlers: false, flushIntervalMs: 100_000, diag: () => {},
  });
  try {
    await assert.rejects(
      f.withSpan('boom', async () => { throw new Error('kaboom'); }),
      /kaboom/,
    );
    await f.flush();
    const sp = c.allSpans().find((s) => s.name === 'boom');
    assert.ok(sp);
    assert.equal(sp.status?.code, 2, 'ERROR=2');
    assert.ok(getStringAttr(sp.attributes, 'exception.type'));
    assert.ok(getStringAttr(sp.attributes, 'exception.message'));
  } finally {
    await f.close(2000);
    server.close();
  }
});

test('Context manager: spans anidados sin pasar context (auto-parent)', async () => {
  const c = makeTraceCollector();
  const { server, port } = await startServer(c.handler);

  const f = faro.init({
    endpoint: `http://127.0.0.1:${port}`, token: 'tk', service: 't-nested',
    installGlobalHandlers: false, flushIntervalMs: 100_000, diag: () => {},
  });
  try {
    await f.withSpan('outer', async () => {
      await f.withSpan('inner', async () => {
        const active = f.activeSpan();
        assert.ok(active, 'activeSpan no debe ser null dentro de withSpan');
      });
    });
    await f.flush();
    const spans = c.allSpans();
    const outer = spans.find((s) => s.name === 'outer');
    const inner = spans.find((s) => s.name === 'inner');
    assert.ok(outer && inner);
    assert.equal(inner.traceId, outer.traceId);
    assert.equal(inner.parentSpanId, outer.spanId);
  } finally {
    await f.close(2000);
    server.close();
  }
});

test('logs dentro de withSpan auto-heredan trace_id/span_id', async () => {
  const c = makeTraceCollector();
  let logsBody = null;
  const { server, port } = await startServer((req, res, body) => {
    if (req.url === '/api/v1/ingest/logs') {
      logsBody = JSON.parse(body);
      res.writeHead(200); res.end('{}');
      return;
    }
    c.handler(req, res, body);
  });

  const f = faro.init({
    endpoint: `http://127.0.0.1:${port}`, token: 'tk', service: 't-corr',
    installGlobalHandlers: false, flushIntervalMs: 100_000, diag: () => {},
  });
  try {
    let spanCtx;
    await f.withSpan('handler', async (span) => {
      spanCtx = span.spanContext();
      f.info('procesando request', { foo: 'bar' });
    });
    await f.flush();
    const log = logsBody?.logs?.[0];
    assert.ok(log, 'log presente');
    assert.equal(log.trace_id, spanCtx.trace_id, 'trace_id hereda del span activo');
    assert.equal(log.span_id, spanCtx.span_id, 'span_id hereda del span activo');
  } finally {
    await f.close(2000);
    server.close();
  }
});

test('traceparent(): formato W3C válido', async () => {
  // Sin red, sólo verificamos el formato del header.
  const f = faro.init({
    endpoint: 'http://127.0.0.1:1', token: 'tk', service: 't-tp',
    installGlobalHandlers: false, flushIntervalMs: 100_000,
    enableTracing: true, diag: () => {},
  });
  try {
    const span = f.startSpan('x');
    const tp = span.traceparent();
    assert.match(tp, /^00-[0-9a-f]{32}-[0-9a-f]{16}-(00|01)$/);
    span.end();
  } finally {
    await f.close(50);
  }
});

test('recordException: setea ERROR + exception.* attrs sin re-lanzar', async () => {
  const c = makeTraceCollector();
  const { server, port } = await startServer(c.handler);

  const f = faro.init({
    endpoint: `http://127.0.0.1:${port}`, token: 'tk', service: 't-exc',
    installGlobalHandlers: false, flushIntervalMs: 100_000, diag: () => {},
  });
  try {
    const span = f.startSpan('op');
    span.recordException(new TypeError('bad input'));
    span.end();
    await f.flush();
    const sp = c.allSpans().find((s) => s.name === 'op');
    assert.ok(sp);
    assert.equal(sp.status?.code, 2);
    assert.equal(getStringAttr(sp.attributes, 'exception.type'), 'TypeError');
    assert.equal(getStringAttr(sp.attributes, 'exception.message'), 'bad input');
  } finally {
    await f.close(2000);
    server.close();
  }
});

test('enableTracing:false omite la inicialización OTel', async () => {
  // Spans creados quedan no-op (traceId=000...000). El test verifica que init no rompe.
  const f = faro.init({
    endpoint: 'http://127.0.0.1:1', token: 'tk', service: 't-disabled',
    installGlobalHandlers: false, flushIntervalMs: 100_000,
    enableTracing: false, diag: () => {},
  });
  try {
    const span = f.startSpan('x');
    const sc = span.spanContext();
    // Tracer no-op del global → traceId todo ceros. Lo importante: no rompe.
    assert.match(sc.trace_id, /^[0-9a-f]{32}$/);
    span.end();
  } finally {
    await f.close(50);
  }
});
