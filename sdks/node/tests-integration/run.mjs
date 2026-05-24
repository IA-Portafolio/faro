/**
 * Integration test del SDK Node contra un Faro real (clickhouse + backend) levantado
 * por docker-compose.sdk-integration.yml. Verifica el camino completo:
 *
 *   1. Health del backend
 *   2. Login admin → cookie de sesión (necesaria para leer)
 *   3. Init SDK + log() con marcador único
 *   4. close() (drena la cola con timeout acotado)
 *   5. Polling de GET /api/v1/logs?service=...&query=<marker>
 *   6. Asserts: el evento llegó con el shape correcto
 *
 * Sale con exit 0 si todo OK, exit 1 con diagnóstico si algo falla — esa señal
 * es la que CI usa para bloquear publish-sdks.yml.
 */

import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';

const ENDPOINT = process.env.FARO_ENDPOINT || 'http://127.0.0.1:8080';
const TOKEN = process.env.FARO_TOKEN || 'dev-ingest-token';
const ADMIN_EMAIL = process.env.FARO_ADMIN_EMAIL || 'admin@local.test';
const ADMIN_PASSWORD = process.env.FARO_ADMIN_PASSWORD || 'admin12345';
const SERVICE = `sdk-integration-${process.platform}`;
const MARKER = `marker-${randomUUID()}`;

function log(msg) {
  console.log(`[integration] ${msg}`);
}

async function waitUntil(label, fn, { timeoutMs = 60_000, intervalMs = 1_000 } = {}) {
  const deadline = Date.now() + timeoutMs;
  let lastErr;
  while (Date.now() < deadline) {
    try {
      const result = await fn();
      if (result) return result;
    } catch (e) {
      lastErr = e;
    }
    await new Promise((r) => setTimeout(r, intervalMs));
  }
  throw new Error(`timeout esperando ${label}: ${lastErr?.message ?? 'cond falsa'}`);
}

async function healthCheck() {
  await waitUntil(
    'backend ready (/readyz=200)',
    async () => {
      const r = await fetch(`${ENDPOINT}/readyz`);
      return r.ok;
    },
    { timeoutMs: 120_000 },
  );
  log('backend healthy');
}

async function adminLogin() {
  const r = await fetch(`${ENDPOINT}/api/v1/auth/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email: ADMIN_EMAIL, password: ADMIN_PASSWORD }),
  });
  if (!r.ok) {
    const t = await r.text().catch(() => '');
    throw new Error(`login fallido: HTTP ${r.status} ${t.slice(0, 200)}`);
  }
  const setCookie = r.headers.get('set-cookie');
  if (!setCookie) throw new Error('login no devolvió Set-Cookie');
  // Quedarse solo con `name=value` (la primera coma separa cookies; quitamos atributos).
  const cookie = setCookie.split(/,\s*(?=[^=]+=)/)[0].split(';')[0];
  log(`login OK; cookie=${cookie.slice(0, 30)}...`);
  return cookie;
}

async function sendViaSdk() {
  const faro = await import('../dist/index.js');
  const c = faro.init({
    endpoint: ENDPOINT,
    token: TOKEN,
    service: SERVICE,
    installGlobalHandlers: false,
    flushIntervalMs: 200,
  });
  c.log({ level: 'INFO', message: `integration-test ${MARKER}`, attributes: { test_marker: MARKER } });
  await c.flush();
  await c.close(5_000);
  log(`log enviado con marker=${MARKER}`);
}

async function fetchLogs(cookie) {
  const url =
    `${ENDPOINT}/api/v1/logs` +
    `?service=${encodeURIComponent(SERVICE)}` +
    `&query=${encodeURIComponent(MARKER)}` +
    `&from=-15m`;
  const r = await fetch(url, { headers: { cookie } });
  if (!r.ok) {
    const t = await r.text().catch(() => '');
    throw new Error(`GET /api/v1/logs HTTP ${r.status}: ${t.slice(0, 200)}`);
  }
  return r.json();
}

async function main() {
  log(`endpoint=${ENDPOINT} service=${SERVICE} marker=${MARKER}`);

  await healthCheck();
  const cookie = await adminLogin();
  await sendViaSdk();

  // Polling: ingest es async (worker en backend → CH → flush). Damos hasta 30s.
  const rows = await waitUntil(
    'el log con marker aparezca en /api/v1/logs',
    async () => {
      const arr = await fetchLogs(cookie);
      if (Array.isArray(arr) && arr.length >= 1) return arr;
      return null;
    },
    { timeoutMs: 30_000, intervalMs: 1_000 },
  );

  assert.ok(rows.length >= 1, 'al menos una fila');
  const row = rows[0];
  log(`recuperado: ${JSON.stringify(row).slice(0, 200)}...`);

  // Asserts de shape: el SDK envía `service`, `level: 'INFO'`, `message`, `attributes`.
  // El backend los serializa como service_name / severity_text / body / attributes.
  assert.equal(row.service_name, SERVICE, 'service_name coincide');
  assert.equal(row.severity_text, 'INFO', 'severity_text correcto');
  assert.ok(
    typeof row.body === 'string' && row.body.includes(MARKER),
    'body contiene el marker',
  );
  assert.ok(row.attributes && row.attributes.test_marker === MARKER, 'attribute preservado');

  log('OK: shape verificado');
  process.exit(0);
}

main().catch((err) => {
  console.error('[integration] FAIL:', err?.message ?? err);
  console.error(err?.stack ?? '');
  process.exit(1);
});
