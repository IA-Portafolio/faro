#!/usr/bin/env bash
# Setup + ejecución de integration tests del backend dentro del container de
# docker-compose.test.yml. Vivimos en un script separado porque ponerlo inline
# en el YAML pierde los newlines en algunos runtimes (Docker Desktop / WSL2)
# y bash interpreta todo como una sola línea, rompiendo el flow.
set -euo pipefail

echo "[setup] installing curl + pkg-config (for swagger-ui build.rs)"
apt-get update -qq
apt-get install -y -qq --no-install-recommends curl pkg-config

echo "[setup] applying clickhouse schema (one statement per request)"
# CH HTTP por default ejecuta una sola sentencia por POST. Los .sql del repo
# tienen varias CREATE TABLE / MV separadas por `;`. Quitamos TODOS los
# comentarios `--` (de línea completa E inline) ANTES de colapsar los saltos de
# línea: si no, un `-- comentario` inline (p. ej. en 60-alerts.sql) se come el
# resto de la sentencia al unir las líneas con `tr` y la tabla nunca se crea.
# Ningún `--` aparece dentro de literales de cadena en el schema, así que
# borrar de `--` al fin de línea es seguro. Luego splitéo por `;`.
for f in /clickhouse/init/*.sql /clickhouse/migrations/*.sql; do
  [ -f "$f" ] || continue
  echo "  applying $(basename "$f")"
  sed 's/--.*$//' "$f" | tr '\n' ' ' | tr ';' '\n' \
    | while IFS= read -r stmt; do
        trimmed=$(echo "$stmt" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
        if [ -n "$trimmed" ]; then
          resp=$(curl -sS -u faro:faro --data-binary "$trimmed" \
            "http://clickhouse-test:8123/?database=faro" 2>&1) || \
            echo "    warn: $resp"
        fi
      done
done

echo "[setup] verifying schema (faro.logs must exist)"
curl -sSf -u faro:faro \
  --data-binary "SELECT count() FROM faro.logs" \
  "http://clickhouse-test:8123/?database=faro" \
  || { echo "FATAL: faro.logs not present after schema bootstrap" >&2; exit 1; }

echo "[setup] installing cargo-nextest (binario precompilado, ~5 s)"
# nextest paraleliza los 11 binarios de tests CROSS-BINARY usando un único
# pool; `cargo test` los serializa. Con la fixture de tests/common/mod.rs
# que aísla por project_id, los tests son seguros de correr concurrentes
# contra el mismo CH. Config: backend/.config/nextest.toml.
if ! command -v cargo-nextest >/dev/null 2>&1; then
  curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C "${CARGO_HOME:-$HOME/.cargo}/bin"
fi

echo "[setup] running cargo nextest (lib + todas las integration suites)"
# Antes excluíamos workers_* y stream_* con un comment "en progreso"; hoy
# todos compilan y pasan, así que dejamos que nextest auto-descubra todo.
exec cargo nextest run --all-features --no-fail-fast
