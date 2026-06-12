#!/usr/bin/env bash
# Verifica el bootstrap del schema en dos dimensiones:
#   1) FORWARD       — init/*.sql + migrations/*.sql aplican limpios sobre DB vacía.
#   2) IDEMPOTENCIA  — correr migrations/*.sql una SEGUNDA vez no produce errores.
# Finalmente confirma con SHOW TABLES que estén todas las tablas/MVs esperadas.
#
# Cubre el bug clásico de "una migración rompe en el segundo deploy" cuando
# alguien olvida un IF NOT EXISTS y la primera vez pasa porque la tabla aún no
# existía. Cubre también el caso de bootstrap fresh (init/ + migrations/ deben
# no colisionar entre sí).
#
# Usa `clickhouse-client` si está en PATH, sino cae a `docker run` con la imagen
# oficial. El --multiquery del client soporta múltiples sentencias por archivo
# (el HTTP API de CH NO lo soporta sin parsing manual del lado del cliente).
#
# Vars de entorno:
#   CLICKHOUSE_HOST       (default: localhost)
#   CLICKHOUSE_PORT       (default: 9000, TCP nativo)
#   CLICKHOUSE_USER       (default: faro)
#   CLICKHOUSE_PASSWORD   (default: faro)
#   CLICKHOUSE_DATABASE   (default: faro)
#   CLICKHOUSE_IMAGE      (default: clickhouse/clickhouse-server:24-alpine)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"

CH_HOST="${CLICKHOUSE_HOST:-localhost}"
CH_PORT="${CLICKHOUSE_PORT:-9000}"
CH_USER="${CLICKHOUSE_USER:-faro}"
CH_PASS="${CLICKHOUSE_PASSWORD:-faro}"
CH_DB="${CLICKHOUSE_DATABASE:-faro}"
CH_IMAGE="${CLICKHOUSE_IMAGE:-clickhouse/clickhouse-server:24-alpine}"

if command -v clickhouse-client >/dev/null 2>&1; then
  CLIENT=(clickhouse-client)
elif command -v docker >/dev/null 2>&1; then
  # --network=host para que el client llegue al puerto publicado en localhost
  # (funciona en Linux nativo y en GH Actions; en Docker Desktop Mac/Win
  # podría requerir mapear el host del compose en CLICKHOUSE_HOST).
  CLIENT=(docker run --rm -i --network=host "$CH_IMAGE" clickhouse-client)
else
  echo "ERROR: necesitas clickhouse-client o docker en PATH" >&2
  exit 1
fi

CH_ARGS=(
  --host="$CH_HOST"
  --port="$CH_PORT"
  --user="$CH_USER"
  --password="$CH_PASS"
  --database="$CH_DB"
)

ch_exec_file() { "${CLIENT[@]}" "${CH_ARGS[@]}" --multiquery < "$1"; }
ch_query()     { "${CLIENT[@]}" "${CH_ARGS[@]}" --query="$1"; }

echo "== Faro ClickHouse migration idempotency test =="
echo "target: $CH_USER@$CH_HOST:$CH_PORT db=$CH_DB"

# Sanity: la DB debe arrancar VACÍA, sino IF NOT EXISTS sería no-op desde el
# arranque y el test no demuestra nada. Para re-correr local, recrear la DB:
#   DROP DATABASE faro; CREATE DATABASE faro;
preexisting=$(ch_query "SELECT count() FROM system.tables WHERE database = '$CH_DB'" | tr -d '[:space:]')
if [[ "$preexisting" != "0" ]]; then
  echo "ERROR: se esperaba '$CH_DB' vacía, se encontraron $preexisting tablas." >&2
  echo "       Recrear la DB antes de correr (DROP DATABASE / CREATE DATABASE)." >&2
  exit 1
fi

echo
echo "-- step 1: applying init/*.sql (fresh bootstrap) --"
for f in "$ROOT/init"/*.sql; do
  echo "  $(basename "$f")"
  ch_exec_file "$f"
done

echo
echo "-- step 2: applying migrations/*.sql (pass 1) --"
for f in "$ROOT/migrations"/*.sql; do
  echo "  $(basename "$f")"
  ch_exec_file "$f"
done

echo
echo "-- step 3: applying migrations/*.sql (pass 2 → idempotency check) --"
for f in "$ROOT/migrations"/*.sql; do
  echo "  $(basename "$f")"
  ch_exec_file "$f"
done

echo
echo "-- step 4: verifying expected tables / MVs via SHOW TABLES --"
# Excluimos las tablas internas de MVs (.inner*): hoy todos los MVs usan TO
# explícito y no generan ninguna, pero si algún día aparece una no queremos
# que el check inverso explote por una tabla que no es nuestra.
actual=$(ch_query "SELECT name FROM system.tables WHERE database = '$CH_DB' AND name NOT LIKE '.inner%' ORDER BY name")
echo "found:"
echo "$actual" | sed 's/^/  /'

# Catálogo cerrado de tablas/MVs que init+migrations deben producir. El check
# es BIDIRECCIONAL: falla si falta una tabla esperada Y también si aparece una
# tabla que no está registrada acá. Si añadís una tabla nueva en una migración,
# tenés que sumarla a este array (en orden alfabético) o el test va a fallar.
EXPECTED=(
  alert_incidents
  alert_rules
  api_monitors
  cohorts
  error_clusters
  error_events
  error_issue_status
  errors_hourly
  feature_flags
  integrations
  logs
  logs_stats
  metrics
  monitor_results
  monitor_uptime_daily
  mv_errors_hourly
  mv_logs_stats
  mv_monitor_uptime_daily
  mv_product_events_per_day
  mv_product_unique_users_per_day
  mv_services_seen_logs
  mv_services_seen_metrics
  mv_services_seen_spans
  mv_spans_latency_hourly
  mv_traces_index
  notification_channels
  product_events
  product_events_per_day
  product_sessions
  product_unique_users_per_day
  product_user_aliases
  product_users
  projects
  service_stale_events
  services_seen
  session_replays
  spans
  spans_latency_hourly
  traces_index
  user_login_challenges
  user_preferences
  user_recovery_codes
  user_sessions
  users
)

missing=()
for t in "${EXPECTED[@]}"; do
  if ! grep -qx "$t" <<<"$actual"; then
    missing+=("$t")
  fi
done

if [[ ${#missing[@]} -gt 0 ]]; then
  echo "ERROR: faltan ${#missing[@]} tablas/MVs:" >&2
  printf '  - faro.%s\n' "${missing[@]}" >&2
  exit 1
fi

# Check inverso: ninguna tabla real puede quedar fuera del catálogo, sino el
# "catálogo cerrado" sería puro verso (una tabla nueva pasaría en silencio).
unexpected=()
while IFS= read -r t; do
  [[ -z "$t" ]] && continue
  found=0
  for e in "${EXPECTED[@]}"; do
    if [[ "$t" == "$e" ]]; then
      found=1
      break
    fi
  done
  if [[ $found -eq 0 ]]; then
    unexpected+=("$t")
  fi
done <<<"$actual"

if [[ ${#unexpected[@]} -gt 0 ]]; then
  echo "ERROR: ${#unexpected[@]} tablas no registradas en el catálogo — agregalas a EXPECTED en test-migrations.sh:" >&2
  printf '  - faro.%s\n' "${unexpected[@]}" >&2
  exit 1
fi

echo
echo "OK: migraciones idempotentes, ${#EXPECTED[@]} tablas/MVs esperadas presentes."
