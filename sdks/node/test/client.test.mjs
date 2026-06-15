/**
 * Tests unitarios del SDK Node — 4 invariantes mínimas:
 *   1. queue cap descarta cuando se llena
 *   2. fallo de red / 5xx re-encola para reintentar
 *   3. beforeSend filtra (null → descartar) y transforma
 *   4. scrubbing aplica scrubFields + scrubPatterns
 *
 * Runner: node --test (built-in, sin dependencias). Cada test levanta un
 * servidor HTTP local que captura batches, así no tocamos la red de verdad.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';

// El build (dist/) se construye en CI antes de los tests via `npm run build`.
const faro = await import('../dist/index.js');

// ---- Helpers ----

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

function delay(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

// ---- 1. queue cap ----

test('queue cap: descarta nuevos eventos cuando se llena', async () => {
  // Apuntamos a un puerto vacío → fetch falla instantáneamente con ECONNREFUSED.
  // Esto nos permite verificar el cap sin riesgo de cuelgues por servers lentos.
  const c = faro.init({
    endpoint: 'http://127.0.0.1:1', // puerto 1 reservado, fetch falla rápido
    token: 'tk',
    service: 'queue-cap-test',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,       // no auto-flush durante el test
    maxQueueSize: 5,
    diag: () => {},                 // silencia los "cola llena" en stderr
  });
  try {
    for (let i = 0; i < 10; i++) c.log({ level: 'INFO', message: `evento ${i}` });
    // Acceso al estado interno: `queue` (detalle de implementación pero estabiliza
    // el test sin tener que depender de un servidor).
    assert.equal(c.queue.length, 5, 'la cola debe pararse en el cap');
  } finally {
    await c.close(50); // cierre rápido; el endpoint es inalcanzable
  }
});

// ---- 2. retry sobre 5xx ----

test('5xx: el batch se re-encola para reintentar', async () => {
  let calls = 0;
  const { server, port } = await startServer((_req, res, _body) => {
    calls += 1;
    if (calls === 1) {
      res.writeHead(503);
      res.end('upstream caído');
    } else {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end('{"ok":true}');
    }
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'retry-test',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    diag: () => {}, // silenciar warning interno del 503
  });
  try {
    c.log({ level: 'INFO', message: 'algo' });
    await c.flush(); // primer intento → 503
    // En el SDK actual el flush re-encola en `catch` (fallo de red), no en 4xx/5xx.
    // Aceptamos cualquier estrategia siempre que: o reintente (calls >= 2) o conserve
    // los datos en cola para el siguiente tick.
    if (calls === 1) {
      // No reintentó solo — comprobamos que al menos NO perdió el evento.
      assert.ok(c.queue.length >= 1, 'tras 5xx la cola no debe estar vacía');
      await c.flush();
    }
    assert.ok(calls >= 2, 'debe haber reintentado al menos una vez');
  } finally {
    await c.close(500);
    server.close();
  }
});

// ---- 3. beforeSend ----

test('beforeSend: devolver null descarta el evento', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'beforeSend-test',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    beforeSend: (e) => (e.message.includes('descarta-me') ? null : e),
  });
  try {
    c.log({ level: 'INFO', message: 'guarda-me' });
    c.log({ level: 'INFO', message: 'descarta-me' });
    c.log({ level: 'INFO', message: 'también guarda-me' });
    await c.flush();
    await delay(50);
    assert.equal(seen.length, 1, 'un único batch');
    const msgs = seen[0].logs.map((l) => l.message);
    assert.deepEqual(msgs, ['guarda-me', 'también guarda-me']);
  } finally {
    await c.close(500);
    server.close();
  }
});

test('beforeSend: puede transformar (mutar)', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'beforeSend-mutate',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    beforeSend: (e) => ({ ...e, attributes: { ...e.attributes, injected: 'yes' } }),
  });
  try {
    c.log({ level: 'INFO', message: 'hola' });
    await c.flush();
    await delay(50);
    assert.equal(seen[0].logs[0].attributes.injected, 'yes');
  } finally {
    await c.close(500);
    server.close();
  }
});

// ---- 4. scrubbing ----

test('scrubbing: scrubFields redacta valores por clave', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'scrub-fields',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
  });
  try {
    c.log({
      level: 'INFO',
      message: 'login',
      attributes: {
        'user.password': 'p4ssw0rd',
        'http.request.header.authorization': 'Bearer abc',
        'safe.field': 'visible',
      },
    });
    await c.flush();
    await delay(50);
    const attrs = seen[0].logs[0].attributes;
    assert.equal(attrs['user.password'], '[REDACTED]', 'password debe redactarse');
    assert.equal(attrs['http.request.header.authorization'], '[REDACTED]');
    assert.equal(attrs['safe.field'], 'visible', 'campo benigno intacto');
  } finally {
    await c.close(500);
    server.close();
  }
});

// ---- 5. init con opts inválidas ----

test('init: endpoint vacío lanza Error claro', () => {
  assert.throws(
    () => faro.init({ endpoint: '', token: 'tk', service: 's' }),
    /endpoint.*obligatorio/i,
  );
});

test('init: token ausente lanza Error claro', () => {
  assert.throws(
    // @ts-expect-error: faltando token a propósito
    () => faro.init({ endpoint: 'http://x', service: 's' }),
    /token.*obligatorio/i,
  );
});

test('init: service ausente lanza Error claro', () => {
  assert.throws(
    // @ts-expect-error: faltando service a propósito
    () => faro.init({ endpoint: 'http://x', token: 'tk' }),
    /service.*obligatorio/i,
  );
});

// ---- 6. log + flush + assert payload (shape del wire) ----

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
    endpoint: `http://127.0.0.1:${port}`,
    token: 'mi-token',
    service: 'payload-test',
    environment: 'prod',
    release: 'v1.2.3',
    attributes: { region: 'eu-west-1' },
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
  });
  try {
    c.log({
      level: 'WARN',
      message: 'algo raro',
      attributes: { 'http.status_code': 500, 'user.id': 'u42' },
    });
    await c.flush();
    await delay(50);

    assert.equal(seen.length, 1, 'un único POST');
    const req = seen[0];

    // Método + path + headers de transporte
    assert.equal(req.method, 'POST');
    assert.equal(req.path, '/api/v1/ingest/logs');
    assert.equal(req.auth, 'Bearer mi-token');
    assert.equal(req.contentType, 'application/json');

    // Envelope: { service, logs:[...] }
    assert.equal(req.body.service, 'payload-test');
    assert.ok(Array.isArray(req.body.logs));
    assert.equal(req.body.logs.length, 1);

    // Entry: nivel uppercase, atributos mergeados, timestamp ISO
    const entry = req.body.logs[0];
    assert.equal(entry.level, 'WARN');
    assert.equal(entry.message, 'algo raro');
    assert.match(entry.timestamp, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/);

    // Atributos: defaults del init + environment/release + attrs del log,
    // todos como strings (el wire serializa todo a string).
    assert.equal(entry.attributes['region'], 'eu-west-1');
    assert.equal(entry.attributes['deployment.environment'], 'prod');
    assert.equal(entry.attributes['service.version'], 'v1.2.3');
    assert.equal(entry.attributes['http.status_code'], '500'); // number → string
    assert.equal(entry.attributes['user.id'], 'u42');
  } finally {
    await c.close(500);
    server.close();
  }
});

// ---- 7. auto-captura de excepciones (uncaughtException → evento ERROR) ----

test('auto-captura: uncaughtException dispara captureException + flush', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  // Conservamos los listeners previos y los des-instalamos en el finally
  // para que un fallo en mitad del test no contamine el resto del runner.
  const prevUncaught = process.listeners('uncaughtException').slice();
  process.removeAllListeners('uncaughtException');

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'auto-capture-test',
    installGlobalHandlers: true, // <- core del test
    flushIntervalMs: 100_000,
    diag: () => {},
  });
  try {
    // Emitir manualmente en vez de throw real: throw real cae al runner y
    // marca el test como failed. process.emit ejecuta los listeners igual.
    process.emit('uncaughtException', new Error('¡crash sintético!'));

    // El handler hace `void this.flush()`. Esperamos al ciclo de event loop.
    await delay(150);
    // Si el primer flush no alcanzó a cerrar, un flush explícito lo termina.
    await c.flush();
    await delay(50);

    assert.ok(seen.length >= 1, 'al menos un POST al server tras uncaughtException');
    const entry = seen[0].logs[0];
    assert.equal(entry.level, 'ERROR');
    assert.match(entry.message, /crash sintético/);
    assert.equal(entry.attributes['exception.type'], 'Error');
    assert.equal(entry.attributes['exception.message'], '¡crash sintético!');
    assert.ok(
      typeof entry.attributes['exception.stacktrace'] === 'string',
      'stacktrace presente',
    );
  } finally {
    await c.close(500);
    server.close();
    process.removeAllListeners('uncaughtException');
    for (const l of prevUncaught) process.on('uncaughtException', l);
  }
});

// ---- 8. close() graceful: no pierde eventos en cola ----

test('close: drena la cola antes de devolver — sin pérdida', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'close-test',
    installGlobalHandlers: false,
    // Intervalo lejano para garantizar que NO es el timer quien drena —
    // el envío tiene que pasar dentro de close(), no por casualidad.
    flushIntervalMs: 100_000,
  });

  // 7 eventos > maxBatchSize default no se aplica porque default es 200.
  // Aquí lo que importa es: con flushIntervalMs gigante, sin close() esos
  // eventos NUNCA llegarían. Con close() sí.
  for (let i = 0; i < 7; i++) c.log({ level: 'INFO', message: `evento-${i}` });

  await c.close(2000);
  // Tras close, el server ya debe haber visto los 7.
  const all = seen.flatMap((b) => b.logs.map((l) => l.message));
  assert.equal(all.length, 7, 'close() debe drenar los 7 eventos en cola');
  assert.deepEqual(all, ['evento-0', 'evento-1', 'evento-2', 'evento-3', 'evento-4', 'evento-5', 'evento-6']);

  server.close();
});

test('scrubbing: scrubPatterns redacta JWTs y API keys en values y message', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'scrub-patterns',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
  });
  try {
    c.log({
      level: 'INFO',
      message: 'auth con eyJabc.def.ghi y key sk-abcdefghijklmnop',
      attributes: { embedded: 'ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' },
    });
    await c.flush();
    await delay(50);
    const log = seen[0].logs[0];
    assert.ok(!log.message.includes('eyJabc'), 'JWT en message debe estar redactado');
    assert.ok(!log.message.includes('sk-abcdef'), 'sk-* en message debe estar redactado');
    assert.equal(log.attributes.embedded, '[REDACTED]', 'ghp_* en attribute value redactado');
  } finally {
    await c.close(500);
    server.close();
  }
});

// ---- Product events API: track / identify / alias ----

test('track: envía evento a /api/v1/ingest/events con la shape correcta', async () => {
  const seen = [];
  const { server, port } = await startServer((req, res, body) => {
    seen.push({ url: req.url, body: JSON.parse(body) });
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'track-test',
    installGlobalHandlers: false,
    flushIntervalMs: 50,
  });
  try {
    c.track('checkout_completed', { amount: 99.5, currency: 'USD' });
    await c.flush();
    await delay(80);
    const eventBatches = seen.filter((s) => s.url.endsWith('/events'));
    assert.ok(eventBatches.length >= 1, `esperaba batch a /events; visto: ${JSON.stringify(seen.map((s) => s.url))}`);
    const e = eventBatches[0].body.events[0];
    assert.equal(e.type, 'track');
    assert.equal(e.name, 'checkout_completed');
    assert.deepEqual(e.properties, { amount: 99.5, currency: 'USD' });
    assert.ok(e.distinct_id.startsWith('anon_'), 'pre-identify distinct_id == anonymous_id');
    assert.equal(e.distinct_id, e.anonymous_id);
    assert.equal(e.source, 'backend');
  } finally {
    await c.close(500);
    server.close();
  }
});

test('track: adjunta trace_id y span_id desde traceContext', async () => {
  const seen = [];
  const { server, port } = await startServer((req, res, body) => {
    seen.push({ url: req.url, body: JSON.parse(body) });
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'trace-context-test',
    installGlobalHandlers: false,
    flushIntervalMs: 50,
    traceContext: () => '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01',
  });
  try {
    c.track('checkout_completed');
    await c.flush();
    await delay(80);
    const eventBatches = seen.filter((s) => s.url.endsWith('/events'));
    assert.ok(eventBatches.length >= 1, 'esperaba batch a /events');
    const e = eventBatches[0].body.events[0];
    assert.equal(e.trace_id, '4bf92f3577b34da6a3ce929d0e0e4736');
    assert.equal(e.span_id, '00f067aa0ba902b7');
  } finally {
    await c.close(500);
    server.close();
  }
});

test('identify: setea distinct_id para los eventos siguientes', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'identify-test',
    installGlobalHandlers: false,
    flushIntervalMs: 50,
  });
  try {
    c.identify('user_42', { email: 'a@b.com', plan: 'pro' });
    c.track('after_login');
    await c.close(2000);

    const events = seen.flatMap((b) => b.events ?? []);
    const ident = events.find((e) => e.type === 'identify');
    const trk = events.find((e) => e.type === 'track');
    assert.ok(ident, 'debe haber un evento identify');
    assert.equal(ident.distinct_id, 'user_42');
    assert.deepEqual(ident.user_properties, { email: 'a@b.com', plan: 'pro' });
    assert.ok(trk, 'el track tras identify debe llegar');
    assert.equal(trk.distinct_id, 'user_42', 'tras identify, distinct_id queda fijado');
  } finally {
    server.close();
  }
});

test('alias: fusiona sesión pre-login con user post-login', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'alias-test',
    installGlobalHandlers: false,
    flushIntervalMs: 50,
  });
  try {
    c.alias('anon_old', 'user_99');
    c.track('post_alias');
    await c.close(2000);

    const events = seen.flatMap((b) => b.events ?? []);
    const ali = events.find((e) => e.type === 'alias');
    const trk = events.find((e) => e.type === 'track');
    assert.equal(ali.anonymous_id, 'anon_old', 'alias lleva el PREV id como anonymous_id');
    assert.equal(ali.distinct_id, 'user_99');
    assert.equal(trk.distinct_id, 'user_99', 'tras alias, los eventos usan el nuevo id');
  } finally {
    server.close();
  }
});

// ---- Feature flags ----

test('feature flags: evalúa conditions de properties y rollout 100 localmente', async () => {
  const { server, port } = await startServer((_req, res, _body) => {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({
      project: 'default',
      flags: [
        {
          key: 'new-checkout',
          rollout_percentage: 100,
          conditions: { properties: { plan: 'pro' } },
        },
      ],
    }));
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'flags-test',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    featureFlagRefreshIntervalMs: 100_000,
  });
  try {
    await c.refreshFeatureFlags();
    assert.equal(
      c.isFeatureEnabled('new-checkout', { distinct_id: 'user_42', properties: { plan: 'pro' } }),
      true,
    );
    assert.equal(
      c.isFeatureEnabled('new-checkout', { distinct_id: 'user_42', properties: { plan: 'free' } }),
      false,
    );
  } finally {
    await c.close(500);
    server.close();
  }
});

test('feature flags: rollout parcial es sticky por distinct_id', async () => {
  const { server, port } = await startServer((_req, res, _body) => {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({
      project: 'default',
      flags: [{ key: 'beta-nav', rollout_percentage: 10, conditions: {} }],
    }));
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'flags-rollout',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    featureFlagRefreshIntervalMs: 100_000,
  });
  try {
    await c.refreshFeatureFlags();
    const first = c.isFeatureEnabled('beta-nav', { distinct_id: 'user_42' });
    for (let i = 0; i < 20; i++) {
      assert.equal(c.isFeatureEnabled('beta-nav', { distinct_id: 'user_42' }), first);
    }
  } finally {
    await c.close(500);
    server.close();
  }
});

test('feature flags: fallo de refresh conserva la cache anterior', async () => {
  let fail = false;
  const { server, port } = await startServer((_req, res, _body) => {
    if (fail) {
      res.writeHead(503);
      res.end('down');
      return;
    }
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({
      project: 'default',
      flags: [{ key: 'sticky-cache', rollout_percentage: 100, conditions: {} }],
    }));
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'flags-cache',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    featureFlagRefreshIntervalMs: 100_000,
    diag: () => {},
  });
  try {
    await c.refreshFeatureFlags();
    assert.equal(c.isFeatureEnabled('sticky-cache', { distinct_id: 'user_42' }), true);
    fail = true;
    await c.refreshFeatureFlags();
    assert.equal(c.isFeatureEnabled('sticky-cache', { distinct_id: 'user_42' }), true);
  } finally {
    await c.close(500);
    server.close();
  }
});

test('feature flags: isFeatureEnabled emite exposure una sola vez por variante y usuario', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    if (body) seen.push(JSON.parse(body));
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({
      project: 'default',
      flags: [{ key: 'new-checkout', rollout_percentage: 100, conditions: {} }],
    }));
  });

  const c = faro.init({
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'flags-exposure',
    installGlobalHandlers: false,
    flushIntervalMs: 100_000,
    featureFlagRefreshIntervalMs: 100_000,
  });
  try {
    await c.refreshFeatureFlags();
    assert.equal(c.isFeatureEnabled('new-checkout', { distinct_id: 'user_42' }), true);
    assert.equal(c.isFeatureEnabled('new-checkout', { distinct_id: 'user_42' }), true);
    await c.flush();

    const events = seen.flatMap((b) => b.events ?? []);
    const exposures = events.filter((e) => e.name === '$feature_exposure');
    assert.equal(exposures.length, 1);
    assert.equal(exposures[0].distinct_id, 'user_42');
    assert.deepEqual(exposures[0].properties, {
      flag_key: 'new-checkout',
      variant: 'B',
      enabled: true,
    });
  } finally {
    await c.close(500);
    server.close();
  }
});
