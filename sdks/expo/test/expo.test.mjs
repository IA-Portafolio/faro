/**
 * Tests unitarios del SDK Expo — 4 invariantes NUEVAS (complementan client.test.mjs):
 *   1. Init con opts inválidas → Error claro
 *   2. log + flush + assert payload (shape del wire)
 *   5. captureException compone shape OTel (auto-captura lo invoca igual)
 *   6. close() drena la cola antes de devolver
 *
 * Las invariantes #3 (queue cap), #4 (retry on 5xx), beforeSend y scrubbing
 * ya viven en `client.test.mjs` — no las duplicamos aquí.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { createRequire } from 'node:module';

// El bundle Expo es CJS (main: dist/index.js). createRequire deja que el
// `require()` interno del SDK funcione (Metro lo expone en producción).
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
    service: 'expo-extra-tests',
    installGlobalHandlers: false,
    persistence: false,
    flushIntervalMs: 100_000,
  };
}

// ---- 1. init con opts inválidas ----

test('init: endpoint vacío lanza Error claro', () => {
  assert.throws(
    () => faro.init({ endpoint: '', token: 'tk', service: 's', persistence: false }),
    /endpoint.*obligatorio/i,
  );
});

test('init: token ausente lanza Error claro', () => {
  assert.throws(
    () => faro.init({ endpoint: 'http://x', service: 's', persistence: false }),
    /token.*obligatorio/i,
  );
});

test('init: service ausente lanza Error claro', () => {
  assert.throws(
    () => faro.init({ endpoint: 'http://x', token: 'tk', persistence: false }),
    /service.*obligatorio/i,
  );
});

// ---- 2. log + flush + assert payload ----

test('payload: shape del JSON enviado al wire', async () => {
  const seen = [];
  const { server, port } = await startServer((req, res, body) => {
    seen.push({
      method: req.method,
      path: req.url,
      auth: req.headers['authorization'],
      contentType: req.headers['content-type'],
      body: JSON.parse(body),
    });
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    ...commonOpts(port),
    token: 'mi-token',
    service: 'payload-test',
    environment: 'prod',
    release: 'v1.2.3',
    attributes: { region: 'eu-west-1' },
  });
  try {
    c.log({
      level: 'WARN',
      message: 'algo raro',
      attributes: { 'http.status_code': 500, 'user.id': 'u42' },
    });
    await c.flush();
    await delay(50);

    assert.equal(seen.length, 1);
    const req = seen[0];
    assert.equal(req.method, 'POST');
    assert.equal(req.path, '/api/v1/ingest/logs');
    assert.equal(req.auth, 'Bearer mi-token');
    assert.equal(req.contentType, 'application/json');

    assert.equal(req.body.service, 'payload-test');
    assert.equal(req.body.logs.length, 1);
    const entry = req.body.logs[0];
    assert.equal(entry.level, 'WARN');
    assert.equal(entry.message, 'algo raro');
    assert.match(entry.timestamp, /^\d{4}-\d{2}-\d{2}T/);
    assert.equal(entry.attributes['region'], 'eu-west-1');
    assert.equal(entry.attributes['deployment.environment'], 'prod');
    assert.equal(entry.attributes['service.version'], 'v1.2.3');
    assert.equal(entry.attributes['http.status_code'], '500');
    assert.equal(entry.attributes['user.id'], 'u42');
  } finally {
    await c.close();
    server.close();
  }
});

// ---- 5. captureException compone shape OTel ----
//
// El handler que setGlobalHandler instala invoca captureException internamente.
// Si esto está bien, el flujo auto-disparado por ErrorUtils también lo está
// (el wrapper son 4 líneas). En Node no podemos disparar el global handler de
// RN — pero sí validar la lógica que ese handler ejecuta.

test('captureException compone exception.{type,message,stacktrace} + isFatal', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });
  const c = faro.init(commonOpts(port));
  try {
    let err;
    try { throw new TypeError('boom sintético'); } catch (e) { err = e; }
    c.captureException(err, { tags: { origin: 'test' }, isFatal: true });
    await c.flush();
    await delay(50);

    const entry = seen[0].logs[0];
    assert.equal(entry.level, 'ERROR');
    assert.match(entry.message, /boom sintético/);
    assert.equal(entry.attributes['exception.type'], 'TypeError');
    assert.equal(entry.attributes['exception.message'], 'boom sintético');
    assert.ok(entry.attributes['exception.stacktrace'].length > 0, 'stacktrace presente');
    assert.equal(entry.attributes['fatal'], 'true');
    assert.equal(entry.attributes['origin'], 'test');
  } finally {
    await c.close();
    server.close();
  }
});

// ---- 6. close() graceful ----

test('close: drena la cola antes de devolver — sin pérdida', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });
  const c = faro.init({
    ...commonOpts(port),
    // Si no fuera por close(), estos 7 eventos NO llegarían.
    flushIntervalMs: 100_000,
  });
  for (let i = 0; i < 7; i++) c.log({ level: 'INFO', message: `evento-${i}` });
  await c.close();

  const msgs = seen.flatMap((b) => b.logs.map((l) => l.message));
  assert.equal(msgs.length, 7, 'close() debe drenar los 7 eventos en cola');
  assert.deepEqual(
    msgs,
    ['evento-0','evento-1','evento-2','evento-3','evento-4','evento-5','evento-6'],
  );
  server.close();
});
