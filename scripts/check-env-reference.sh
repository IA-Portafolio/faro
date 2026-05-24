#!/usr/bin/env bash
#
# Verifica que docs/reference/environment.md esté en sync con .env.example.
# CI falla el PR si la página generada difiere del archivo committeado.
#
# Uso local: `bash scripts/check-env-reference.sh`. Para regenerar:
#   bash scripts/gen-env-reference.sh

set -euo pipefail
cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

OUT="docs/reference/environment.md"

if [[ ! -f "$OUT" ]]; then
  echo "ERROR: falta $OUT. Corré: bash scripts/gen-env-reference.sh" >&2
  exit 1
fi

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

bash scripts/gen-env-reference.sh --to "$TMP" >/dev/null

if diff -u "$OUT" "$TMP" > /tmp/env-ref.diff 2>&1; then
  echo "OK — docs/reference/environment.md sincronizado con .env.example."
  exit 0
fi

echo "ERROR: $OUT está desincronizado con .env.example."
echo
echo "Diff (committeado → regenerado):"
echo
cat /tmp/env-ref.diff
echo
echo "Arregla con:"
echo "    bash scripts/gen-env-reference.sh"
echo "    git add $OUT"
exit 1
