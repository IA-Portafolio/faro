// Seeds the local Faro instance with logs distributed across the last hour at
// varying density and severity. Goal: produce a histogram that visibly varies
// minute-to-minute and includes a noticeable spike for the brush demo.

const TOKEN = process.env.FARO_INGEST_TOKEN || 'dev-ingest-token';
const URL_BASE = process.env.FARO_URL || 'http://127.0.0.1:8080';

const SERVICES = ['billing', 'auth', 'api-gateway', 'web'];
const LEVELS = [
  ['INFO',  0.72],
  ['WARN',  0.16],
  ['DEBUG', 0.06],
  ['ERROR', 0.05],
  ['FATAL', 0.01]
];

function pickLevel() {
  const r = Math.random();
  let acc = 0;
  for (const [lvl, w] of LEVELS) { acc += w; if (r < acc) return lvl; }
  return 'INFO';
}

const now = Date.now();
const HOUR = 60 * 60 * 1000;
const fromMs = now - HOUR;
const logs = [];

for (let minute = 0; minute < 60; minute++) {
  const minStart = fromMs + minute * 60 * 1000;
  // Baseline 5..25 logs/min, with a fat spike at minute 35 (~100 logs).
  const spike = Math.exp(-Math.pow((minute - 35) / 3, 2)) * 80;
  const baseline = 5 + Math.floor(Math.random() * 20);
  const count = Math.max(1, Math.round(baseline + spike));
  for (let i = 0; i < count; i++) {
    const ts = new Date(minStart + Math.floor(Math.random() * 60_000)).toISOString();
    const svc = SERVICES[Math.floor(Math.random() * SERVICES.length)];
    const lvl = pickLevel();
    logs.push({
      timestamp: ts,
      service: svc,
      level: lvl,
      message: `[${svc}] ${lvl} event #${minute}.${i}`,
      attributes: { source: 'seed', minute: String(minute) }
    });
  }
}

console.log(`posting ${logs.length} logs across last 60 min...`);

const res = await fetch(`${URL_BASE}/api/v1/ingest/logs`, {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
    'Authorization': `Bearer ${TOKEN}`
  },
  body: JSON.stringify({ service: 'mixed', logs })
});

const txt = await res.text();
console.log(res.status, txt);
if (!res.ok) process.exit(1);
