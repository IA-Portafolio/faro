#!/usr/bin/env bash
# Restaura un backup de DATOS de ClickHouse creado por scripts/backup-clickhouse.sh.
#
# Uso:
#   bash scripts/restore-clickhouse.sh <ruta-al-tarball.tar.gz>
#
# Variables de entorno: las mismas que backup-clickhouse.sh
#   (FARO_CH_CONTAINER, CLICKHOUSE_USER, CLICKHOUSE_PASSWORD, CLICKHOUSE_DATABASE).
#
# Qué hace:
#   1. Extrae el tarball.
#   2. Aplica schema.sql (CREATE TABLE IF NOT EXISTS — idempotente).
#   3. Re-inserta los datos de cada tabla desde su dump Native.
#
# ADVERTENCIA: INSERT es ADITIVO. Para un restore limpio sobre datos existentes,
# truncá las tablas antes (o restaurá sobre una base vacía). Las tablas con engine
# ReplacingMergeTree colapsan duplicados por su clave de orden en el próximo merge,
# pero no asumas dedupe inmediato.
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "uso: $0 <tarball.tar.gz>" >&2
  exit 2
fi
TARBALL="$1"
[ -f "$TARBALL" ] || { echo "no existe: $TARBALL" >&2; exit 2; }

FARO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FARO_CH_CONTAINER="${FARO_CH_CONTAINER:-faro-clickhouse}"
CLICKHOUSE_USER="${CLICKHOUSE_USER:-faro}"
CLICKHOUSE_DATABASE="${CLICKHOUSE_DATABASE:-faro}"

if [ -z "${CLICKHOUSE_PASSWORD:-}" ] && [ -f "$FARO_DIR/.env.prod" ]; then
  CLICKHOUSE_PASSWORD="$(grep '^CLICKHOUSE_PASSWORD=' "$FARO_DIR/.env.prod" | cut -d= -f2- || true)"
fi
CLICKHOUSE_PASSWORD="${CLICKHOUSE_PASSWORD:-faro}"

ch() {
  docker exec -i "$FARO_CH_CONTAINER" clickhouse-client \
    --user="$CLICKHOUSE_USER" --password="$CLICKHOUSE_PASSWORD" \
    --database="$CLICKHOUSE_DATABASE" "$@"
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar -C "$work" -xzf "$TARBALL"
dir="$(find "$work" -maxdepth 1 -type d -name 'faro-data-*' | head -1)"
[ -n "$dir" ] || { echo "tarball sin directorio faro-data-*" >&2; exit 1; }

echo "[restore] recreando tablas base + insertando datos"
while IFS= read -r t; do
  [ -n "$t" ] || continue
  sf="$dir/schema/$t.sql"
  if [ -f "$sf" ]; then
    ch --multiquery < "$sf"
  fi
  f="$dir/$t.native.gz"
  if [ ! -f "$f" ]; then
    echo "[restore] AVISO: falta dump de $t, se omite" >&2
    continue
  fi
  # Una tabla sin filas produce un dump Native vacío; INSERT con stdin vacío da
  # NO_DATA_TO_INSERT. La tabla ya se creó arriba, así que solo saltamos el INSERT.
  if [ "$(gunzip -c "$f" | wc -c)" -eq 0 ]; then
    echo "[restore]   $t (vacía, solo schema)"
    continue
  fi
  echo "[restore]   $t"
  gunzip -c "$f" | ch --query "INSERT INTO \`$CLICKHOUSE_DATABASE\`.\`$t\` FORMAT Native"
done < "$dir/tables.txt"

# Materialized views al final (dependen de las tablas base ya creadas).
if [ -f "$dir/views.txt" ]; then
  echo "[restore] recreando materialized views"
  while IFS= read -r v; do
    [ -n "$v" ] || continue
    vf="$dir/views/$v.sql"
    [ -f "$vf" ] || continue
    echo "[restore]   $v"
    ch --multiquery < "$vf"
  done < "$dir/views.txt"
fi

echo "[restore] OK"
