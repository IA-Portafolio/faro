#!/usr/bin/env bash
#
# Detección de docs huérfanas.
#
# Verifica que cada archivo bajo `docs/` esté enlazado desde algún índice,
# sidebar o doc raíz del repo. Un archivo en docs/ que nadie referencia es,
# en la práctica, invisible — y por tanto como si no existiera.
#
# Reglas:
#   - "Doc" = cualquier archivo bajo docs/ con extensión común de
#     documentación (.md/.mdx/.png/.svg/.jpg/.jpeg/.gif/.webp).
#   - "Índice" = cualquier .md/.mdx del repo fuera de node_modules y
#     directorios de build/artefactos.
#   - Un doc cuenta como enlazado si su basename o su ruta repo-relativa
#     aparece en algún índice distinto de sí mismo. Mencionar el nombre del
#     archivo en prosa también cuenta: en Markdown renderizado es visible y
#     da pie a que un lector lo busque.
#
# Salida:
#   - exit 0 si todos los docs están referenciados.
#   - exit 1 con la lista de huérfanos en stdout en caso contrario.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

# 1) Catálogo de archivos bajo docs/ que esperamos ver referenciados.
#    Excluimos docs/superpowers/** : son specs/plans de diseño archivados de
#    sesiones puntuales, no documentación de navegación que deba indexarse.
mapfile -t DOC_FILES < <(
  find docs -type f \
    -not -path 'docs/superpowers/*' \
    \( -iname '*.md'   -o -iname '*.mdx'  -o -iname '*.png' \
    -o -iname '*.svg'  -o -iname '*.jpg'  -o -iname '*.jpeg' \
    -o -iname '*.gif'  -o -iname '*.webp' \) \
    | sed 's|^\./||' \
    | sort
)

# 2) Posibles índices/sidebars: cualquier .md/.mdx fuera de directorios
#    ruidosos (deps, builds, artefactos). Usar -prune en find para no
#    recorrer node_modules — son cientos de miles de archivos.
mapfile -t INDEX_FILES < <(
  find . \
    \( -type d \( \
        -name node_modules -o -name target -o -name dist -o -name build \
        -o -name .git -o -name .svelte-kit -o -name .next -o -name out \
        -o -name coverage -o -name .turbo -o -name .cache \
      \) -prune \) \
    -o -type f \( -iname '*.md' -o -iname '*.mdx' \) -print \
    | sed 's|^\./||' \
    | sort
)

if [[ ${#DOC_FILES[@]} -eq 0 ]]; then
  echo "No se encontraron docs bajo docs/."
  exit 0
fi

orphans=()
for doc in "${DOC_FILES[@]}"; do
  base="$(basename "$doc")"
  found_in=""
  for idx in "${INDEX_FILES[@]}"; do
    [[ "$idx" == "$doc" ]] && continue
    # grep -F: el basename ('0001-...md', 'architecture.png') o la ruta
    # repo-relativa exacta ('docs/adr/0001-...md') deben aparecer literal.
    if grep -qF -e "$base" -e "$doc" "$idx"; then
      found_in="$idx"
      break
    fi
  done
  if [[ -z "$found_in" ]]; then
    orphans+=("$doc")
  fi
done

if [[ ${#orphans[@]} -eq 0 ]]; then
  echo "OK — los ${#DOC_FILES[@]} archivos de docs/ están enlazados."
  exit 0
fi

echo "Docs huérfanas (no enlazadas desde ningún índice del repo):"
for o in "${orphans[@]}"; do
  echo "  - $o"
done
echo
echo "Total: ${#orphans[@]} de ${#DOC_FILES[@]} archivos sin referencia."
echo
echo "Arregla enlazándolas desde un índice (README.md raíz, docs/adr/README.md,"
echo "CONTRIBUTING.md, etc.) o borrándolas si ya no aportan."
exit 1
