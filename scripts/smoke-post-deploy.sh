#!/usr/bin/env bash
#
# Smoke test post-deploy contra la instancia pública. Hace un round-trip
# completo (login → ingest → query → healthz) para detectar el escenario
# "el deploy pasó readyz pero la ingesta o la auth están rotas".
#
# Pensado para correr DESPUÉS del readyz que ya hace deploy.yml; si esto falla,
# el job de deploy queda en rojo (= notificación). Rollback automático no se
# hace porque las imágenes no están tageadas por SHA: la operación de rollback
# es `git revert + push` (o restaurar con backups), explícitamente humana.
#
# Env vars REQUERIDAS:
#   FARO_BASE_URL              ej. https://faro.iaportafolio.com
#
# Env vars OPCIONALES (si faltan, se hace `warn` + se saltan los checks
# que dependen de ellas — el check de healthz/protocol sí se ejecuta siempre):
#   FARO_SMOKE_EMAIL           usuario de test sin 2FA
#   FARO_SMOKE_PASSWORD        password del usuario de test
#   FARO_SMOKE_INGEST_TOKEN    bearer de proyecto para POST /api/v1/ingest/logs
#   FARO_EXPECTED_PROTOCOL     protocol.current esperado (default: 1)
#   FARO_SMOKE_TIMEOUT         segundos a esperar a que el log aterrice en CH
#                              (default: 30 — el writer hace flush cada 750 ms,
#                              30 s cubre throttle + insert + cualquier retry)
#
# Exit codes (útiles para distinguir qué falló en alarmas):
#   0  OK
#   1  config inválida o herramientas faltantes
#   2  healthz fail (down o versión de protocolo no esperada)
#   3  auth/login fail
#   4  ingest/logs fail
#   5  query: log no apareció en /api/v1/logs dentro del timeout

set -euo pipefail

BASE_URL="${FARO_BASE_URL:?FARO_BASE_URL required (e.g. https://faro.iaportafolio.com)}"
EXPECTED_PROTOCOL="${FARO_EXPECTED_PROTOCOL:-1}"
SMOKE_TIMEOUT="${FARO_SMOKE_TIMEOUT:-30}"
SMOKE_EMAIL="${FARO_SMOKE_EMAIL:-}"
SMOKE_PASSWORD="${FARO_SMOKE_PASSWORD:-}"
SMOKE_INGEST_TOKEN="${FARO_SMOKE_INGEST_TOKEN:-}"

# ---------- helpers ----------

note() { printf 'smoke[%s] %s\n' "$1" "$2"; }
warn() { printf '::warning::smoke[%s] %s\n' "$1" "$2" >&2; }
fail() {
  printf '::error::smoke[FAIL] %s\n' "$1" >&2
  exit "${2:-1}"
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required tool: $1 (install with: $2)" 1
}

require_cmd curl "apt-get install -y curl"
require_cmd jq   "apt-get install -y jq"

# Cookie jar temporal — se limpia siempre, incluso si fallamos.
JAR="$(mktemp -t faro-smoke-cookies.XXXXXX 2>/dev/null || mktemp)"
trap 'rm -f "$JAR"' EXIT

# ---------- 1) /healthz ----------
#
# Health + versión de protocolo. Lo corremos ANTES que login para confirmar
# que el binario que aterrizó habla el protocolo que esperamos — si subió
# una versión que cambió wire format sin que nadie haya bumpeado los SDKs,
# queremos saberlo ya y rojo, no después de que un cliente se rompa.

note healthz "GET $BASE_URL/healthz"
HEALTH_JSON=$(curl -fsS --max-time 10 "$BASE_URL/healthz") \
  || fail "healthz did not return 2xx (URL: $BASE_URL/healthz)" 2

PROTO=$(printf '%s' "$HEALTH_JSON" | jq -r '.protocol.current // empty')
if [ -z "$PROTO" ]; then
  fail "healthz body missing .protocol.current. body=$HEALTH_JSON" 2
fi
if [ "$PROTO" != "$EXPECTED_PROTOCOL" ]; then
  fail "healthz protocol mismatch: got $PROTO, expected $EXPECTED_PROTOCOL. Bump FARO_EXPECTED_PROTOCOL si subiste el protocolo a propósito; ver versions.rs." 2
fi
VERSION=$(printf '%s' "$HEALTH_JSON" | jq -r '.version // "unknown"')
note healthz "OK — version=$VERSION protocol.current=$PROTO"

# ---------- gate: a partir de acá hace falta creds + token ----------

if [ -z "$SMOKE_EMAIL" ] || [ -z "$SMOKE_PASSWORD" ] || [ -z "$SMOKE_INGEST_TOKEN" ]; then
  warn config "FARO_SMOKE_EMAIL/PASSWORD/INGEST_TOKEN no configurados en .env.prod — saltando login/ingest/query. El smoke pasa con sólo /healthz, lo cual NO valida que la ingesta funcione. Configurar para cubrir el caso 'readyz verde pero ingesta rota'."
  exit 0
fi

# ---------- 2) POST /api/v1/auth/login ----------
#
# La respuesta puede ser AuthUser (sin 2FA) o NeedsTotp (con 2FA): ambas son 200.
# Para que el paso 4 (GET /api/v1/logs) funcione necesitamos cookie real → si
# el usuario tiene 2FA, sólo podemos confirmar que el endpoint vive y rechazar
# explícitamente los pasos que dependen de sesión. Por eso el smoke user
# DEBE ser un usuario dedicado sin 2FA — no usar el admin humano.

note login "POST $BASE_URL/api/v1/auth/login as $SMOKE_EMAIL"
LOGIN_BODY=$(jq -nc \
  --arg e "$SMOKE_EMAIL" \
  --arg p "$SMOKE_PASSWORD" \
  '{email:$e, password:$p}')

LOGIN_RESP=$(curl -fsS --max-time 10 \
  -X POST "$BASE_URL/api/v1/auth/login" \
  -H 'Content-Type: application/json' \
  -c "$JAR" \
  --data "$LOGIN_BODY") \
  || fail "auth/login no devolvió 2xx — verificar que el smoke user existe y password coincide" 3

NEEDS_TOTP=$(printf '%s' "$LOGIN_RESP" | jq -r '.needs_totp // false')
if [ "$NEEDS_TOTP" = "true" ]; then
  fail "el smoke user '$SMOKE_EMAIL' tiene 2FA activo — crear un usuario dedicado sin 2FA para smoke tests (Faro no soporta provisión de TOTP automática)" 3
fi

# Confirmar que la cookie aterrizó (cookie name vive en auth.rs::SESSION_COOKIE).
if ! grep -q $'\tfaro_session\t' "$JAR" 2>/dev/null && ! grep -q 'faro_session' "$JAR"; then
  fail "auth/login no setteó faro_session cookie. body=$LOGIN_RESP" 3
fi
note login "OK — faro_session cookie obtenida"

# ---------- 3) POST /api/v1/ingest/logs ----------
#
# Mandamos UN log con un marker único para poder identificarlo después en la
# query. service='faro-smoke' deja una huella reconocible en los dashboards.

MARKER="smoke-$(date -u +%Y%m%dT%H%M%SZ)-$$-$RANDOM"
TS=$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ)
INGEST_BODY=$(jq -nc \
  --arg svc "faro-smoke" \
  --arg msg "smoke-post-deploy marker=$MARKER" \
  --arg ts "$TS" \
  '{service:$svc, logs:[{timestamp:$ts, level:"INFO", message:$msg, attributes:{source:"smoke-post-deploy", marker:$marker}}]}' \
  --arg marker "$MARKER")

note ingest "POST $BASE_URL/api/v1/ingest/logs marker=$MARKER"
INGEST_RESP=$(curl -fsS --max-time 10 \
  -X POST "$BASE_URL/api/v1/ingest/logs" \
  -H "Authorization: Bearer $SMOKE_INGEST_TOKEN" \
  -H 'Content-Type: application/json' \
  --data "$INGEST_BODY") \
  || fail "ingest/logs no devolvió 2xx — token de proyecto inválido o ingest path roto" 4

ACCEPTED=$(printf '%s' "$INGEST_RESP" | jq -r '.accepted // 0')
PROJECT=$(printf '%s' "$INGEST_RESP" | jq -r '.project // "?"')
if [ "$ACCEPTED" -lt 1 ]; then
  fail "ingest accepted=$ACCEPTED esperaba >=1. body=$INGEST_RESP" 4
fi
note ingest "OK — accepted=$ACCEPTED project=$PROJECT"

# ---------- 4) GET /api/v1/logs (cookie del paso 2) — encuentra el log del paso 3 ----------
#
# El writer hace flush cada 750ms (FARO_BATCH_FLUSH_MS), después CH async_insert
# tarda otro tic. Polling con timeout largo para no flakear bajo carga.

note query "polling $BASE_URL/api/v1/logs?query=$MARKER (hasta ${SMOKE_TIMEOUT}s)"
deadline=$(( $(date +%s) + SMOKE_TIMEOUT ))
attempts=0
found=0
LAST_BODY=""
while [ "$(date +%s)" -lt "$deadline" ]; do
  attempts=$((attempts + 1))
  # URL-encoding básico — el marker es [a-zA-Z0-9-_] así que no necesita escape,
  # pero --data-urlencode lo hace robusto si en el futuro cambia el formato.
  Q_RESP=$(curl -fsS --max-time 10 \
    -b "$JAR" \
    -G "$BASE_URL/api/v1/logs" \
    --data-urlencode "query=$MARKER" \
    --data-urlencode "last_minutes=5" \
    --data-urlencode "limit=10" 2>/dev/null) || true
  LAST_BODY="$Q_RESP"
  if [ -n "$Q_RESP" ] && printf '%s' "$Q_RESP" | jq -e --arg m "$MARKER" '
        type == "array" and length > 0 and (map(.body // "") | any(contains($m)))
      ' >/dev/null 2>&1; then
    found=1
    break
  fi
  sleep 2
done
if [ "$found" -ne 1 ]; then
  fail "log con marker=$MARKER NO apareció en /api/v1/logs tras ${SMOKE_TIMEOUT}s y $attempts intentos. \
último body=$(printf '%s' "$LAST_BODY" | head -c 400)" 5
fi
note query "OK — log encontrado tras $attempts intento(s)"

printf '\nsmoke[OK] login + ingest + query + healthz/protocol pasaron contra %s\n' "$BASE_URL"
