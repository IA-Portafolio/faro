#!/usr/bin/env bash
# Setup + ejecución de integration tests del backend dentro del container de
# docker-compose.test.yml. Vivimos en un script separado porque ponerlo inline
# en el YAML pierde los newlines en algunos runtimes (Docker Desktop / WSL2)
# y bash interpreta todo como una sola línea, rompiendo el flow.
set -euo pipefail

echo "[setup] installing curl + pkg-config (for swagger-ui build.rs)"
apt-get update -qq
apt-get install -y -qq --no-install-recommends curl pkg-config

echo "[setup] applying clickhouse schema (script compartido, estricto)"
# El split de sentencias (una por POST, regla del HTTP API de CH) y la
# verificación de tablas centinela viven en scripts/apply-clickhouse-schema.sh,
# compartido con el job `backend` de ci.yml. Necesita el curl instalado arriba.
# El compose monta ./scripts:/scripts y ./clickhouse:/clickhouse, así que el
# SCHEMA_DIR default del script resuelve a /clickhouse acá adentro.
CLICKHOUSE_URL=http://clickhouse-test:8123 bash /scripts/apply-clickhouse-schema.sh

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
