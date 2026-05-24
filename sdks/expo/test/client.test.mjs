/**
 * Tests unitarios del SDK Expo — 4 invariantes:
 *   1. queue cap
 *   2. retry on 5xx
 *   3. beforeSend filtra
 *   4. scrubbing
 *
 * No requiere react-native ni AsyncStorage: pasamos `installGlobalHandlers: false`
 * y `persistence: false` para evitar los `require('react-native')` /
 * `require('@react-native-async-storage/async-storage')`.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { createRequire } from 'node:module';

// El bundle Expo es CJS (main: dist/index.js). Cargamos con createRequire para
// que el `require()` interno del SDK funcione (Metro lo expone en producción).
const faro = createRequire(import.meta.url)('../dist/index.js');

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

const delay = (ms) => new Promise((r) => setTimeout(r, ms));

function commonOpts(port) {
  return {
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'expo-tests',
    installGlobalHandlers: false,
    persistence: false,
    flushIntervalMs: 100_000,
  };
}

// ---- 1. queue cap ----

test('queue cap: enqueue silencioso al límite', async () => {
  const c = faro.init({ ...commonOpts(1), maxQueueSize: 5 });
  try {
    for (let i = 0; i < 50; i++) c.log({ level: 'INFO', message: `e${i}` });
    assert.equal(c.queue.length, 5, 'la cola debe pararse en el cap');
  } finally {
    await c.close();
  }
});

// ---- 2. retry sobre 5xx ----

test('5xx: el batch se re-encola', async () => {
  let calls = 0;
  const { server, port } = await startServer((_req, res) => {
    calls += 1;
    if (calls === 1) {
      res.writeHead(503);
      res.end('caído');
    } else {
      res.writeHead(200);
      res.end('{}');
    }
  });

  const c = faro.init(commonOpts(port));
  try {
    c.log({ level: 'INFO', message: 'reintentar-me' });
    await c.flush(); // 503 → re-encola
    await c.flush(); // ahora OK
    await delay(50);
    assert.ok(calls >= 2, 'segundo intento debe haber llegado');
  } finally {
    await c.close();
    server.close();
  }
});

// ---- 3. beforeSend ----

test('beforeSend: null descarta', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    ...commonOpts(port),
    beforeSend: (e) => (e.message.includes('descartar') ? null : e),
  });
  try {
    c.log({ level: 'INFO', message: 'guardar' });
    c.log({ level: 'INFO', message: 'descartar' });
    c.log({ level: 'INFO', message: 'guardar también' });
    await c.flush();
    await delay(50);
    const msgs = seen[0].logs.map((l) => l.message);
    assert.deepEqual(msgs, ['guardar', 'guardar también']);
  } finally {
    await c.close();
    server.close();
  }
});

// ---- 4. scrubbing ----

test('scrubbing: scrubFields + scrubPatterns', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init(commonOpts(port));
  try {
    c.log({
      level: 'INFO',
      message: 'auth con eyJabc.def.ghi y key sk-abcdefghijklmnop',
      attributes: {
        'user.password': 'p4ss',
        'http.request.header.authorization': 'Bearer x',
        'safe.field': 'visible',
        'embedded': 'ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      },
    });
    await c.flush();
    await delay(50);
    const log = seen[0].logs[0];
    assert.equal(log.attributes['user.password'], '[REDACTED]');
    assert.equal(log.attributes['http.request.header.authorization'], '[REDACTED]');
    assert.equal(log.attributes['safe.field'], 'visible');
    assert.equal(log.attributes['embedded'], '[REDACTED]');
    assert.ok(!log.message.includes('eyJabc'), 'JWT redactado en message');
    assert.ok(!log.message.includes('sk-abcdef'), 'sk-* redactado en message');
  } finally {
    await c.close();
    server.close();
  }
});

// ---- Product events API: track / identify / screen / alias ----

test('track: envía evento mobile a /api/v1/ingest/events', async () => {
  const seen = [];
  const { server, port } = await startServer((req, res, body) => {
    seen.push({ url: req.url, body: JSON.parse(body) });
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init(commonOpts(port));
  try {
    c.track('checkout_completed', { amount: 99.5, currency: 'USD' });
    await c.flush();
    await delay(50);

    const eventBatch = seen.find((s) => s.url.endsWith('/api/v1/ingest/events'));
    assert.ok(eventBatch, `esperaba POST a /events; visto: ${JSON.stringify(seen.map((s) => s.url))}`);
    assert.equal(eventBatch.body.service, 'expo-tests');
    const event = eventBatch.body.events[0];
    assert.equal(event.type, 'track');
    assert.equal(event.name, 'checkout_completed');
    assert.deepEqual(event.properties, { amount: 99.5, currency: 'USD' });
    assert.ok(event.distinct_id.startsWith('anon_'));
    assert.equal(event.distinct_id, event.anonymous_id);
    assert.equal(event.session_id, '');
    assert.equal(event.source, 'mobile');
  } finally {
    await c.close();
    server.close();
  }
});

test('identify: fija distinct_id para eventos siguientes', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init(commonOpts(port));
  try {
    c.identify('user_42', { email: 'a@b.com', plan: 'pro' });
    c.track('after_login');
    await c.flush();
    await delay(50);

    const events = seen.flatMap((b) => b.events ?? []);
    const identify = events.find((e) => e.type === 'identify');
    const track = events.find((e) => e.type === 'track');
    assert.ok(identify, 'debe llegar un identify');
    assert.equal(identify.name, '$identify');
    assert.equal(identify.distinct_id, 'user_42');
    assert.deepEqual(identify.user_properties, { email: 'a@b.com', plan: 'pro' });
    assert.ok(track, 'debe llegar el track posterior');
    assert.equal(track.distinct_id, 'user_42');
  } finally {
    await c.close();
    server.close();
  }
});

test('screen: emite screen view mobile con propiedades', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init(commonOpts(port));
  try {
    c.screen('CheckoutSuccess', { source: 'cart' });
    await c.flush();
    await delay(50);

    const event = seen.flatMap((b) => b.events ?? []).find((e) => e.type === 'screen');
    assert.ok(event, 'debe llegar un screen event');
    assert.equal(event.name, 'CheckoutSuccess');
    assert.deepEqual(event.properties, { source: 'cart' });
    assert.equal(event.source, 'mobile');
  } finally {
    await c.close();
    server.close();
  }
});

test('alias: fusiona anonymous_id previo con user post-login', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init(commonOpts(port));
  try {
    c.alias('anonymous_abc123', 'user_42');
    c.track('post_alias');
    await c.flush();
    await delay(50);

    const events = seen.flatMap((b) => b.events ?? []);
    const alias = events.find((e) => e.type === 'alias');
    const track = events.find((e) => e.type === 'track');
    assert.ok(alias, 'debe llegar un alias');
    assert.equal(alias.name, '$alias');
    assert.equal(alias.anonymous_id, 'anonymous_abc123');
    assert.equal(alias.distinct_id, 'user_42');
    assert.equal(track.distinct_id, 'user_42');
  } finally {
    await c.close();
    server.close();
  }
});
