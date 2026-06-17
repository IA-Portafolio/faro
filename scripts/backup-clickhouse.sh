#!/usr/bin/env bash
# Backup de los DATOS de ClickHouse de Faro (no del código).
#
# ¿Por qué existe? Faro corría en producción SIN ningún backup de datos: el único
# estado durable es el volumen `clickhouse_data`. Un `docker volume rm`, un
# `make reset` (down -v) o un disco lleno destruían toda la telemetría/usuarios sin
# punto de restauración. Este script crea un dump portable y, opcionalmente, lo
# sincroniza OFF-HOST (un volumen único no replicado NO es un backup).
#
# Diseño (deliberadamente no invasivo):
#   * NO modifica docker-compose ni la config de ClickHouse ni reinicia el contenedor.
#   * Usa `clickhouse-client` vía `docker exec` y streamea cada tabla en formato
#     Native por stdout → host (gzip). Restaurable con scripts/restore-clickhouse.sh.
#   * Sin dependencias extra (no requiere `clickhouse-backup` ni un disco de backup
#     configurado en CH).
#
# Uso:
#   bash scripts/backup-clickhouse.sh
#
# Variables de entorno:
#   FARO_CH_CONTAINER   contenedor de ClickHouse           (default: faro-clickhouse)
#   CLICKHOUSE_USER     usuario CH                          (default: faro)
#   CLICKHOUSE_PASSWORD password CH (si no, se lee de .env.prod)
#   CLICKHOUSE_DATABASE base                                (default: faro)
#   FARO_BACKUP_DIR     dir local de salida                 (default: <repo>/backups)
#   FARO_BACKUP_KEEP    cuántos backups locales conservar   (default: 7)
#   FARO_BACKUP_REMOTE  destino off-host. Si está seteado se sincroniza el tarball.
#                       Soporta:
#                         - rsync/scp: usuario@host:/ruta/   (se usa `rsync -av`)
#                         - S3:        s3://bucket/prefijo/   (se usa `aws s3 cp`)
#                       Si NO está seteado, el backup queda SOLO local (se avisa).
set -euo pipefail

FARO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FARO_CH_CONTAINER="${FARO_CH_CONTAINER:-faro-clickhouse}"
CLICKHOUSE_USER="${CLICKHOUSE_USER:-faro}"
CLICKHOUSE_DATABASE="${CLICKHOUSE_DATABASE:-faro}"
FARO_BACKUP_DIR="${FARO_BACKUP_DIR:-$FARO_DIR/backups}"
FARO_BACKUP_KEEP="${FARO_BACKUP_KEEP:-7}"
FARO_BACKUP_REMOTE="${FARO_BACKUP_REMOTE:-}"

# Password: env var explícita, o leída de .env.prod (igual que deploy.yml).
if [ -z "${CLICKHOUSE_PASSWORD:-}" ] && [ -f "$FARO_DIR/.env.prod" ]; then
  CLICKHOUSE_PASSWORD="$(grep '^CLICKHOUSE_PASSWORD=' "$FARO_DIR/.env.prod" | cut -d= -f2- || true)"
fi
CLICKHOUSE_PASSWORD="${CLICKHOUSE_PASSWORD:-faro}"

# Helper: ejecuta una query en el CH del contenedor y devuelve el resultado por stdout.
ch() {
  docker exec -i "$FARO_CH_CONTAINER" clickhouse-client \
    --user="$CLICKHOUSE_USER" --password="$CLICKHOUSE_PASSWORD" \
    --database="$CLICKHOUSE_DATABASE" "$@"
}

ts="$(date -u +%Y%m%d-%H%M%S)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/faro-data-$ts"
mkdir -p "$stage"

echo "[backup] contenedor=$FARO_CH_CONTAINER db=$CLICKHOUSE_DATABASE -> $FARO_BACKUP_DIR"

# Tablas base (familia MergeTree). Se excluyen Views/MaterializedView (se regeneran
# desde las tablas base) y cualquier engine no persistente.
mapfile -t tables < <(ch --query \
  "SELECT name FROM system.tables WHERE database = '$CLICKHOUSE_DATABASE' \
   AND engine LIKE '%MergeTree%' ORDER BY name")

if [ "${#tables[@]}" -eq 0 ]; then
  echo "[backup] FATAL: no se encontraron tablas MergeTree en '$CLICKHOUSE_DATABASE'" >&2
  exit 1
fi

echo "[backup] ${#tables[@]} tablas a respaldar"
# Schema POR-TABLA (un archivo por tabla): aplicar cada DDL por separado en el
# restore es más robusto que concatenarlos — cada SHOW CREATE es exactamente lo que
# CH acepta, sin riesgo de que la unión rompa el parser.
mkdir -p "$stage/schema"
for t in "${tables[@]}"; do
  echo "  - $t"
  # TabSeparatedRaw: DDL con saltos de línea REALES (sin escapar `\n`). Inyectamos
  # `IF NOT EXISTS` (solo en la 1ª línea) para que el restore sea idempotente y no
  # aborte si la tabla ya existe (restaurar sobre un CH con schema es lo normal).
  ch --query "SHOW CREATE TABLE \`$CLICKHOUSE_DATABASE\`.\`$t\`" --format=TabSeparatedRaw \
    | sed '1s/^CREATE TABLE /CREATE TABLE IF NOT EXISTS /' \
    > "$stage/schema/$t.sql"
  # Dump de datos en formato Native (binario, exacto, restaurable 1:1) + gzip.
  ch --query "SELECT * FROM \`$CLICKHOUSE_DATABASE\`.\`$t\` FORMAT Native" \
    | gzip > "$stage/$t.native.gz"
done

# Materialized views: sólo el DDL (sus datos viven en las tablas destino, ya
# respaldadas arriba). Se recrean DESPUÉS de las tablas base en el restore para que
# la agregación siga poblándose tras restaurar.
mapfile -t views < <(ch --query \
  "SELECT name FROM system.tables WHERE database = '$CLICKHOUSE_DATABASE' \
   AND engine = 'MaterializedView' ORDER BY name")
mkdir -p "$stage/views"
for v in "${views[@]:-}"; do
  [ -n "$v" ] || continue
  ch --query "SHOW CREATE TABLE \`$CLICKHOUSE_DATABASE\`.\`$v\`" --format=TabSeparatedRaw \
    | sed '1s/^CREATE MATERIALIZED VIEW /CREATE MATERIALIZED VIEW IF NOT EXISTS /' \
    > "$stage/views/$v.sql"
done
echo "[backup] ${#views[@]} materialized views (sólo DDL)"

printf '%s\n' "${tables[@]}" > "$stage/tables.txt"
printf '%s\n' "${views[@]:-}" > "$stage/views.txt"
echo "faro_backup_version=1" > "$stage/manifest.txt"
echo "created_utc=$ts" >> "$stage/manifest.txt"
echo "database=$CLICKHOUSE_DATABASE" >> "$stage/manifest.txt"

mkdir -p "$FARO_BACKUP_DIR"
tarball="$FARO_BACKUP_DIR/faro-data-$ts.tar.gz"
tar -C "$work" -czf "$tarball" "faro-data-$ts"
echo "[backup] OK: $tarball ($(du -h "$tarball" | cut -f1))"

# Retención local: conservar los últimos N tarballs de DATOS (no toca los de código
# source-*.tar.gz que usa el rollback de deploy).
mapfile -t old < <(ls -1t "$FARO_BACKUP_DIR"/faro-data-*.tar.gz 2>/dev/null | tail -n +$((FARO_BACKUP_KEEP + 1)) || true)
for f in "${old[@]:-}"; do
  [ -n "$f" ] || continue
  echo "[backup] retención: borrando $f"
  rm -f "$f"
done

# Sync off-host (un volumen único no replicado NO es un backup).
if [ -n "$FARO_BACKUP_REMOTE" ]; then
  echo "[backup] sincronizando off-host -> $FARO_BACKUP_REMOTE"
  case "$FARO_BACKUP_REMOTE" in
    s3://*)
      aws s3 cp "$tarball" "$FARO_BACKUP_REMOTE" ;;
    *)
      rsync -av "$tarball" "$FARO_BACKUP_REMOTE" ;;
  esac
  echo "[backup] off-host OK"
else
  echo "[backup] AVISO: FARO_BACKUP_REMOTE no seteado — el backup quedó SOLO en el host." >&2
  echo "[backup]        Un fallo de disco/host lo pierde. Configurá FARO_BACKUP_REMOTE." >&2
fi
