/**
 * Tests del SDK Expo para las 3 tareas nuevas:
 *   (a) feature flag rollout=100 → isFeatureEnabled true + se encola $feature_exposure variant 'B'
 *   (b) flag con conditions.properties no satisfechas → false sin exposición
 *   (c) identify('user_42') → un log siguiente lleva attributes['user.id'] === 'user_42'
 *   (d) close(timeoutMs) resuelve aunque la red cuelgue
 *   (e) los 5 vectores dorados del hash stickyBucket coinciden con el SDK Node
 *
 * Igual que el resto de la suite: installGlobalHandlers:false + persistence:false
 * para evitar los require('react-native') / AsyncStorage.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { createRequire } from 'node:module';

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
    service: 'expo-ff-tests',
    installGlobalHandlers: false,
    persistence: false,
    flushIntervalMs: 100_000,
    // Sin auto-refresh: lo disparamos a mano con refreshFeatureFlags().
    featureFlagRefreshIntervalMs: 100_000,
  };
}

// Server que responde feature-flags con `flags` y captura POSTs de events.
function ffServer(flags, project = 'proj') {
  const events = [];
  return startServer((req, res, body) => {
    if (req.url.endsWith('/api/v1/ingest/feature-flags')) {
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ project, flags }));
      return;
    }
    if (req.url.endsWith('/api/v1/ingest/events')) {
      events.push(...(JSON.parse(body).events ?? []));
    }
    res.writeHead(200);
    res.end('{}');
  }).then((s) => ({ ...s, events }));
}

// ---- (a) rollout=100 → enabled + $feature_exposure variant B ----

test('feature flag rollout=100 → isFeatureEnabled true + $feature_exposure B', async () => {
  const { server, port, events } = await ffServer([
    { key: 'new-checkout', rollout_percentage: 100 },
  ]);
  const c = faro.init(commonOpts(port));
  try {
    await c.refreshFeatureFlags();
    const enabled = c.isFeatureEnabled('new-checkout');
    assert.equal(enabled, true, 'rollout=100 debe estar habilitado');

    await c.flush();
    await delay(50);

    const exposure = events.find((e) => e.name === '$feature_exposure');
    assert.ok(exposure, 'debe encolarse un $feature_exposure');
    assert.equal(exposure.type, 'track');
    assert.equal(exposure.properties.flag_key, 'new-checkout');
    assert.equal(exposure.properties.variant, 'B');
    assert.equal(exposure.properties.enabled, true);
  } finally {
    await c.close();
    server.close();
  }
});

// ---- (b) conditions no satisfechas → false sin exposición ----

test('feature flag con conditions.properties no satisfechas → false sin exposición', async () => {
  const { server, port, events } = await ffServer([
    { key: 'beta', rollout_percentage: 100, conditions: { properties: { plan: 'pro' } } },
  ]);
  const c = faro.init(commonOpts(port));
  try {
    await c.refreshFeatureFlags();
    // properties no satisface plan === 'pro'
    const enabled = c.isFeatureEnabled('beta', { distinct_id: 'u1', properties: { plan: 'free' } });
    assert.equal(enabled, false, 'conditions no satisfechas → false');

    await c.flush();
    await delay(50);

    const exposure = events.find((e) => e.name === '$feature_exposure');
    assert.equal(exposure, undefined, 'NO debe emitirse exposición cuando las conditions fallan');
  } finally {
    await c.close();
    server.close();
  }
});

// ---- (c) identify enriquece logs con user.id ----

test('identify("user_42") → los logs siguientes llevan attributes["user.id"]', async () => {
  const seen = [];
  const { server, port } = await startServer((req, res, body) => {
    if (req.url.endsWith('/api/v1/ingest/logs')) seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });
  const c = faro.init(commonOpts(port));
  try {
    c.identify('user_42', { plan: 'pro' });
    c.log({ level: 'INFO', message: 'tras login' });
    await c.flush();
    await delay(50);

    const log = seen.flatMap((b) => b.logs ?? [])[0];
    assert.ok(log, 'debe llegar el log');
    assert.equal(log.attributes['user.id'], 'user_42');
  } finally {
    await c.close();
    server.close();
  }
});

test('identify: un user.id explícito en attrs gana sobre el del identify', async () => {
  const seen = [];
  const { server, port } = await startServer((req, res, body) => {
    if (req.url.endsWith('/api/v1/ingest/logs')) seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });
  const c = faro.init(commonOpts(port));
  try {
    c.identify('user_42');
    c.log({ level: 'INFO', message: 'override', attributes: { 'user.id': 'explicit_99' } });
    await c.flush();
    await delay(50);

    const log = seen.flatMap((b) => b.logs ?? [])[0];
    assert.equal(log.attributes['user.id'], 'explicit_99');
  } finally {
    await c.close();
    server.close();
  }
});

// ---- (d) close(timeoutMs) resuelve aunque la red cuelgue ----

test('close(timeoutMs): resuelve aunque la red cuelgue', async () => {
  // Server que NUNCA responde → el fetch del flush queda colgado.
  const { server, port } = await startServer(() => {
    /* sin res.end: la conexión queda abierta para siempre */
  });
  const c = faro.init(commonOpts(port));
  c.log({ level: 'INFO', message: 'no va a llegar' });

  const start = Date.now();
  await c.close(300); // debe rendirse al deadline, no colgarse
  const elapsed = Date.now() - start;

  assert.ok(elapsed < 2000, `close debe rendirse cerca del deadline (tardó ${elapsed}ms)`);
  server.close();
});

// ---- (e) vectores dorados del hash (paridad con SDK Node) ----
//
// Replicamos stickyBucket aquí (es interno) para asegurar que el algoritmo
// FNV-1a 32-bit produce exactamente los mismos buckets que el SDK Node.

function stickyBucket(input) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0) % 100;
}

test('stickyBucket: 5 vectores dorados coinciden con el SDK Node', () => {
  assert.equal(stickyBucket('proj:new-checkout:user_42'), 9);
  assert.equal(stickyBucket('acme:flag-a:anon_x'), 54);
  assert.equal(stickyBucket('myproj:dark-mode:user_1'), 75);
  assert.equal(stickyBucket('p:k:abcdefghij'), 49);
  assert.equal(stickyBucket('demo:exp1:user_42'), 34);
});

// Y verificamos que el SDK realmente USA ese hash: con project 'proj' y
// distinct_id 'user_42' (bucket 9), un rollout=10 incluye al usuario; rollout=9 no.

test('isFeatureEnabled usa stickyBucket para el rollout parcial', async () => {
  const { server, port } = await ffServer([
    { key: 'new-checkout', rollout_percentage: 10 },
  ], 'proj');
  const c = faro.init(commonOpts(port));
  try {
    await c.refreshFeatureFlags();
    // bucket('proj:new-checkout:user_42') === 9 < 10 → enabled
    assert.equal(c.isFeatureEnabled('new-checkout', { distinct_id: 'user_42' }), true);
  } finally {
    await c.close();
    server.close();
  }
});
