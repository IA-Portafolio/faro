#!/usr/bin/env bash
# Bootstrap ESTRICTO del schema de ClickHouse para tests: aplica
# clickhouse/init/*.sql + clickhouse/migrations/*.sql sentencia por sentencia.
#
# ¿Por qué existe? El HTTP API de CH ejecuta UNA sola sentencia por POST. Los
# .sql del repo tienen varias CREATE TABLE / MV separadas por `;`, así que
# postear el archivo entero (curl --data-binary "@$f") aplica solo la primera
# sentencia y descarta el resto en silencio → schema incompleto y tests que
# "pasan" contra tablas que no existen.
#
# ¿Quién lo usa?
#   * .github/workflows/ci.yml, job `backend`, step "Bootstrap ClickHouse
#     schema" (CH del service container en localhost:18123).
#   * scripts/run-integration-tests.sh, dentro del container backend-test de
#     docker-compose.test.yml (CH en clickhouse-test:8123). El compose monta
#     ./scripts:/scripts y ./clickhouse:/clickhouse, por eso el default de
#     SCHEMA_DIR (relativo a este script) resuelve bien en ambos mundos.
#
# Asunción documentada: ningún `;` ni `--` aparece dentro de literales de
# cadena en los .sql del schema. Si algún día se mete uno, el split naive de
# acá lo rompe — agregalo sin comillas raras o cambiá el splitter.
#
# Estricto = cada sentencia va con `curl --fail-with-body` (curl >= 7.76,
# presente en el runner Ubuntu y en rust:1-bookworm): cualquier error de CH
# imprime la sentencia + el body del error y corta con exit 1. Nada de `|| true`.
set -euo pipefail

CLICKHOUSE_URL="${CLICKHOUSE_URL:-http://localhost:8123}"
CLICKHOUSE_USER="${CLICKHOUSE_USER:-faro}"
CLICKHOUSE_PASSWORD="${CLICKHOUSE_PASSWORD:-faro}"
CLICKHOUSE_DATABASE="${CLICKHOUSE_DATABASE:-faro}"
# Default relativo al script: /opt/faro/clickhouse en el host, /clickhouse
# dentro del container de docker-compose.test.yml (script vive en /scripts).
SCHEMA_DIR="${SCHEMA_DIR:-$(cd "$(dirname "$0")/../clickhouse" && pwd)}"

echo "[schema] applying $SCHEMA_DIR/{init,migrations}/*.sql against $CLICKHOUSE_URL (db=$CLICKHOUSE_DATABASE)"

for f in "$SCHEMA_DIR"/init/*.sql "$SCHEMA_DIR"/migrations/*.sql; do
  [ -f "$f" ] || continue
  echo "  applying $(basename "$f")"
  # CH HTTP por default ejecuta una sola sentencia por POST. Los .sql del repo
  # tienen varias CREATE TABLE / MV separadas por `;`. Quitamos TODOS los
  # comentarios `--` (de línea completa E inline) ANTES de colapsar los saltos
  # de línea: si no, un `-- comentario` inline (p. ej. en 60-alerts.sql) se
  # come el resto de la sentencia al unir las líneas con `tr` y la tabla nunca
  # se crea. Ningún `--` aparece dentro de literales de cadena en el schema,
  # así que borrar de `--` al fin de línea es seguro. Luego splitéo por `;`.
  #
  # OJO subshell: el `while` corre en un subshell por el pipe, pero al ser el
  # ÚLTIMO comando del pipeline su exit status ES el del pipeline, y con
  # `set -euo pipefail` un `exit 1` adentro aborta el script entero.
  sed 's/--.*$//' "$f" | tr '\n' ' ' | tr ';' '\n' \
    | while IFS= read -r stmt || [ -n "$stmt" ]; do
        trimmed=$(echo "$stmt" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
        if [ -n "$trimmed" ]; then
          if ! resp=$(curl -sS --fail-with-body -u "$CLICKHOUSE_USER:$CLICKHOUSE_PASSWORD" \
              --data-binary "$trimmed" \
              "$CLICKHOUSE_URL/?database=$CLICKHOUSE_DATABASE" 2>&1); then
            echo "FATAL: sentencia rechazada por ClickHouse en $(basename "$f"):" >&2
            echo "  stmt: ${trimmed:0:200}" >&2
            echo "  resp: $resp" >&2
            exit 1
          fi
        fi
      done
done

# Verificación final con tablas centinela: `logs` (init/), `cohorts` y
# `feature_flags` (últimas migraciones que crean tablas). No reemplaza el gate
# exhaustivo de catálogo de clickhouse/test-migrations.sh — acá alcanza porque
# cada sentencia de arriba ya es estricta; esto solo ataja un schema vacío o
# un SCHEMA_DIR mal apuntado.
echo "[schema] verifying sentinel tables (logs, cohorts, feature_flags)"
for table in logs cohorts feature_flags; do
  count=$(curl -sS --fail-with-body -u "$CLICKHOUSE_USER:$CLICKHOUSE_PASSWORD" \
    --data-binary "SELECT count() FROM system.tables WHERE database = '$CLICKHOUSE_DATABASE' AND name = '$table'" \
    "$CLICKHOUSE_URL/")
  if [ "$count" != "1" ]; then
    echo "FATAL: tabla centinela '$CLICKHOUSE_DATABASE.$table' no existe tras el bootstrap" >&2
    exit 1
  fi
done

echo "[schema] OK: schema aplicado y centinelas presentes"
