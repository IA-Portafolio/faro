# Testing — la red de regresión

Esta guía describe **toda** la suite de tests del monorepo y cómo correrla de
un tirón. El objetivo es simple: **antes de mergear código nuevo, verificar que
nada de lo existente se rompió.** Cada función que ya funciona tiene (o debería
tener) un test que la fija; cuando llega una feature nueva, la suite es la que
avisa si rompiste un contrato previo.

## TL;DR — correr todo

```bash
scripts/test-all.sh            # todas las suites que el entorno permita
scripts/test-all.sh frontend cli   # sólo las nombradas
```

El runner corre cada suite, **salta con un aviso claro** las que no tienen el
toolchain instalado (no aborta el resto) y termina con un resumen. Devuelve
exit ≠ 0 si cualquier suite que sí se pudo correr falló — apto para usar como
pre-merge gate local.

Suites válidas: `backend cli frontend sdk-node sdk-nextjs sdk-expo sdk-python
sdk-go sdk-flutter sdk-kotlin`.

> Si seteas `CLICKHOUSE_URL` y ClickHouse responde, la suite `backend` corre
> **también** los integration tests de `backend/tests/*.rs`. Sin eso corre sólo
> los unit tests inline (`cargo test --lib`), que no necesitan base de datos.

## Mapa de la suite

| Componente | Qué cubre | Dónde viven los tests | Comando | Toolchain |
| --- | --- | --- | --- | --- |
| **backend** | lógica pura (redaction, rate-limit, fingerprint, feature-flags, auth, notify, workers…) + integration HTTP↔ClickHouse | `backend/src/**` (`#[cfg(test)]`) y `backend/tests/*.rs` | `cd backend && cargo test` (o `cargo nextest run`) | Rust |
| **cli** | parsing de flags, querystring, cookies, severidades, duraciones | `cli/src/main.rs` (`#[cfg(test)]`) | `cd cli && cargo test` | Rust |
| **frontend** | módulos puros de `lib/` (palette, stores, sessions, retention, insights, product-users, url-filters, toasts, keyboard, sdk-snippets, sdk-docs, api) **+ component tests** de Svelte (login) con `@testing-library/svelte` | `frontend/src/**/*.test.ts` (unit) y `frontend/src/**/*.component.test.ts` (components) | `cd frontend && npm test` | Node ≥ 20 |
| **sdk-node** | cliente, tracing OTel, métricas, middleware express | `sdks/node/test/*.test.mjs` | `cd sdks/node && npm test` | Node ≥ 18 |
| **sdk-nextjs** | RUM browser, web-vitals, replay, feature flags | `sdks/nextjs/test/*.test.mjs` | `cd sdks/nextjs && npm test` | Node ≥ 18 |
| **sdk-expo** | cliente RN, cola/flush, close sin pérdida | `sdks/expo/test/*.test.mjs` | `cd sdks/expo && npm test` | Node ≥ 18 |
| **sdk-python** | cliente, tracing, scrubbing, product events, helpers de trace-context | `sdks/python/tests/test_*.py` | `cd sdks/python && pytest` | Python ≥ 3.9 |
| **sdk-go** | cliente, tracing | `sdks/go/*_test.go` | `cd sdks/go && go test ./...` | Go |
| **sdk-flutter** | cliente | `sdks/flutter/test/*_test.dart` | `cd sdks/flutter && flutter test` | Flutter |
| **sdk-kotlin** | cliente | `sdks/kotlin/src/test/**` | `cd sdks/kotlin && ./gradlew test` | JDK + Gradle |

El frontend requiere **Node 20** (Vite 8 / Svelte 5); ver [`.nvmrc`](../.nvmrc).
El primer `npm test` corre `svelte-kit sync` para generar
`.svelte-kit/tsconfig.json` — sin eso, vitest falla al resolver el `tsconfig`.
`scripts/test-all.sh` ya lo hace por ti.

`npm test` corre **dos proyectos** de vitest en una sola pasada (ver
[`frontend/vitest.config.ts`](../frontend/vitest.config.ts)): `unit`
(environment node, sin plugin Svelte — los módulos puros, `src/**/*.test.ts`)
y `components` (jsdom + `@testing-library/svelte`, compila los `.svelte` —
`src/**/*.component.test.ts`). El sufijo importa: un component test que no
termine en `.component.test.ts` cae al proyecto `unit`, que no compila Svelte.

## Cómo se mapea a CI

- [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) — job `backend`
  (`cargo fmt --check` + `cargo clippy --all-targets -D warnings` + `cargo
  nextest run` contra un ClickHouse de servicio) y job `frontend`
  (`svelte-check` + `npm test`).
- [`.github/workflows/sdk-tests.yml`](../.github/workflows/sdk-tests.yml) — un
  job por SDK (node, nextjs, expo, python, go, flutter, kotlin).

`scripts/test-all.sh` replica esto localmente para que puedas reproducir un
fallo de CI sin esperar el runner.

## Reglas para que la red no se degrade

1. **Toda función nueva con lógica entra con su test.** Si es lógica pura
   (parsing, formato, cálculo, validación), un unit test al lado del código
   (`#[cfg(test)]` en Rust, `*.test.ts` / `test_*.py` / `*.test.mjs`) es lo
   más barato y rápido. Reserva los integration tests (`backend/tests/`) para
   lo que de verdad cruza la red o ClickHouse.
2. **Un test que existe pero no se corre no protege nada.** Si agregas un
   archivo de test a un SDK Node, asegúrate de que el glob de `npm test` lo
   incluya (`test/*.test.mjs`).
3. **El módulo de tests va al final del archivo** en Rust
   (`clippy::items_after_test_module` lo exige con `-D warnings`).
4. **No introduzcas tests flaky.** Nada de `Date.now()`/relojes reales sin
   `tokio::time::pause()` o timers falsos; nada que dependa de orden de
   ejecución o de variables de entorno globales compartidas entre tests.
5. **Antes de abrir el PR:** `scripts/test-all.sh` en verde (o al menos las
   suites que tu cambio toca).

## Estado actual (snapshot)

Cobertura verificada en verde al escribir esta guía:

- backend: **137** unit tests inline (`cargo test --lib`) + 21 binarios de
  integration tests (requieren ClickHouse).
- cli: **11** unit tests.
- frontend: **181** tests (16 archivos: 15 unit + 1 component test).
- sdk-node: **37** · sdk-nextjs: **24** · sdk-expo: **14** · sdk-python: **62**.
- sdk-go / sdk-flutter / sdk-kotlin: suites existentes (se corren en CI con su
  toolchain).
