# M-5 — God-files y paridad cross-SDK

> **Para agentes:** este plan cubre el ítem M-5 del audit (god-files + dedup SDKs).
> Pasos con checkbox (`- [ ]`) para tracking. **BLOQUEADO** indica dependencia
> que el humano debe resolver antes de continuar; NO se marca como hecho.

## Estado de ejecución (2026-06-10)

Tasks 1-5 **ejecutadas y en verde** (evidencia en la sesión de ejecución).
Desvíos deliberados respecto del texto original:

- **Scope npm**: el paquete compartido es `@iaportafolio/sdk-core` (no
  `@faro/sdk-core`) — consistente con `@iaportafolio/node|nextjs|expo`.
  Es `private: true`: entra como **devDependency `file:`** y tsup lo
  **inlinea** en el bundle de cada SDK (una dependency runtime no publicada
  rompería el publish a npm).
- **`traceparent` NO se movió al core**: solo existe en sdk-node (no estaba
  duplicado) y `currentOpenTelemetryTraceContext` depende de
  `@opentelemetry/api` (dep de plataforma).
- **Tests de SDKs intactos**: los tests de feature-flags de expo prueban vía
  API pública (`isFeatureEnabled`), no las funciones puras → quedan como
  tests de integración. El conteo de cada SDK no baja; el core agrega 19
  tests nuevos (unit + parity + guard anti-reimplementación).
- CI: job `sdk-core` agregado a `.github/workflows/sdk-tests.yml`; suite
  `sdk-core` agregada a `scripts/test-all.sh`.

Task 6 sigue **fuera de alcance**; Task 7 sigue **BLOQUEADO — esperando
contenido de M-7/M-8**.

## Contexto

M-5 reúne **3 problemas distintos** con la misma etiqueta de severidad MEDIA:

1. **God-files en backend/frontend**: 3 archivos >1100L concentran tipos,
   helpers, fetchers/handlers y markup; su tamaño dificulta code review y
   reuso (`insights.rs:1192L`, `api.ts:1085L`, `funnels/+page.svelte:1146L`).
2. **Duplicación cross-SDK en TS**: las 6 funciones puras de feature flags +
   scrubbing (`scrubString`, `scrubWire`, `clampRollout`, `normalizeConditions`,
   `matchesFeatureConditions`, `stickyBucket`) están reimplementadas en
   `sdks/node/src/index.ts`, `sdks/nextjs/src/browser-core.ts` y
   `sdks/expo/src/index.ts`. Es duplicación **deliberada y documentada** para
   mantener paridad cross-SDK, pero **sin spec ejecutable** que la garantice.
3. **M-7 y M-8 — divergencias reales**: el audit menciona que la falta de
   paridad ejecutable ya produjo divergencias entre SDKs. **El contenido de
   M-7/M-8 no está en el repo** (GAP-AUDIT-2026-06-05.md no los incluye); ver
   "Bloqueos" abajo.

**Toolchain disponible en este entorno**: `cargo`, `rustc`, `node`, `npm`,
`python3`, `gradle`. `go`, `flutter` y `pytest` (módulo) **no son
instalables** (no root/sudo, apt/snap fallan). Implicación AGENTS.md §6:
`sdks/go/**` y `sdks/flutter/**` solo se tocan si tienen test suite
**ejecutable localmente**; si no, esos SDKs quedan **NO CORRIDO** en la
evidencia — no marcados como "hecho".

## Decisiones de diseño (resumen)

| Eje | Decisión | Por qué |
|---|---|---|
| God-files backend | Particionar por **endpoint**, no por capa | `insights.rs` ya tiene 4 endpoints aislados (revenue/latency/web-vitals/service-dashboard); 1 endpoint = 1 archivo en `api/insights/` |
| God-file frontend api.ts | Particionar por **recurso REST** | `api.ts` ya está organizado por recurso (dashboard, logs, traces, …); mover cada grupo a `lib/api/<recurso>.ts` y reexportar desde `index.ts` |
| God-file funnels svelte | Particionar por **sección visible** | La página tiene 4 sub-flujos (compute / drop-off / time-to-convert / saved); extraer 4 subcomponentes en `lib/components/funnels/` |
| Dedup SDKs TS | **Extraer a paquete compartido** `@faro/sdk-core` (workspace npm) | Funciones puras, sin deps de plataforma, mismo contrato en los 3 SDKs; un paquete local editable (`file:` ref) y un solo set de tests de paridad |
| Paridad spec | Tests **fixtures compartidos** + un harness Node que corre los mismos casos contra los 3 SDKs | Fixtures en JSON, no duplicados por SDK; harness en `sdks/_parity/` |
| M-7/M-8 | **BLOQUEADO** hasta que humano comparta el contenido | Sin el dato, no se puede diseñar fix |

## Estructura de tasks

### Task 1 — Particionar `backend/src/api/insights.rs` (1192L)

**Archivos:**

- Crear: `backend/src/api/insights/mod.rs` (re-exports + `router()` agregado)
- Crear: `backend/src/api/insights/revenue_impact.rs`
- Crear: `backend/src/api/insights/latency_funnel_impact.rs`
- Crear: `backend/src/api/insights/web_vitals_conversion_impact.rs`
- Crear: `backend/src/api/insights/service_dashboard.rs`
- Modificar: `backend/src/api/mod.rs` (path adjustment)
- Borrar: `backend/src/api/insights.rs`

- [x] Crear `insights/` y mover cada bloque de `#[axum::...]` + structs
      asociados a su archivo (los 4 endpoints se identifican por sus
      `*Query`/`*Result` structs en `insights.rs:46-216`).
- [x] Mover el `pub fn router()` a `insights/mod.rs` agregando los 4 sub-routers.
- [x] Borrar el archivo viejo y ajustar imports (`use crate::api::insights`).
- [x] Correr suite backend (sin `cargo test --lib` solo si el cambio es
      puramente mecánico de organización; integration tests existen para
      `revenue_impact` y `latency_funnel_impact` y DEBEN correr contra
      ClickHouse).
- [x] Correr `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings`.

**Riesgo bajo**: partición por endpoint, sin lógica cruzada. Cero cambio de
comportamiento esperado.

### Task 2 — Particionar `frontend/src/lib/api.ts` (1085L, 169 exports)

**Archivos:**

- Crear: `frontend/src/lib/api/index.ts` (re-exports + `api<T>`, `apiBase`,
  `qs`, `RangeArgs` compartidos)
- Crear: `frontend/src/lib/api/dashboard.ts`
- Crear: `frontend/src/lib/api/logs.ts`
- Crear: `frontend/src/lib/api/traces.ts`
- Crear: `frontend/src/lib/api/services.ts`
- Crear: `frontend/src/lib/api/metrics.ts`
- Crear: `frontend/src/lib/api/issues.ts`
- Crear: `frontend/src/lib/api/monitors.ts`
- Crear: `frontend/src/lib/api/replays.ts`
- Crear: `frontend/src/lib/api/productEvents.ts`
- Crear: `frontend/src/lib/api/productUsers.ts`
- Crear: `frontend/src/lib/api/funnels.ts`
- Crear: `frontend/src/lib/api/experiments.ts`
- Crear: `frontend/src/lib/api/cohorts.ts`
- Modificar: todos los `+page.svelte` y `+layout.svelte` que importen desde
  `'$lib/api'` (verificar con `grep` que `$lib/api` siga funcionando vía
  re-exports; **no** debería romperse)
- Test: `frontend/src/lib/api/api.test.ts` ya existe — no debería romperse
- Borrar: `frontend/src/lib/api.ts`

- [x] Mover cada bloque temático a su archivo nuevo (los `export const
      fetch*` ya están agrupados por recurso: `fetchDashboard:270`,
      `fetchLogs:274`, `fetchTraces:280`, `fetchFunnelEvents:568`, etc.).
- [x] Reexportar todo desde `api/index.ts` para no romper `$lib/api`.
- [x] Correr `cd frontend && npm test`, `npm run check`, `npm run build`.
- [x] **Prohibido** cambiar firmas ni agregar renames: el contrato público
      de cada fetcher se mantiene.

**Riesgo bajo**: movimiento mecánico. El test suite de frontend ya cubre
tipos y fetchers.

### Task 3 — Particionar `frontend/src/routes/funnels/+page.svelte` (1146L)

**Archivos:**

- Crear: `frontend/src/lib/components/funnels/ComputeSection.svelte`
- Crear: `frontend/src/lib/components/funnels/DropOffSection.svelte`
- Crear: `frontend/src/lib/components/funnels/TimeToConvertSection.svelte`
- Crear: `frontend/src/lib/components/funnels/SavedFunnelsSection.svelte`
- Modificar: `frontend/src/routes/funnels/+page.svelte` (queda como
  composition root < 300L)

- [x] Identificar las 4 secciones en el `<script lang="ts">` (líneas 1-447)
      y su markup correspondiente.
- [x] Extraer cada sección a su subcomponente pasando props tipadas.
- [x] Mover helpers compartidos a `frontend/src/lib/funnels.ts` (parsing,
      formateo de pasos, validaciones).
- [x] Correr `npm test` + `npm run check` + `npm run build`.

**Riesgo medio**: Svelte props tipados requieren cuidado con stores
(`writable` de funnelVersion es global). Verificar que el refactor no
rompe el patrón anti-race reqSeq/funnelVersion mencionado en el audit.

### Task 4 — Extraer `@faro/sdk-core` y dedupe los 3 SDKs TS

**Archivos:**

- Crear: `sdks/_shared/sdk-core/` (paquete workspace, scope `@faro/sdk-core`)
  - `package.json` (name `@faro/sdk-core`, exports: `./feature-flags`,
    `./scrub`, `./traceparent`, `./wire`)
  - `src/feature-flags.ts` (`clampRollout`, `normalizeConditions`,
    `matchesFeatureConditions`, `stickyBucket`)
  - `src/scrub.ts` (`scrubString`, `scrubWire`, `DEFAULT_SCRUB_FIELDS`,
    `HEADER_SCRUB_FIELDS`, `REDACTED`, `SCRUB_REGEXES`, `ScrubPreset`)
  - `src/traceparent.ts` (`parseTraceparent`, `normalizeTraceContext`,
    `normalizeHex`, `currentOpenTelemetryTraceContext`)
  - `src/wire.ts` (tipos `Wire` / `WireEvent` consolidados)
  - `test/feature-flags.test.mjs` (suite unificada de los 6 helpers)
  - `test/scrub.test.mjs`
  - `test/traceparent.test.mjs`
- Modificar: `sdks/node/package.json` (dep `@faro/sdk-core: "file:../_shared/sdk-core"`)
- Modificar: `sdks/node/src/index.ts` (importar desde `@faro/sdk-core`,
  borrar las 6 funciones; re-export `FeatureFlagWire`/`FeatureFlagContext`
  desde el core)
- Modificar: `sdks/node/test/feature-flags.test.mjs` (borrar los casos
  duplicados a los del core; mantener los de integración con
  `FaroClient.isFeatureEnabled`)
- Modificar: `sdks/nextjs/package.json` + `sdks/nextjs/src/browser-core.ts`
  (ídem node)
- Modificar: `sdks/expo/package.json` + `sdks/expo/src/index.ts` (ídem node)
- Crear: `sdks/_shared/sdk-core/test/parity.test.mjs` — **spec ejecutable
  de paridad**: corre la misma batería de casos (12 fijos, ej.:
  stickyBucket('user-42') === mismo número siempre; clampRollout(NaN) ===
  0; matchesFeatureConditions con conditions `eq`/`in`/`regex`/`not_in`;
  scrubWire con needles + regexes contra un fixture de evento) contra
  cada uno de los 3 SDKs. Si un SDK diverge, el test falla con diff.

- [x] Crear `sdks/_shared/sdk-core/` y mover las 6 funciones + constantes +
      tipos de `sdks/node/src/index.ts` como fuente canónica.
- [x] Reemplazar las 6 funciones en los 3 SDKs con `import { ... } from
      '@faro/sdk-core'`. Sin cambios de firma ni de semántica.
- [x] Reemplazar los tests de `feature-flags` duplicados en
      `sdks/node/test/feature-flags.test.mjs` y
      `sdks/expo/test/feature-flags.test.mjs` por re-exports del core
      (mantener solo los tests que prueban la integración con la API de
      alto nivel del SDK).
- [x] Crear `sdks/_shared/sdk-core/test/parity.test.mjs` con la batería
      fija.
- [x] Correr `cd sdks/node && npm test` + `cd sdks/nextjs && npm test` +
      `cd sdks/expo && npm test` + `cd sdks/_shared/sdk-core && npm test`.
- [x] **NO tocar** go/flutter/kotlin/python en este task. Quedan para
      evaluation posterior (ver Task 6).

**Riesgo medio**: el test runner de cada SDK debe poder resolver el
paquete `@faro/sdk-core` por `file:`. Hay que verificar que `tsc` (o el
build) de cada SDK acepte el `file:` dep o si requiere un
`npm install` previo en `sdks/_shared/sdk-core/`.

### Task 5 — Spec ejecutable cross-SDK (cubre Task 4 parcialmente)

**Archivos:**

- Crear: `sdks/_shared/sdk-core/test/parity.test.mjs` (ver Task 4)
- Crear: `sdks/_shared/sdk-core/test/fixtures/` (JSON con casos
  canónicos: 1 evento de log, 1 wire de feature flag, 1 traceparent válido,
  1 inválido, 1 con regex, etc.)
- Crear: `docs/superpowers/specs/2026-06-10-sdk-parity-spec.md` (la spec
  escrita que el test codifica — un humano puede leerla y los devs
  pueden extenderla)

- [x] Definir los casos canónicos en el doc de spec primero.
- [x] Codificarlos como fixtures JSON.
- [x] Implementar el harness que itera `node_modules/@faro/node`,
      `@faro/nextjs`, `@faro/expo` (o builds de cada uno) y corre cada
      caso contra cada SDK.

**Riesgo bajo**: el harness es solo Node; no requiere toolchain extra.

### Task 6 — Evaluación de dedup para go/python/flutter/kotlin

**BLOQUEADO hasta Task 4 verde.** No se aborda en este plan porque:

- `sdks/go/faro.go` (1131L) — Go no instalable en este entorno (no
  root/sudo). La suite `sdk-go` quedaría **NO CORRIDO** (AGENTS.md §6.4).
  Cualquier refactor sin test verde = prohibido.
- `sdks/flutter/lib/faro_sdk.dart` (667L) — Mismo problema con Flutter.
- `sdks/python/faro_sdk/__init__.py` (1206L) — `pytest` no está como
  módulo pero `python3 -m pytest` funciona si se instala. Posible, pero
  requiere `pip install -e ".[dev]"` antes. **Decidir** con humano si
  vale la pena en esta pasada.
- `sdks/kotlin/.../Faro.kt` (681L) — Gradle presente. Tratable, pero
  JVM/Gradle builds son lentos; mejor en pasada separada.

Si Task 4 prueba que la paridad funciona para TS, el mismo patrón se
puede replicar a python/kotlin sin re-inventar el harness. Pero la
ejecución queda **fuera de este plan**.

### Task 7 — M-7 y M-8 (BLOQUEADO)

**BLOQUEADO**: el contenido de M-7 y M-8 no está en el repo. El audit
original los menciona pero no los vuelca. Sin ese dato, no se puede
diseñar fix. **Acción del humano**: pegar el contenido de M-7 y M-8 en
este doc (o en issues separados) antes de abordar Task 7.

Si M-7/M-8 ya están "resueltos" en algún PR/branch reciente: marcar
`[x]` y archivar.

## Toolchain & gates por task (AGENTS.md §1)

| Task | Suites que toca | Gates obligatorios |
|---|---|---|
| 1 (insights.rs split) | **backend** | `cargo test` + `cargo fmt --check` + `cargo clippy -D warnings` + integration (`revenue_impact_insights`, `latency_funnel_impact` requieren ClickHouse) |
| 2 (api.ts split) | **frontend** | `npm test` + `npm run check` + `npm run build` |
| 3 (funnels split) | **frontend** | ídem Task 2 |
| 4 (sdk-core) | **sdk-node, sdk-nextjs, sdk-expo** | `npm test` en cada uno + `npm test` en `sdks/_shared/sdk-core` |
| 5 (parity spec) | **sdk-node, sdk-nextjs, sdk-expo** (cubierto por el harness de Task 4) | mismo que Task 4 |
| 6 (go/py/flutter/kotlin) | **BLOQUEADO en este plan** | — |
| 7 (M-7/M-8) | depende del contenido | a definir cuando se conozca |

**NO CORRIDO garantizado** (AGENTS.md §6): `sdk-go`, `sdk-flutter` —
declarar y nombrar el motivo en la evidencia de cada corrida que toque
estos SDKs (si los tocamos).

## Orden de ejecución propuesto

1. **Task 2** (api.ts split) — bajo riesgo, alto valor, no toca lógica,
   frontend es la suite más rápida de iterar. Calienta el camino para
   Task 3.
2. **Task 3** (funnels split) — riesgo medio, mismo scope frontend.
3. **Task 1** (insights.rs split) — backend, requiere ClickHouse vivo
   para integration tests; alineado con §1.d.
4. **Task 4 + 5** (sdk-core + parity spec) — atómico, no se entrega
   Task 4 sin Task 5.
5. **Task 7** (M-7/M-8) — cuando se desbloquee.
6. **Task 6** (extender dedup a otros SDKs) — pasada separada.

## Riesgos y preguntas abiertas

- **ClickHouse vivo para Task 1**: la suite integration de backend
  requiere `CLICKHOUSE_URL` y CH respondiendo (AGENTS.md §1.d). Sin
  eso, `cargo test --lib` no basta. El plan declara integration como
  obligatorio para Task 1; si el entorno no tiene CH, declarar
  **BLOQUEADO** y ofrecer la vía Docker
  (`docker compose -f docker-compose.test.yml -p faro-test up`).
- **Task 4 — workspace npm**: el repo no usa workspaces npm hoy (cada
  SDK tiene su `package.json` independiente). Hay que decidir entre
  (a) `file:` dep simple — funciona, pero requiere `npm install` en
  orden; o (b) agregar `"workspaces": ["sdks/_shared/*", "sdks/*"]` al
  `package.json` raíz. Recomiendo (a) por mínimo cambio. **Confirmar
  con humano**.
- **Task 3 — patrón anti-race reqSeq/funnelVersion**: el audit destaca
  esto como "madurez"; cualquier cambio debe preservar el patrón
  (debounce, contadores de secuencia). Verificar con `grep` antes de
  mover stores.
- **M-7/M-8 — sin contenido en el repo**: ver Task 7.

## Definition of Done

- [x] Tasks 1-3 ejecutados y merges con cero cambio de comportamiento
      (verificado por suites backend + frontend en verde, evidencia
      literal pegada).
- [x] Task 4 ejecutado: 3 SDKs TS importan `@faro/sdk-core`, el archivo
      core tiene su test suite en verde, y `parity.test.mjs` pasa en los
      3.
- [x] Task 5: `docs/superpowers/specs/2026-06-10-sdk-parity-spec.md`
      creado y referenciado desde el harness.
- [x] `sdks/_shared/sdk-core/` aparece en `scripts/test-all.sh` como
      suite ejecutable (o se documenta la invocación manual).
- [x] `sdk-go` y `sdk-flutter` declarados explícitamente como
      **NO CORRIDO — go ausente / flutter ausente** en la evidencia
      final, sin reclamar verde.
- [ ] Task 7 (M-7/M-8) marcada `[x]` si se resolvió, o queda en
      **BLOQUEADO — esperando contenido** con el motivo nombrado.
- [x] Test counts de cada suite **suben ≥** la cantidad de tests nuevos
      que el plan agrega (regla §5.5 de AGENTS.md): si Task 4 mueve tests
      al core, el conteo del core sube y el de cada SDK baja en la
      misma cantidad — no en cero. Documentar el delta.

## Referencias

- `AGENTS.md` §1 (mapeo cambio→suite), §1.a (backend fmt+clippy), §1.c
  (frontend check+build), §1.d (integration con ClickHouse), §5
  (anti-degradación), §6 (toolchain ausente), §10 (DoD checklist).
- `docs/testing.md` — fuente de verdad de comandos y suites.
- `GAP-AUDIT-2026-06-05.md` — audit original (no contiene M-7/M-8).
- `docs/superpowers/plans/2026-05-24-feature-flags.md` — formato de plan
  usado como referencia.
- `scripts/test-all.sh` — runner único.
