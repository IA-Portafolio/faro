# CLAUDE.md — Faro

## ⛔ ANTES DE TOCAR CÓDIGO: leé y obedecé [`AGENTS.md`](AGENTS.md)

[`AGENTS.md`](AGENTS.md) es la política de testing **vinculante** para todo agente
LLM en este monorepo (estándar cross-tool: Cursor, Codex, Claude Code…). Es
canónica y exhaustiva: mapeo cambio→comando, gates de backend/frontend/SDK,
ClickHouse, "docs que son código", la lista completa de excusas prohibidas y el
checklist de Terminado. **Leela y seguila.**

Lo de abajo es el **protocolo mínimo no-negociable restated inline**, para que
tenga dientes aunque no abras `AGENTS.md`. No lo sustituye — lo resume.

---

## 🔴 REGLA NO-NEGOCIABLE

**Ningún cambio de código está terminado hasta que pegues la salida REAL,
LITERAL y POSTERIOR de los tests, en VERDE, de TODAS las suites que tu cambio
toca.** Terminado = *cambio* + *suites en verde* + *evidencia pegada*. Decir
"tests en verde" / "listo" / "compila" **sin pegar la salida** = no terminado.

## El comando

```bash
bash /opt/faro/scripts/test-all.sh <suite...>   # las suites que tu cambio toca
bash /opt/faro/scripts/test-all.sh              # todas las que el entorno permita
```

Suites: `backend cli frontend sdk-node sdk-nextjs sdk-expo sdk-python sdk-go
sdk-flutter sdk-kotlin`. Salta (no falla) las suites sin toolchain; exit ≠ 0 si
alguna que SÍ corrió falló.

## Qué corre cada cambio (regla: ¿qué suite ejercita el código que toqué?)

- `backend/src|tests/**` → `cd backend && cargo test`
  **+ obligatorio:** `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings`.
  Si toca handlers/queries/schema → levantá ClickHouse y corré integration
  (`docker compose -f docker-compose.test.yml -p faro-test up --abort-on-container-exit --exit-code-from backend-test`); `cargo test --lib` solo NO basta.
- `cli/src/**` → `cd cli && cargo test` (+ fmt + clippy).
- `frontend/src/lib/**` → `cd frontend && npm test` (+ `npm run check` + `npm run build` para CI).
- `sdks/<x>/**` → node/nextjs/expo: `npm test` · python: `python3 -m pytest -q`
  · go: `go test ./...` · flutter: `flutter test` · kotlin: `./gradlew test`.

Tocar el **código fuente** obliga a su suite aunque no hayas tocado el test.

## Evidencia (pegala literal)

Pegá la cola REAL del runner (no parafraseada, no de memoria, posterior a tu
última edición) + `echo EXIT=$?`:
`test result: ok. N passed` / `N passed` / `N passed in Xs` / `All tests passed!`
/ `BUILD SUCCESSFUL`, y el bloque **RESUMEN completo** (`pasaron/fallaron/saltadas`)
de `test-all.sh`. **Fabricar o reconstruir salida es la falta más grave.**

## VERDE de verdad (no te engañes con el RESUMEN)

`fallaron: 0` NO es evidencia por sí solo: el runner da `fallaron: 0` aunque
TODO se haya **saltado** (`pasaron: 0`). En este entorno faltan `cargo`, `go`,
`flutter` y el módulo `pytest`. Verde = la suite que tocaste aparece en
**`pasaron:`**, no en `saltadas:` ni `fallaron:`. Por cada suite tocada:
`Suite de MI cambio: <x> → pasó`.

## No hay excusa (restated — detalle completo en AGENTS.md §7)

PROHIBIDO cerrar sin correr y pegar evidencia por: **trivial / una línea**,
**solo refactor o rename**, **solo docs** (`sdk-docs.ts`, `.env.example`, `docs/**`
son código con gates), **ya pasaban** (corré post-edit), **es lento** (acotá con
`<suite...>`), **no hay toolchain** (verificá con `command -v`; toolchain parcial
como `pytest` ausente = ROJO; instalá o corré en Docker; si la suite tocada no
corre → BLOQUEADO, no "hecho"), **falló por entorno/flaky/preexistente** (resolvé
el setup o probá contra el estado base). **No es repo git**: enumerá a mano tus
archivos y mapealos; ante la duda, `test-all.sh` completo.

## Antes de declarar Terminado

Suite(s) tocada(s) en `pasaron:` con salida literal pegada · backend/cli con
fmt+clippy en verde · integration corrido si tocaste esa capa · suites no
corridas declaradas `NO CORRIDO — <suite>: <motivo>` · 5 reglas anti-degradación
(§5 de AGENTS.md) cumplidas y el test nuevo visible en el conteo. Detalle y
checklist completo: [`AGENTS.md`](AGENTS.md) §10. Fuente de verdad de comandos:
[`docs/testing.md`](docs/testing.md).
