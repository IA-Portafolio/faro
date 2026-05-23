// Small mock server used to validate the LogVolumeHistogram component end-to-end
// in the browser without waiting for the full Rust backend build. Mirrors the
// shape of /api/v1/auth/me, /api/v1/logs/stats and /api/v1/logs.
import http from 'node:http';

const PORT = 8080;

const SEVERITIES = [
  { text: 'INFO', num: 9, weight: 70 },
  { text: 'WARN', num: 13, weight: 18 },
  { text: 'ERROR', num: 17, weight: 8 },
  { text: 'DEBUG', num: 5, weight: 3 },
  { text: 'FATAL', num: 21, weight: 1 }
];
const SERVICES = ['billing', 'auth', 'api-gateway'];

function pad(n) { return String(n).padStart(2, '0'); }
function chTs(d) {
  // ClickHouse-style "YYYY-MM-DD HH:MM:SS" (UTC)
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth()+1)}-${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`;
}
function nsTs(d) {
  // DateTime64 nanosecond style used by /logs response
  return `${chTs(d)}.000000000`;
}

// Seeded RNG so the synthetic data is stable across page reloads.
function mulberry32(a) {
  return function() {
    a = (a + 0x6D2B79F5) | 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function statsBuckets(fromMs, toMs, bucketSec) {
  const bucketMs = bucketSec * 1000;
  const startBucket = Math.floor(fromMs / bucketMs) * bucketMs;
  const endBucket = Math.floor(toMs / bucketMs) * bucketMs;
  const out = [];
  for (let t = startBucket; t <= endBucket; t += bucketMs) {
    const rng = mulberry32(Math.floor(t / 60000));
    // Mostly mild volume with a single spike around 35% into the range.
    const norm = (t - startBucket) / Math.max(1, endBucket - startBucket);
    const baseline = 8 + Math.floor(rng() * 14);
    const spike = Math.exp(-Math.pow((norm - 0.35) * 5, 2)) * 50;
    const total = Math.max(1, Math.round(baseline + spike));
    for (const sev of SEVERITIES) {
      const c = Math.floor((total * sev.weight) / 100);
      if (c <= 0) continue;
      for (const svc of SERVICES) {
        const portion = Math.floor(c / SERVICES.length);
        if (portion <= 0) continue;
        out.push({ ts: chTs(new Date(t)), service: svc, severity: sev.text, count: portion });
      }
    }
  }
  return out;
}

function logsRows(fromMs, toMs, limit) {
  const out = [];
  const total = Math.min(limit, 300);
  for (let i = 0; i < total; i++) {
    const t = toMs - Math.random() * (toMs - fromMs);
    const sev = SEVERITIES[Math.floor(Math.random() * SEVERITIES.length)];
    const svc = SERVICES[Math.floor(Math.random() * SERVICES.length)];
    out.push({
      timestamp: nsTs(new Date(t)),
      observed_timestamp: nsTs(new Date(t)),
      service_name: svc,
      severity_text: sev.text,
      severity_number: sev.num,
      body: `mock log ${i} on ${svc}`,
      trace_id: '',
      span_id: '',
      scope_name: '',
      resource_attributes: {},
      attributes: { mock: 'true' }
    });
  }
  out.sort((a, b) => b.timestamp.localeCompare(a.timestamp));
  return out;
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  res.setHeader('Access-Control-Allow-Origin', req.headers.origin || '*');
  res.setHeader('Access-Control-Allow-Credentials', 'true');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type');
  res.setHeader('Access-Control-Allow-Methods', 'GET,POST,PUT,DELETE,OPTIONS');
  if (req.method === 'OPTIONS') { res.statusCode = 204; res.end(); return; }

  const now = Date.now();

  function resolveRange() {
    const fromParam = url.searchParams.get('from');
    const toParam = url.searchParams.get('to');
    if (fromParam && toParam) {
      return [Date.parse(fromParam), Date.parse(toParam)];
    }
    const lastMin = parseInt(url.searchParams.get('last_minutes') || '60', 10);
    return [now - lastMin * 60 * 1000, now];
  }

  if (url.pathname === '/api/v1/auth/me') {
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ id: 'u1', email: 'admin@local.test', name: 'Admin', role: 'admin' }));
    return;
  }
  if (url.pathname === '/api/v1/logs/stats') {
    const [fromMs, toMs] = resolveRange();
    const bucketSec = parseInt(url.searchParams.get('bucket_seconds') || '60', 10);
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify(statsBuckets(fromMs, toMs, bucketSec)));
    return;
  }
  if (url.pathname === '/api/v1/logs') {
    const [fromMs, toMs] = resolveRange();
    const limit = parseInt(url.searchParams.get('limit') || '500', 10);
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify(logsRows(fromMs, toMs, limit)));
    return;
  }
  if (url.pathname === '/api/v1/projects') {
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify([]));
    return;
  }
  if (url.pathname === '/api/v1/logs/live') {
    res.setHeader('Content-Type', 'text/event-stream');
    res.setHeader('Cache-Control', 'no-cache');
    res.write(':\n\n');
    return; // keep open, no events
  }

  res.statusCode = 404;
  res.setHeader('Content-Type', 'application/json');
  res.end(JSON.stringify({ error: 'not found in mock', path: url.pathname }));
});

server.listen(PORT, () => {
  console.log(`mock api listening on http://127.0.0.1:${PORT}`);
});
