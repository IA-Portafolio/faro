/**
 * Tests unitarios del SDK Next.js client (browser-core) — 4 invariantes:
 *   1. queue cap
 *   2. retry on 5xx
 *   3. beforeSend filtra/transforma
 *   4. scrubbing
 *
 * Stubeamos los globales del navegador antes de importar — la suite corre con
 * `node --test` sin JSDOM. Desactivamos toda auto-captura para minimizar la
 * superficie de los stubs.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';

// ---- Stubs mínimos de globales del navegador ----
// Tienen que estar definidos ANTES de importar el dist.

const storage = () => {
  const m = new Map();
  return {
    getItem: (k) => (m.has(k) ? m.get(k) : null),
    setItem: (k, v) => void m.set(k, String(v)),
    removeItem: (k) => void m.delete(k),
    clear: () => m.clear(),
  };
};

// Algunos globales en Node 24+ son getters de solo-lectura — sobrescribimos
// vía defineProperty con `configurable:true` y `writable:true`.
const define = (k, v) =>
  Object.defineProperty(globalThis, k, { value: v, writable: true, configurable: true });

const noopListener = () => {};
const windowListeners = new Map();
const documentListeners = new Map();
const locationState = { href: 'http://test/', pathname: '/', hash: '' };

function addListener(registry, type, handler) {
  const handlers = registry.get(type) ?? new Set();
  handlers.add(handler);
  registry.set(type, handlers);
}

function removeListener(registry, type, handler) {
  registry.get(type)?.delete(handler);
}

function dispatchWindow(type, event = {}) {
  for (const handler of windowListeners.get(type) ?? []) handler(event);
}

function dispatchDocument(type, event = {}) {
  for (const handler of documentListeners.get(type) ?? []) handler(event);
}

function setTestLocation(url) {
  const parsed = new URL(url, locationState.href);
  locationState.href = parsed.href;
  locationState.pathname = parsed.pathname;
  locationState.hash = parsed.hash;
}

function resetBrowserHarness() {
  windowListeners.clear();
  documentListeners.clear();
  if (globalThis.MutationObserver?.instances) globalThis.MutationObserver.instances = [];
  setTestLocation('http://test/');
  history.pushState = (_state, _title, url) => {
    if (url) setTestLocation(url);
  };
  history.replaceState = (_state, _title, url) => {
    if (url) setTestLocation(url);
  };
}

const win = {
  addEventListener: (type, handler) => addListener(windowListeners, type, handler),
  removeEventListener: (type, handler) => removeListener(windowListeners, type, handler),
  location: locationState,
};
define('window', win);
define('document', {
  visibilityState: 'visible',
  body: { textContent: 'initial' },
  referrer: 'http://referrer/',
  addEventListener: (type, handler) => addListener(documentListeners, type, handler),
  removeEventListener: (type, handler) => removeListener(documentListeners, type, handler),
});
define('navigator', { userAgent: 'node-test', sendBeacon: () => false });
define('location', locationState);
define('sessionStorage', storage());
define('localStorage', storage());
define('crypto', { randomUUID: () => '11111111-1111-4111-8111-111111111111' });
define('history', { pushState: noopListener, replaceState: noopListener });
define('MutationObserver', class {
  static instances = [];
  constructor(callback) {
    this.callback = callback;
    this.connected = false;
    this.constructor.instances.push(this);
  }
  observe() { this.connected = true; }
  disconnect() { this.connected = false; }
  static trigger() {
    for (const observer of this.instances) {
      if (observer.connected) observer.callback([{ type: 'childList' }]);
    }
  }
});
resetBrowserHarness();

// Importación dinámica DESPUÉS de stubs.
const faro = await import('../dist/client.js');

// ---- helpers ----

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

function fakeElement({
  tagName = 'button',
  id = '',
  text = '',
  attrs = {},
  href = '',
  parent = null,
} = {}) {
  const lowerAttrs = Object.fromEntries(Object.entries(attrs).map(([k, v]) => [k.toLowerCase(), v]));
  const el = {
    tagName: tagName.toUpperCase(),
    id,
    textContent: text,
    href,
    parentElement: parent,
    getAttribute(name) {
      return lowerAttrs[name.toLowerCase()] ?? null;
    },
    hasAttribute(name) {
      return Object.prototype.hasOwnProperty.call(lowerAttrs, name.toLowerCase());
    },
  };
  return el;
}

function eventPayloads(seen) {
  return seen.flatMap((s) => (s.body ?? s).events ?? []);
}

function commonOpts(port) {
  return {
    endpoint: `http://127.0.0.1:${port}`,
    token: 'tk',
    service: 'browser-tests',
    captureUnhandled: false,
    captureConsole: false,
    captureWebVitals: false,
    captureClicks: false,
    captureNavigation: false,
    flushIntervalMs: 100_000,
    featureFlagRefreshIntervalMs: 100_000,
  };
}

// ---- 1. queue cap ----

test('queue cap: enqueue silenciosamente al alcanzar el límite', async () => {
  const c = faro.initFaroClient({ ...commonOpts(1), maxQueueSize: 5 });
  try {
    for (let i = 0; i < 50; i++) c.log({ level: 'INFO', message: `e${i}` });
    assert.equal(c.queue.length, 5, 'la cola debe pararse en el cap');
  } finally {
    c.close();
  }
});

// ---- 2. retry on 5xx ----

test('5xx: el batch se re-encola para reintentar', async () => {
  let calls = 0;
  const { server, port } = await startServer((_req, res, _body) => {
    calls += 1;
    if (calls === 1) {
      res.writeHead(503);
      res.end('caído');
    } else {
      res.writeHead(200);
      res.end('{}');
    }
  });

  const c = faro.initFaroClient(commonOpts(port));
  try {
    c.log({ level: 'INFO', message: 'reintentar-me' });
    await c.flush();
    // En 5xx el browser-core re-encola; un segundo flush manda otra vez.
    await c.flush();
    await delay(50);
    assert.ok(calls >= 2, 'debe haber llegado un segundo intento');
  } finally {
    c.close();
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

  const c = faro.initFaroClient({
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
    c.close();
    server.close();
  }
});

// ---- 4. scrubbing ----

// ---- 5. init con opts inválidas ----

test('init: endpoint vacío lanza Error claro', () => {
  assert.throws(
    () => faro.initFaroClient({ endpoint: '', token: 'tk', service: 's' }),
    /endpoint.*obligatorio/i,
  );
});

test('init: token ausente lanza Error claro', () => {
  assert.throws(
    () => faro.initFaroClient({ endpoint: 'http://x', service: 's' }),
    /token.*obligatorio/i,
  );
});

test('init: service ausente lanza Error claro', () => {
  assert.throws(
    () => faro.initFaroClient({ endpoint: 'http://x', token: 'tk' }),
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

  const c = faro.initFaroClient({
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
    c.close();
    server.close();
  }
});

// ---- 7. auto-captura: captureException compone shape OTel ----
//
// window.onerror / unhandledrejection invocan captureException internamente.
// Si el método compone bien la entry, el flujo auto-disparado también lo hace
// (los handlers son wrappers de 2-3 líneas). Disparar un event sintético
// requeriría un addEventListener real (los stubs actuales son noop) — se sale
// del scope de este test.

test('captureException compone exception.{type,message,stacktrace}', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });
  const c = faro.initFaroClient(commonOpts(port));
  try {
    let err;
    try { throw new TypeError('boom sintético'); } catch (e) { err = e; }
    c.captureException(err, { tags: { origin: 'test' } });
    await c.flush();
    await delay(50);

    const entry = seen[0].logs[0];
    assert.equal(entry.level, 'ERROR');
    assert.match(entry.message, /boom sintético/);
    assert.equal(entry.attributes['exception.type'], 'TypeError');
    assert.equal(entry.attributes['exception.message'], 'boom sintético');
    assert.ok(entry.attributes['exception.stacktrace'].length > 0, 'stacktrace presente');
    assert.equal(entry.attributes['origin'], 'test');
  } finally {
    c.close();
    server.close();
  }
});

// ---- 8. close() graceful ----
//
// El close() del browser SDK es síncrono y dispara flush() fire-and-forget
// (porque en pagehide/visibilitychange no podemos await). Aceptamos el patrón:
// log → close() → esperar un breve momento → server recibió. Si el flush
// jamás se hubiera disparado, el server no vería nada.

test('close: el flush disparado en close entrega los eventos en cola', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });
  const c = faro.initFaroClient({
    ...commonOpts(port),
    // Intervalo lejano: si no fuera por el flush de close(), estos eventos
    // NO llegarían al server.
    flushIntervalMs: 100_000,
  });
  for (let i = 0; i < 7; i++) c.log({ level: 'INFO', message: `evento-${i}` });
  c.close();
  // El flush de close() es fire-and-forget; esperamos a que el POST termine.
  await delay(200);

  const msgs = seen.flatMap((b) => b.logs.map((l) => l.message));
  assert.equal(msgs.length, 7, 'close() debe drenar los 7 eventos en cola');
  assert.deepEqual(
    msgs,
    ['evento-0','evento-1','evento-2','evento-3','evento-4','evento-5','evento-6'],
  );
  server.close();
});

test('scrubbing: scrubFields + scrubPatterns', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.initFaroClient(commonOpts(port));
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
    c.close();
    server.close();
  }
});

// ---- Product events API: track / identify / page / alias ----

test('track: envía evento a /api/v1/ingest/events con source web y session_id', async () => {
  const seen = [];
  const { server, port } = await startServer((req, res, body) => {
    seen.push({ url: req.url, body: JSON.parse(body) });
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.initFaroClient(commonOpts(port));
  try {
    c.track('checkout_completed', { amount: 99.5, currency: 'USD' });
    await c.flush();
    await delay(50);

    const eventBatch = seen.find((s) => s.url.endsWith('/api/v1/ingest/events'));
    assert.ok(eventBatch, `esperaba POST a /events; visto: ${JSON.stringify(seen.map((s) => s.url))}`);
    assert.equal(eventBatch.body.service, 'browser-tests');
    const event = eventBatch.body.events[0];
    assert.equal(event.type, 'track');
    assert.equal(event.name, 'checkout_completed');
    assert.deepEqual(event.properties, { amount: 99.5, currency: 'USD' });
    assert.match(event.distinct_id, /^[0-9a-f-]{36}$/, 'pre-identify distinct_id debe ser UUID');
    assert.equal(event.distinct_id, event.anonymous_id);
    assert.ok(event.session_id.length > 0, 'browser events deben llevar session_id');
    assert.equal(event.source, 'web');
  } finally {
    c.close();
    server.close();
  }
});

test('anonymous_id: usa crypto.randomUUID y persiste en localStorage', async () => {
  localStorage.clear();
  let i = 0;
  globalThis.crypto.randomUUID = () => `11111111-1111-4111-8111-11111111111${++i}`;
  const { server, port } = await startServer((_req, res, _body) => {
    res.writeHead(200);
    res.end('{}');
  });

  const c1 = faro.initFaroClient(commonOpts(port));
  c1.track('first');
  const first = c1.eventsQueue[0].anonymous_id;
  c1.close();

  const c2 = faro.initFaroClient(commonOpts(port));
  c2.track('second');
  const second = c2.eventsQueue[0].anonymous_id;
  c2.close();
  server.close();

  assert.equal(first, '11111111-1111-4111-8111-111111111111');
  assert.equal(second, first, 'localStorage conserva el mismo anonymous_id');
  assert.equal(localStorage.getItem('faro.anon_id'), first);
});

test('identify: fija distinct_id para eventos siguientes y enriquece logs con user.id', async () => {
  const seen = [];
  const { server, port } = await startServer((req, res, body) => {
    seen.push({ url: req.url, body: JSON.parse(body) });
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.initFaroClient(commonOpts(port));
  try {
    c.identify('user_42', { email: 'a@b.com', plan: 'pro' });
    c.track('after_login');
    c.info('post-login log');
    await c.flush();
    await delay(50);

    const events = seen.flatMap((s) => s.body.events ?? []);
    const alias = events.find((e) => e.type === 'alias');
    const identify = events.find((e) => e.type === 'identify');
    const track = events.find((e) => e.type === 'track');
    assert.ok(alias, 'identify debe emitir $alias automáticamente');
    assert.equal(alias.name, '$alias');
    assert.equal(alias.distinct_id, 'user_42');
    assert.equal(alias.anonymous_id, track.anonymous_id);
    assert.deepEqual(alias.properties, { from: track.anonymous_id, to: 'user_42' });
    assert.ok(identify, 'debe llegar un identify');
    assert.equal(identify.name, '$identify');
    assert.equal(identify.distinct_id, 'user_42');
    assert.equal(identify.anonymous_id, track.anonymous_id);
    assert.deepEqual(identify.user_properties, { email: 'a@b.com', plan: 'pro' });
    assert.ok(track, 'debe llegar el track posterior');
    assert.equal(track.distinct_id, 'user_42');
    assert.equal(track.anonymous_id, alias.anonymous_id);

    const logs = seen.flatMap((s) => s.body.logs ?? []);
    assert.equal(logs[0].attributes['user.id'], 'user_42');
    assert.equal(logs[0].attributes['user.email'], 'a@b.com');
  } finally {
    c.close();
    server.close();
  }
});

test('identify: no duplica $alias para el mismo user en la misma sesión', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.initFaroClient(commonOpts(port));
  try {
    c.identify('user_42');
    c.identify('user_42');
    await c.flush();
    await delay(50);

    const aliases = seen.flatMap((b) => b.events ?? []).filter((e) => e.type === 'alias');
    assert.equal(aliases.length, 1);
  } finally {
    c.close();
    server.close();
  }
});

test('page: emite page view web con path y propiedades', async () => {
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.initFaroClient(commonOpts(port));
  try {
    c.page('/checkout/success', { source: 'cart' });
    await c.flush();
    await delay(50);

    const event = seen.flatMap((b) => b.events ?? []).find((e) => e.type === 'page');
    assert.ok(event, 'debe llegar un page event');
    assert.equal(event.name, '/checkout/success');
    assert.deepEqual(event.properties, { source: 'cart' });
    assert.equal(event.source, 'web');
  } finally {
    c.close();
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

  const c = faro.initFaroClient(commonOpts(port));
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
    assert.deepEqual(alias.properties, { from: 'anonymous_abc123', to: 'user_42' });
    assert.equal(track.distinct_id, 'user_42');
  } finally {
    c.close();
    server.close();
  }
});

// ---- Auto tracking opt-in ----

test('autoCapture.pageViews: emite page inicial y navegaciones history', async () => {
  resetBrowserHarness();
  const seen = [];
  const { server, port } = await startServer((req, res, body) => {
    seen.push({ url: req.url, body: JSON.parse(body) });
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.initFaroClient({
    ...commonOpts(port),
    autoCapture: { pageViews: true },
  });
  try {
    await c.flush();
    history.pushState({}, '', '/checkout/success');
    await c.flush();
    await delay(50);

    const pages = eventPayloads(seen).filter((event) => event.type === 'page');
    assert.equal(pages.length, 2);
    assert.equal(pages[0].name, '/');
    assert.equal(pages[0].properties.navigation_type, 'initial');
    assert.equal(pages[1].name, '/checkout/success');
    assert.equal(pages[1].properties.navigation_type, 'pushState');
  } finally {
    c.close();
    server.close();
  }
});

test('autoCapture.clicks: captura data-faro, button y a como product events', async () => {
  resetBrowserHarness();
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.initFaroClient({
    ...commonOpts(port),
    autoCapture: { clicks: true },
  });
  try {
    dispatchDocument('click', { target: fakeElement({ tagName: 'div', text: 'ignorar' }) });
    dispatchDocument('click', { target: fakeElement({ tagName: 'button', id: 'pay', text: 'Pagar' }) });
    dispatchDocument('click', {
      target: fakeElement({ tagName: 'div', text: 'Hero CTA', attrs: { 'data-faro': 'hero_cta' } }),
    });
    dispatchDocument('click', {
      target: fakeElement({ tagName: 'a', text: 'Docs', href: 'https://docs.test/' }),
    });
    await c.flush();
    await delay(50);

    const clicks = eventPayloads(seen).filter((event) => event.name === '$autocapture');
    assert.equal(clicks.length, 3);
    assert.deepEqual(
      clicks.map((event) => event.properties.type),
      ['click', 'click', 'click'],
    );
    assert.equal(clicks[0].properties.tag, 'button');
    assert.equal(clicks[0].properties.id, 'pay');
    assert.equal(clicks[1].properties.faro, 'hero_cta');
    assert.equal(clicks[2].properties.href, 'https://docs.test/');
  } finally {
    c.close();
    server.close();
  }
});

test('autoCapture.formSubmissions: captura solo form[data-faro-form]', async () => {
  resetBrowserHarness();
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.initFaroClient({
    ...commonOpts(port),
    autoCapture: { formSubmissions: true },
  });
  try {
    dispatchDocument('submit', { target: fakeElement({ tagName: 'form', attrs: {} }) });
    dispatchDocument('submit', {
      target: fakeElement({ tagName: 'form', id: 'checkout', attrs: { 'data-faro-form': 'checkout' } }),
    });
    await c.flush();
    await delay(50);

    const submits = eventPayloads(seen).filter((event) => event.name === '$form_submit');
    assert.equal(submits.length, 1);
    assert.equal(submits[0].properties.id, 'checkout');
    assert.equal(submits[0].properties.faro_form, 'checkout');
  } finally {
    c.close();
    server.close();
  }
});

test('autoCapture.rageClicks: 3 clicks en menos de 2s sobre el mismo elemento generan insight', async () => {
  resetBrowserHarness();
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.initFaroClient({
    ...commonOpts(port),
    autoCapture: { rageClicks: true },
  });
  try {
    const target = fakeElement({ tagName: 'button', id: 'retry', text: 'Reintentar' });
    dispatchDocument('click', { target });
    dispatchDocument('click', { target });
    dispatchDocument('click', { target });
    await c.flush();
    await delay(50);

    const rage = eventPayloads(seen).find((event) => event.name === '$rage_click');
    assert.ok(rage, 'debe llegar un rage click');
    assert.equal(rage.properties.click_count, 3);
    assert.equal(rage.properties.id, 'retry');
  } finally {
    c.close();
    server.close();
  }
});

test('autoCapture.deadClicks: click elegible sin cambio de DOM ni URL genera insight', async () => {
  resetBrowserHarness();
  const seen = [];
  const { server, port } = await startServer((_req, res, body) => {
    seen.push(JSON.parse(body));
    res.writeHead(200);
    res.end('{}');
  });

  const c = faro.initFaroClient({
    ...commonOpts(port),
    autoCapture: { deadClicks: true },
  });
  try {
    dispatchDocument('click', { target: fakeElement({ tagName: 'button', id: 'dead', text: 'No hace nada' }) });
    await delay(850);
    await c.flush();
    await delay(50);

    const dead = eventPayloads(seen).find((event) => event.name === '$dead_click');
    assert.ok(dead, 'debe llegar un dead click');
    assert.equal(dead.properties.id, 'dead');
    assert.equal(dead.properties.tag, 'button');
  } finally {
    c.close();
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

  const c = faro.initFaroClient(commonOpts(port));
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
    c.close();
    server.close();
  }
});

test('feature flags: fallo de refresh conserva cache anterior', async () => {
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

  const c = faro.initFaroClient(commonOpts(port));
  try {
    await c.refreshFeatureFlags();
    assert.equal(c.isFeatureEnabled('sticky-cache', { distinct_id: 'user_42' }), true);
    fail = true;
    await c.refreshFeatureFlags();
    assert.equal(c.isFeatureEnabled('sticky-cache', { distinct_id: 'user_42' }), true);
  } finally {
    c.close();
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

  const c = faro.initFaroClient(commonOpts(port));
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
    c.close();
    server.close();
  }
});

// ---- 9. seguridad: beacon (useBeacon=true) NO filtra el token en la URL ----
//
// `navigator.sendBeacon` no permite custom headers en los browsers principales,
// por lo que una versión naive embebe el bearer en `?_token=` y lo filtra a
// access-logs, history, referer y proxies. La fix usa `fetch keepalive`,
// que sí soporta headers, así que el token viaja en `Authorization: Bearer`
// y la URL queda limpia. Estos tests pin el contrato: si alguien vuelve a
// meter el token en query string, rompen en rojo.

test('beacon-flush: el token NO aparece en la URL del POST a /logs', async () => {
  const seen = [];
  const { server, port } = await startServer((req, res, body) => {
    seen.push({ method: req.method, url: req.url, body });
    res.writeHead(200);
    res.end('{}');
  });
  const c = faro.initFaroClient({ ...commonOpts(port), token: 'secreto-privado' });
  try {
    c.log({ level: 'INFO', message: 'beacon-test' });
    // useBeacon=true ejercita el path que antes usaba sendBeacon con ?_token=.
    await c.flush(true);
    await delay(50);

    const logReq = seen.find((s) => s.url.startsWith('/api/v1/ingest/logs'));
    assert.ok(logReq, 'debe llegar POST a /api/v1/ingest/logs');
    assert.ok(!logReq.url.includes('?'), `URL no debe llevar query string: ${logReq.url}`);
    assert.ok(
      !logReq.url.includes('secreto-privado') && !logReq.body.includes('secreto-privado'),
      'el token NO debe aparecer ni en URL ni en body'
    );
  } finally {
    c.close();
    server.close();
  }
});

test('beacon-flush: el token viaja en Authorization: Bearer, no en query string', async () => {
  const seen = [];
  const { server, port } = await startServer((req, res, _body) => {
    seen.push({ url: req.url, auth: req.headers['authorization'] });
    res.writeHead(200);
    res.end('{}');
  });
  const c = faro.initFaroClient({ ...commonOpts(port), token: 'mi-token' });
  try {
    c.log({ level: 'INFO', message: 'auth-header' });
    await c.flush(true);
    await delay(50);

    const logReq = seen.find((s) => s.url.startsWith('/api/v1/ingest/logs'));
    assert.ok(logReq, 'debe llegar POST a /api/v1/ingest/logs');
    assert.equal(logReq.url, '/api/v1/ingest/logs', 'URL limpia, sin query string');
    assert.equal(logReq.auth, 'Bearer mi-token', 'el token viaja en Authorization header');
  } finally {
    c.close();
    server.close();
  }
});
