/**
 * Tests del SDK Node — API de métricas (counter / gauge / histogram).
 *
 * Cubre:
 *   1. counter/upDownCounter/gauge encolan el wire correcto
 *   2. histogram encola count=1 sum=min=max=value (sin agregación cliente-side)
 *   3. flush() postea el batch a /api/v1/ingest/metrics con shape correcto
 *   4. respuesta 5xx → re-encola para reintentar
 *   5. valores no finitos (NaN, Infinity) se descartan silenciosamente
 *   6. atributos por defecto (environment, release, opts.attributes) se incluyen
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

// ---- 1. counter / upDownCounter / gauge encolan el wire correcto ----

test('counter/upDownCounter/gauge encolan con el kind y value correctos', async () => {
  const c = faro.init({
    endpoint: 'http://127.0.0.1:1',
    token: 'tk',
    service: 'metrics-shape-test',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    enableTracing: false,
    diag: () => {},
  });
  try {
    c.counter('http.requests.total', { unit: '1' }).add(3, { route: '/api/foo' });
    c.upDownCounter('queue.depth').add(-1, { worker: 'a' });
    c.gauge('memory.rss_bytes', { unit: 'By' }).set(1024);

    assert.equal(c.metricsQueue.length, 3);

    const [cnt, ud, g] = c.metricsQueue;

    assert.equal(cnt.name, 'http.requests.total');
    assert.equal(cnt.kind, 'counter');
    assert.equal(cnt.unit, '1');
    assert.equal(cnt.value, 3);
    assert.equal(cnt.attributes.route, '/api/foo');
    assert.equal(cnt.service, 'metrics-shape-test');
    assert.match(cnt.timestamp, /^\d{4}-\d{2}-\d{2}T/);

    assert.equal(ud.kind, 'sum');
    assert.equal(ud.value, -1);

    assert.equal(g.kind, 'gauge');
    assert.equal(g.unit, 'By');
    assert.equal(g.value, 1024);
  } finally {
    await c.close(50);
  }
});

// ---- 2. histogram encola count=1 sum=min=max=value ----

test('histogram: un data point por record(), sin agregación cliente-side', async () => {
  const c = faro.init({
    endpoint: 'http://127.0.0.1:1',
    token: 'tk',
    service: 'hist-test',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    enableTracing: false,
    diag: () => {},
  });
  try {
    const h = c.histogram('http.request.duration_ms', { unit: 'ms' });
    h.record(50);
    h.record(123, { route: '/api/foo' });

    assert.equal(c.metricsQueue.length, 2);
    const [a, b] = c.metricsQueue;

    assert.equal(a.kind, 'histogram');
    assert.equal(a.unit, 'ms');
    assert.equal(a.count, 1);
    assert.equal(a.sum, 50);
    assert.equal(a.min, 50);
    assert.equal(a.max, 50);
    // En histogramas, `value` NO se envía — el backend usa hist_sum.
    assert.equal(a.value, undefined);

    assert.equal(b.count, 1);
    assert.equal(b.sum, 123);
    assert.equal(b.attributes.route, '/api/foo');
  } finally {
    await c.close(50);
  }
});

// ---- 3. flush() postea el batch al endpoint correcto ----

test('flush(): postea a /api/v1/ingest/metrics con shape esperado', async () => {
  let received;
  const { server, port } = await startServer((req, res, body) => {
    received = { url: req.url, auth: req.headers['authorization'], body: JSON.parse(body) };
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end('{"accepted":1}');
  });
  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk-secret',
    service: 'flush-test',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    enableTracing: false,
    diag: () => {},
  });
  try {
    c.counter('hits').add(5, { ok: true });
    await c.flush();

    assert.equal(received.url, '/api/v1/ingest/metrics');
    assert.equal(received.auth, 'Bearer tk-secret');
    assert.equal(received.body.service, 'flush-test');
    assert.equal(received.body.metrics.length, 1);
    assert.equal(received.body.metrics[0].name, 'hits');
    assert.equal(received.body.metrics[0].kind, 'counter');
    assert.equal(received.body.metrics[0].value, 5);
    assert.equal(received.body.metrics[0].attributes.ok, 'true');
    assert.equal(c.metricsQueue.length, 0);
  } finally {
    await c.close(200);
    server.close();
  }
});

// ---- 4. 5xx re-encola ----

test('5xx: el batch de métricas se re-encola para reintentar', async () => {
  let calls = 0;
  const { server, port } = await startServer((_req, res) => {
    calls += 1;
    if (calls === 1) {
      res.writeHead(503);
      res.end('upstream caído');
    } else {
      res.writeHead(200);
      res.end('{"accepted":1}');
    }
  });
  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'retry-test',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    enableTracing: false,
    diag: () => {},
  });
  try {
    c.counter('hits').add(1);
    await c.flush(); // 503 → re-encola
    assert.equal(c.metricsQueue.length, 1, 'tras 503 el batch vuelve a la cola');

    await c.flush(); // 200 → drena
    assert.equal(c.metricsQueue.length, 0);
    assert.equal(calls, 2);
  } finally {
    await c.close(200);
    server.close();
  }
});

// ---- 5. valores no finitos se descartan ----

test('NaN / Infinity / nombre vacío se descartan sin tirar', () => {
  const c = faro.init({
    endpoint: 'http://127.0.0.1:1',
    token: 'tk',
    service: 'invalid-vals',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    enableTracing: false,
    diag: () => {},
  });
  try {
    c.counter('ok').add(NaN);
    c.gauge('g').set(Infinity);
    c.gauge('g').set(-Infinity);
    c.counter('').add(1);
    assert.equal(c.metricsQueue.length, 0);

    c.counter('ok').add(7);
    assert.equal(c.metricsQueue.length, 1);
  } finally {
    c.close(50);
  }
});

// ---- 6. atributos por defecto se incluyen ----

test('environment + release + opts.attributes se mezclan en cada métrica', () => {
  const c = faro.init({
    endpoint: 'http://127.0.0.1:1',
    token: 'tk',
    service: 'attrs-test',
    environment: 'staging',
    release: 'v1.2.3',
    attributes: { region: 'us-east-1' },
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    enableTracing: false,
    diag: () => {},
  });
  try {
    c.counter('hits').add(1, { custom: 'x' });
    assert.equal(c.metricsQueue.length, 1);
    const wire = c.metricsQueue[0];
    assert.equal(wire.attributes['deployment.environment'], 'staging');
    assert.equal(wire.attributes['service.version'], 'v1.2.3');
    assert.equal(wire.attributes['region'], 'us-east-1');
    assert.equal(wire.attributes['custom'], 'x');
  } finally {
    c.close(50);
  }
});
