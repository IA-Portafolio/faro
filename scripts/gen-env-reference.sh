#!/usr/bin/env bash
#
# Genera docs/reference/environment.md a partir de `.env.example`.
#
# `.env.example` es la fuente única de verdad para variables de entorno
# de Faro. README, docs/deployment.md, infra/README.md, etc. linkean a la
# página generada en lugar de duplicar tablas que se desincronizan.
#
# Formato esperado en `.env.example`:
#   - Secciones: línea `# ---- Nombre de sección ----`.
#   - Cada variable va precedida por 1+ líneas de comentario `# ...` que
#     describen su propósito. Una línea en blanco separa variables.
#   - Variable activa: `VAR=valor`.
#   - Variable opcional (default desde código): `# VAR=valor`.
#
# Uso:
#   bash scripts/gen-env-reference.sh                # escribe el .md
#   bash scripts/gen-env-reference.sh --to /tmp/x.md # a otra ruta
#   bash scripts/gen-env-reference.sh --stdout       # imprime a stdout
#
# Validación en CI: `scripts/check-env-reference.sh`.

set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

SRC=".env.example"
OUT="docs/reference/environment.md"
MODE="file"  # file | stdout

while [[ $# -gt 0 ]]; do
  case "$1" in
    --to)      OUT="$2"; shift 2 ;;
    --stdout)  MODE="stdout"; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *)         echo "Argumento desconocido: $1" >&2; exit 2 ;;
  esac
done

if [[ ! -f "$SRC" ]]; then
  echo "ERROR: no encontré $SRC en $(pwd)" >&2
  exit 1
fi

render() {
  awk '
    BEGIN {
      section = ""
      comment = ""
      in_section = 0
    }

    # Header de sección.
    /^# ---- .+ ---- *$/ {
      line = $0
      sub(/^# ---- /, "", line)
      sub(/ ---- *$/, "", line)
      section = line

      if (in_section) print ""
      print "## " section
      print ""
      print "| Variable | Default | Descripción |"
      print "| -------- | ------- | ----------- |"
      in_section = 1
      comment = ""
      next
    }

    # Línea en blanco resetea el acumulador de comentarios.
    /^[[:space:]]*$/ { comment = ""; next }

    # Variable comentada (opcional / default desde código): `# VAR=valor`.
    /^# *[A-Z][A-Z0-9_]*=/ {
      if (!in_section) next
      line = $0
      sub(/^# */, "", line)
      name = line
      sub(/=.*/, "", name)
      val = line
      sub(/^[^=]+=/, "", val)
      if (val == "") {
        val_md = "_(sin setear)_ · opcional"
      } else {
        val_md = "`" val "` · opcional"
      }
      desc = (comment == "" ? "—" : comment)
      printf "| `%s` | %s | %s |\n", name, val_md, desc
      comment = ""
      next
    }

    # Línea de comentario normal (descripción del próximo var).
    /^#/ {
      line = $0
      sub(/^# ?/, "", line)
      if (comment == "") comment = line
      else comment = comment " " line
      next
    }

    # Variable activa: `VAR=valor`.
    /^[A-Z][A-Z0-9_]*=/ {
      if (!in_section) next
      name = $0
      sub(/=.*/, "", name)
      val = $0
      sub(/^[^=]+=/, "", val)
      if (val == "") {
        val_md = "_(vacío)_"
      } else {
        val_md = "`" val "`"
      }
      desc = (comment == "" ? "—" : comment)
      printf "| `%s` | %s | %s |\n", name, val_md, desc
      comment = ""
      next
    }
  ' "$SRC"
}

emit() {
  cat <<'HEADER'
<!--
  AUTOGENERADO por scripts/gen-env-reference.sh a partir de .env.example.
  NO EDITAR A MANO. Cambiá .env.example y corré:
      bash scripts/gen-env-reference.sh
-->

# Reference · Variables de entorno

Esta página enumera **todas** las variables de entorno que entienden el
backend de Faro, el `docker-compose` y los scripts de operación. Se
genera automáticamente desde [`.env.example`](../../.env.example), que
es la fuente única de verdad — README, `docs/deployment.md`,
`infra/README.md` y los templates de prod linkean acá en lugar de
mantener sus propias tablas.

Convenciones de la columna **Default**:

- `` `valor` `` — variable activa en `.env.example`; ese es el valor
  efectivo si copiás el archivo a `.env` sin tocarlo.
- `` `valor` · opcional`` — variable comentada en `.env.example`; el
  default lo aplica el código (`backend/src/config.rs`,
  `backend/src/telemetry.rs` o el propio `docker-compose.yml`).
  Descomentala sólo para anular el default.
- _(vacío)_ / _(sin setear)_ — la variable no se setea por defecto y el
  código activa el comportamiento "opcional" correspondiente
  (ej. `FARO_METRICS_TOKEN` deja `/metrics` abierto).

Para añadir una variable nueva: edita
[`.env.example`](../../.env.example), corre
`bash scripts/gen-env-reference.sh` y commitea la página resultante. CI
falla el PR si los dos archivos están desincronizados.

HEADER
  render
}

if [[ "$MODE" == "stdout" ]]; then
  emit
else
  mkdir -p "$(dirname "$OUT")"
  emit > "$OUT"
  echo "Generado $OUT a partir de $SRC"
fi
