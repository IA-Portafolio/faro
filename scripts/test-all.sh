#!/usr/bin/env bash
# Corre TODA la suite de regresión del monorepo en un solo comando.
#
# Objetivo: que antes de mergear código nuevo se pueda verificar de un tirón
# que nada de lo existente se rompió — backend (Rust), CLI (Rust), frontend
# (SvelteKit/vitest) y los SDKs (node, nextjs, expo, python, go, flutter,
# kotlin). Replica lo que hacen los workflows de CI (.github/workflows/ci.yml
# y sdk-tests.yml) pero localmente.
#
# Cada suite se salta con un aviso claro si le falta el toolchain (no aborta el
# resto): así el script sirve en una laptop con sólo Node, en un runner con
# todo instalado, o en CI. Devuelve exit != 0 si CUALQUIER suite que SÍ se pudo
# correr falló.
#
# Uso:
#   scripts/test-all.sh                 # todo lo que el entorno permita
#   scripts/test-all.sh frontend cli    # sólo las suites nombradas
#
# Suites: backend cli frontend sdk-core sdk-node sdk-nextjs sdk-expo
#         sdk-python sdk-go sdk-flutter sdk-kotlin
# (sdk-core = sdks/_shared/sdk-core: funciones compartidas + spec ejecutable
#  de paridad cross-SDK; corre antes que los SDKs TS que la consumen.)
#
# CLICKHOUSE_URL: si está seteada y CH responde, el backend corre TAMBIÉN los
# integration tests de backend/tests/*.rs. Si no, sólo los unit tests inline
# (cargo test --lib), que no necesitan ClickHouse.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [ -t 1 ]; then BOLD=$'\033[1m'; GREEN=$'\033[32m'; RED=$'\033[31m'; YEL=$'\033[33m'; DIM=$'\033[90m'; RST=$'\033[0m'
else BOLD=; GREEN=; RED=; YEL=; DIM=; RST=; fi

REQUESTED=("$@")
PASSED=(); FAILED=(); SKIPPED=()

have() { command -v "$1" >/dev/null 2>&1; }

# ¿Se pidió esta suite? Sin argumentos => todas.
wanted() {
  [ ${#REQUESTED[@]} -eq 0 ] && return 0
  local r; for r in "${REQUESTED[@]}"; do [ "$r" = "$1" ] && return 0; done
  return 1
}

# run <nombre> <cmd...> : corre el comando mostrando salida en vivo y registra resultado.
run() {
  local name="$1"; shift
  printf '\n%s━━━ %s ━━━%s\n' "$BOLD" "$name" "$RST"
  if "$@"; then
    PASSED+=("$name"); printf '%s✓ %s%s\n' "$GREEN" "$name" "$RST"
  else
    local rc=$?  # capturar ANTES de tocar el array (que resetea $?)
    FAILED+=("$name"); printf '%s✗ %s (exit %d)%s\n' "$RED" "$name" "$rc" "$RST"
  fi
}

skip() { SKIPPED+=("$1"); printf '\n%s━━━ %s ━━━%s\n%s⊘ saltado: %s%s\n' "$BOLD" "$1" "$RST" "$YEL" "$2" "$RST"; }

# ── Definición de cada suite ──────────────────────────────────────────
do_backend() {
  if ! have cargo; then skip backend "cargo no instalado"; return; fi
  if [ -n "${CLICKHOUSE_URL:-}" ] && curl -fsS "${CLICKHOUSE_URL%/}/ping" >/dev/null 2>&1; then
    run backend bash -c 'cd backend && cargo test --all-features --no-fail-fast'
  else
    printf '%s(sin ClickHouse → sólo unit tests inline; setea CLICKHOUSE_URL para los integration tests)%s\n' "$DIM" "$RST"
    run backend bash -c 'cd backend && cargo test --lib --no-fail-fast'
  fi
}
do_cli()      { have cargo && run cli bash -c 'cd cli && cargo test' || skip cli "cargo no instalado"; }
do_frontend() {
  if ! have npm; then skip frontend "npm no instalado"; return; fi
  run frontend bash -c 'cd frontend && { [ -d node_modules ] || npm install; } && npx svelte-kit sync && npm test'
}
do_sdk_core()   { have npm && run sdk-core   bash -c 'cd sdks/_shared/sdk-core && { [ -d node_modules ] || npm install; } && npm test' || skip sdk-core "npm no instalado"; }
do_sdk_node()   { have npm && run sdk-node   bash -c 'cd sdks/node   && { [ -d node_modules ] || npm install; }                   && npm test' || skip sdk-node "npm no instalado"; }
do_sdk_nextjs() { have npm && run sdk-nextjs bash -c 'cd sdks/nextjs && { [ -d node_modules ] || npm install --legacy-peer-deps; } && npm test' || skip sdk-nextjs "npm no instalado"; }
do_sdk_expo()   { have npm && run sdk-expo   bash -c 'cd sdks/expo   && { [ -d node_modules ] || npm install; }                   && npm test' || skip sdk-expo "npm no instalado"; }
do_sdk_python() { have python3 && run sdk-python bash -c 'cd sdks/python && python3 -m pytest -q' || skip sdk-python "python3 no instalado"; }
do_sdk_go()     { have go && run sdk-go bash -c 'cd sdks/go && go test ./...' || skip sdk-go "go no instalado"; }
do_sdk_flutter(){ have flutter && run sdk-flutter bash -c 'cd sdks/flutter && flutter pub get && flutter test' || skip sdk-flutter "flutter no instalado"; }
do_sdk_kotlin() {
  if [ -x sdks/kotlin/gradlew ]; then run sdk-kotlin bash -c 'cd sdks/kotlin && ./gradlew test --no-daemon'
  elif have gradle; then run sdk-kotlin bash -c 'cd sdks/kotlin && gradle test --no-daemon'
  else skip sdk-kotlin "gradle/gradlew no disponible"; fi
}

# ── Orquestación ──────────────────────────────────────────────────────
wanted backend     && do_backend
wanted cli         && do_cli
wanted frontend    && do_frontend
wanted sdk-core    && do_sdk_core
wanted sdk-node    && do_sdk_node
wanted sdk-nextjs  && do_sdk_nextjs
wanted sdk-expo    && do_sdk_expo
wanted sdk-python  && do_sdk_python
wanted sdk-go      && do_sdk_go
wanted sdk-flutter && do_sdk_flutter
wanted sdk-kotlin  && do_sdk_kotlin

# ── Resumen ───────────────────────────────────────────────────────────
printf '\n%s════════════ RESUMEN ════════════%s\n' "$BOLD" "$RST"
printf '%s  pasaron : %d%s  %s\n' "$GREEN" "${#PASSED[@]}"  "$RST" "${PASSED[*]:-—}"
printf '%s  fallaron: %d%s  %s\n' "$RED"   "${#FAILED[@]}"  "$RST" "${FAILED[*]:-—}"
printf '%s  saltadas: %d%s  %s\n' "$YEL"   "${#SKIPPED[@]}" "$RST" "${SKIPPED[*]:-—}"

[ ${#FAILED[@]} -eq 0 ]
