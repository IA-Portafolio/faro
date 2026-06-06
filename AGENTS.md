# Testing obligatorio para agentes LLM — Faro

> **Vinculante** para cualquier agente LLM (Claude Code, Cursor, Codex, etc.)
> que escriba o modifique código en este monorepo. Si vas a tocar código, lees
> esto **antes** y obedeces. La fuente de verdad de la red de regresión es
> [`docs/testing.md`](docs/testing.md); esta política te obliga a usarla y cierra
> las salidas por las que un agente se escapa sin verificar.
>
> Tono y flujo: ver [`CONTRIBUTING.md`](CONTRIBUTING.md). Imperativo, conciso, en
> español.

---

## 🔴 REGLA NO-NEGOCIABLE (leé esto aunque no leas nada más)

**Ningún cambio de código está terminado hasta que pegues la salida REAL y
POSTERIOR de los tests, en verde, de TODAS las suites que tu cambio toca.**

Terminado = *cambio* **+** *suite(s) que toca en verde* **+** *evidencia literal
pegada*. Sin esa evidencia, tu trabajo está **incompleto** y no se acepta —
decir "los tests pasan", "está listo" o "compila" sin pegar la salida es lo
mismo que no haber corrido nada.

El comando que cierra el ciclo es siempre el mismo:

```bash
bash /opt/faro/scripts/test-all.sh <suite...>   # las suites que tu cambio toca
bash /opt/faro/scripts/test-all.sh              # todas las que el entorno permita
```

No existe el cambio exento. No existe "trivial", "solo refactor", "solo rename",
"solo docs", "ya pasaban", "es lento" ni "no hay toolchain" como motivo para
cerrar sin evidencia. Cada una de esas excusas está refutada abajo con su acción
obligatoria (§7).

---

## 1. Mapeo cambio → comando (corré la suite que ejercita lo que tocaste)

La regla mental es: **¿qué suite ejercita el código que toqué?** Esa corrés —
aunque NO hayas editado ningún archivo de test. Tocar el código fuente obliga a
su suite igual que tocar el test.

| Tocaste (código fuente **o** test) | Suite | Comando exacto (de `docs/testing.md`) |
| --- | --- | --- |
| `backend/src/**`, `backend/tests/*.rs` | **backend** (Rust) | `cd backend && cargo test` (o `cargo nextest run`) |
| `cli/src/**` (cualquier `.rs`) | **cli** (Rust) | `cd cli && cargo test` |
| `frontend/src/lib/**` (el `.ts` fuente **y** sus `*.test.ts`) | **frontend** (SvelteKit/vitest) | `cd frontend && npm test` |
| `sdks/node/**` (src o `test/*.test.mjs`) | **sdk-node** | `cd sdks/node && npm test` |
| `sdks/nextjs/**` | **sdk-nextjs** | `cd sdks/nextjs && npm test` |
| `sdks/expo/**` | **sdk-expo** | `cd sdks/expo && npm test` |
| `sdks/python/**` | **sdk-python** | `cd sdks/python && python3 -m pytest -q` |
| `sdks/go/**` | **sdk-go** | `cd sdks/go && go test ./...` |
| `sdks/flutter/**` | **sdk-flutter** | `cd sdks/flutter && flutter test` |
| `sdks/kotlin/**` | **sdk-kotlin** | `cd sdks/kotlin && ./gradlew test` |

Notas de comandos (basadas en lo que el runner realmente hace — no inventes
otros):

- **sdk-python**: el binario `pytest` puede NO existir aunque `python3` sí. El
  comando real es `python3 -m pytest -q`. `command not found` de `pytest` **no**
  equivale a toolchain ausente (ver §6).
- **sdk-kotlin**: hoy **no** existe `sdks/kotlin/gradlew`; el runner cae a
  `gradle test --no-daemon` con el `gradle` global. "No hay gradlew" **no** es
  motivo para saltar la suite si `gradle` está en el PATH (ver §6).

No mapea a ninguna suite **solo** un cambio que toca exclusivamente archivos
`.md` o imágenes que ningún job de CI compila/testea — y aun así debés
declararlo con la lista de paths tocados (§8). OJO: hay "docs" que SON código
(ver §9).

### 1.a Backend (Rust): los tests NO alcanzan

Si tocaste `backend/**`, **Terminado exige además** pegar en verde:

```bash
cd backend && cargo fmt --all -- --check
cd backend && cargo clippy --all-targets -- -D warnings
```

`clippy` corre con `-D warnings`: **cualquier** warning es error y rompe CI —
incluye `clippy::items_after_test_module` (regla 3 de §5). Sin estos dos en
verde, el cambio Rust **no está terminado**, aunque `cargo test` pase. "No tengo
cargo" no convierte un gate en "no aplica": pasa a §6.

> CI corre además `cargo audit --deny warnings --ignore RUSTSEC-2024-0436`
> ([`ci.yml`](.github/workflows/ci.yml) job `backend-audit`). `test-all.sh` **no**
> lo corre; si tocás dependencias (`Cargo.toml`/`Cargo.lock`), córrelo o avisá
> que es un gate que CI ejercitará.

### 1.b CLI (Rust)

`cli` es Rust: aplican los mismos gates `fmt` + `clippy` de §1.a sobre `cli/`.

### 1.c Frontend: `npm test` no es todo lo que CI exige

`test-all.sh frontend` corre `svelte-kit sync` + `npm test` (vitest). Pero el
job `frontend` de [`ci.yml`](.github/workflows/ci.yml) además corre, y bloquea
el merge con:

```bash
cd frontend && npm run check          # svelte-check / typecheck
cd frontend && npm run build          # PUBLIC_API_BASE=http://localhost:8080
cd frontend && npm audit --omit=dev --audit-level=high
```

Si tocaste `frontend/**`, corré `check` y `build` además de `npm test`, y pegá
su salida. "test-all.sh en verde" **no** garantiza CI verde para frontend.

### 1.d Backend integration tests (ClickHouse)

Los integration tests de `backend/tests/*.rs` ejercitan los handlers reales
contra ClickHouse y requieren `CLICKHOUSE_URL` apuntando a un ClickHouse **vivo**:

- **Con** `CLICKHOUSE_URL` y CH respondiendo → corren unit **+** integration.
- **Sin** ClickHouse → `test-all.sh` corre solo unit inline (`cargo test --lib`).

`cargo test --lib` **solo basta** si tu diff es lógica inline pura que NO cruza
la red ni ClickHouse. **Si tu cambio toca handlers HTTP, queries, ingest, schema
o cualquier cosa cubierta por `backend/tests/*.rs`, los integration tests NO son
opcionales**: levantá ClickHouse y córrelos. Las dos vías (de
[`CONTRIBUTING.md`](CONTRIBUTING.md)):

```bash
# Opción A — con Rust + CH local (puerto 8123, usuario faro):
cd backend && cargo nextest run        # o: cargo test --tests

# Opción B — sin Rust ni CH locales, todo en Docker:
docker compose -f docker-compose.test.yml -p faro-test up \
  --abort-on-container-exit --exit-code-from backend-test
```

Declarar verde con solo `cargo test --lib` cuando el cambio toca la capa de
integración está **PROHIBIDO**. Decí explícitamente que integration corrió.

### 1.e SDKs: `test` local no replica todo el gate de publish

[`sdk-tests.yml`](.github/workflows/sdk-tests.yml) bloquea PRs y publishes con
más que los tests. `test-all.sh` corre solo los tests; estos gates extra los
ejercita CI y conviene anticiparlos cuando tocás un SDK:

- **sdk-go**: `go vet ./...`, `go build ./...`, `go test -race ./...`, golangci-lint.
- **sdk-flutter**: `flutter pub get`, `dart analyze`, `flutter test`.
- **sdk-python**: `ruff check .` (hoy *informativo*: corre con `|| true`, no bloquea), `python -m build`, `pytest`.
- **sdk-kotlin**: `./gradlew build --no-daemon` **antes** de `./gradlew test --no-daemon`.

Si solo corriste el `test` local, avisá que estos gates de lint/build/`-race`
los validará CI.

---

## 2. El comando único: `scripts/test-all.sh`

Para cambios multi-componente, o cuando no estás 100% seguro del alcance, corré
el runner único desde ruta absoluta:

```bash
bash /opt/faro/scripts/test-all.sh                    # todo lo que el entorno permita
bash /opt/faro/scripts/test-all.sh backend frontend   # solo el subconjunto que tocás
```

Suites válidas: `backend cli frontend sdk-node sdk-nextjs sdk-expo sdk-python
sdk-go sdk-flutter sdk-kotlin`.

Comportamiento real del runner (importa para entender la evidencia):

- **Salta** (con aviso `⊘ saltado`, sin abortar el resto) cada suite cuyo
  toolchain falte.
- Devuelve **exit ≠ 0** si CUALQUIER suite que **sí** se pudo correr falló.
- Imprime al final un bloque `RESUMEN` con `pasaron / fallaron / saltadas`.

Invocá siempre con `bash` y ruta absoluta. Un "no es ejecutable" / "no está en
PATH" / cwd equivocado **no es excusa de no ejecución**: corregí la invocación
(`bash /opt/faro/scripts/test-all.sh ...`) y volvé a correr.

---

## 3. Evidencia OBLIGATORIA (sin salida literal, no pasó)

Por cada suite que corras, pegá la **cola literal** de la salida —copiada sin
editar, con el comando exacto que invocaste y suficientes líneas de contexto—
donde se vea el veredicto del runner:

| Suite | Línea de veredicto a pegar |
| --- | --- |
| backend, cli (cargo) | `test result: ok. <N> passed; 0 failed; ...` |
| frontend, sdk-node/nextjs/expo (vitest / node --test) | `Tests  <N> passed (<N>)` / `<N> passed` |
| sdk-python (pytest) | `===== <N> passed in <X>s =====` |
| sdk-go | `ok  <pkg>` / `PASS` |
| sdk-flutter | `All tests passed!` |
| sdk-kotlin (gradle) | `BUILD SUCCESSFUL` |
| `scripts/test-all.sh` | el bloque `════ RESUMEN ════` **completo** (las TRES líneas) |

Reglas de la evidencia:

1. **Literal, no parafraseada.** Copiá lo que imprimió la terminal. PROHIBIDO
   transcribir de memoria, resumir a mano o reconstruir conteos. **Fabricar
   salida es la falta más grave.** No uses números "que sabés que da la suite":
   la salida tiene que venir de una corrida real de esta sesión.
2. **Posterior a tu última edición.** PROHIBIDO reutilizar un verde capturado
   antes del cambio o de otra rama. Si re-editás después de correr, volvés a
   correr. Pegá también `echo EXIT=$?` justo después del comando.
3. **El RESUMEN va completo.** Pegá las tres líneas `pasaron / fallaron /
   saltadas`, no solo `fallaron: 0` ni solo la línea `pasaron`.
4. **Si dice `failed`/`FAILED`/`error[...]`/`BUILD FAILED` o exit ≠ 0**: el
   trabajo NO está hecho. Arreglá el código (o el test si el contrato cambió a
   propósito) y volvé a correr hasta verde real. No silencies ni borres tests,
   ni uses `|| true`, ni filtres la suite que falla.

---

## 4. Qué cuenta como VERDE (cierra el hueco del runner)

> ⚠️ **`fallaron: 0` NO es, por sí solo, evidencia de nada.** El runner devuelve
> exit 0 y `fallaron: 0` **aunque TODAS las suites se hayan SALTADO**
> (`pasaron: 0, saltadas: N`). En este entorno faltan `cargo`, `go`, `flutter` y
> el módulo `pytest`, así que un `test-all.sh` completo deja backend, cli,
> sdk-go, sdk-flutter y sdk-python sin correr y aun así imprime `fallaron: 0`.

Verde para una suite = esa suite aparece en **`pasaron:`** del RESUMEN. Reglas
duras:

- Una suite en **`saltadas:`** NO es evidencia: trátala como NO CORRIDA (§6).
- Es inválido cerrar con `pasaron: 0`, o con cualquier suite que TU CAMBIO TOCA
  en `saltadas:` o `fallaron:`.
- Pegar un RESUMEN con suites verdes **ajenas** (las que el entorno sí permite)
  mientras la suite que tocaste quedó saltada es una **violación**: demostrá que
  pasaron **las tuyas**, no que pasaron otras.

Por eso, junto al RESUMEN, pegá esta línea por cada suite que tocás:

```text
Suite de MI cambio: <nombre> → estado: pasó | saltado | falló
```

---

## 5. Reglas anti-degradación (ya vigentes — respétalas)

Las cinco reglas de [`docs/testing.md` → "Reglas para que la red no se
degrade"](docs/testing.md). Tu cambio debe cumplirlas:

1. **Toda función nueva con lógica entra con su test.** Si es lógica pura
   (parsing, formato, cálculo, validación), un unit test **al lado del código**
   (`#[cfg(test)]` en Rust, `*.test.ts` / `test_*.py` / `*.test.mjs`) es lo más
   barato. Reservá los integration tests (`backend/tests/`) para lo que cruza la
   red o ClickHouse.
2. **Un test que existe pero no se corre no protege nada.** Si agregás un
   archivo de test a un SDK Node, asegurate de que el glob de `npm test` lo
   incluya (`test/*.test.mjs` — **no** recursivo, **no** `.spec.mjs`). El glob
   por SDK/lenguaje: `test/*.test.mjs`, `test_*.py`, `*.test.ts`, `*_test.go`,
   `*_test.dart`. Un test fuera del glob es invisible para CI.
3. **El módulo de tests va al FINAL del archivo** en Rust
   (`clippy::items_after_test_module` lo exige con `-D warnings`).
4. **No introduzcas tests flaky.** Nada de `Date.now()` / relojes reales sin
   `tokio::time::pause()` o timers falsos; nada que dependa del orden de
   ejecución ni de variables de entorno globales compartidas entre tests.
5. **Antes del PR:** `scripts/test-all.sh` en verde (o al menos las suites que tu
   cambio toca, con su evidencia).

**Prueba de que tu test nuevo se ejecuta:** si creaste lógica nueva, NO basta
con que la suite vieja pase en verde. Mostrá (a) la ruta + nombre del test
nuevo, y (b) que se ejecutó: el conteo de la suite subió en ≥ tus casos, o el
nombre de tu test aparece en la corrida. Conteo igual al previo = tu test no se
está corriendo (probablemente fuera del glob).

---

## 6. Toolchain ausente, parcial, o servicio faltante

Es válido que falten toolchains en tu entorno. Lo que NO es válido es mentir
sobre lo que corriste, ni usar la falta como cierre cómodo. Protocolo:

1. **Antes de aceptar `saltado` para una suite, confirmá con `command -v` que el
   toolchain REALMENTE falta** (`cargo`, `go`, `flutter`, `npm`, `python3`,
   `gradle`/`gradlew` para kotlin). Pegá esa verificación junto al aviso de
   saltado. Si el binario existe pero el wrapper no (p. ej. `gradle` sí,
   `gradlew` no), **usá el binario y corré la suite** — no la declares saltada.
2. **Toolchain parcial = ROJO, no saltado.** Si el intérprete está pero falta la
   dependencia de test (caso real: `python3` presente, módulo `pytest`
   ausente → `python3 -m pytest` da `No module named pytest`), eso es un FALLO
   de ejecución. DEBÉS instalar la dep antes de cerrar:
   `pip install pytest` (o `pip install -e ".[dev]"`). PROHIBIDO reclasificar un
   fallo de ejecución como "saltado".
3. **Si la ÚNICA suite que tu cambio toca queda sin correr por falta de
   toolchain, el trabajo NO está terminado.** Antes de declarar nada, intentá en
   este orden:
   a. **Instalar el toolchain** (`rust-toolchain.toml` fija la versión de Rust).
   b. **Correr en Docker** la vía documentada:
      `docker compose -f docker-compose.test.yml -p faro-test up --abort-on-container-exit --exit-code-from backend-test`
      (backend), o `docker-compose.sdk-integration.yml` (SDKs).
   c. Solo si **ambas** fallan, marcá el trabajo como **BLOQUEADO / no
      verificado** —no como hecho— pegando el error de ESE intento, nombrando la
      suite y la lógica que quedó sin ejercitar, y dejando claro que un humano /
      CI debe validarla antes de mergear.
4. **Nunca marques verde lo no corrido.** Una suite saltada es "no verificada",
   nunca "aprobada".

---

## 7. "No hay excusa" — racionalizaciones PROHIBIDAS

Estas son las salidas por las que un agente cierra sin verificar. Cada una está
**prohibida** y tiene su acción obligatoria. Si te encontrás pensando una de
ellas, hacé la acción.

| Excusa | Por qué es falsa | Acción OBLIGATORIA |
| --- | --- | --- |
| **"Es trivial / una línea / obvio."** | Lo trivial se rompe en silencio. "Trivial" no aparece como eximente en esta política. | Corré la(s) suite(s) que toca (§1) y pegá la salida (§3). Sin umbral de tamaño. |
| **"Es solo refactor / rename, no cambia comportamiento."** | Un refactor que no rompe nada se **demuestra** con la suite en verde, no con tu intuición; un rename mal hecho rompe tests igual. | Corré la suite. Refactor sin tests corridos = comportamiento no verificado. |
| **"Es solo docs."** | "Solo docs" = SOLO `.md`/imágenes que ningún job compila/testea. Editar `frontend/src/lib/sdk-docs.ts`, `.env.example`, o agregar bajo `docs/` NO es solo docs (§9). | Si tocaste código-doc, corré su gate y pegalo. Si es `.md` puro, declaralo con la lista de paths (§8). |
| **"Los tests ya pasaban antes."** | "Antes" no es "después de tu diff". El único estado que importa es ahora, con tu cambio aplicado. | Volvé a correr post-edit y pegá esa salida (§3.2). |
| **"Es lento / tarda mucho."** | Lento no es excusa para cero. | Acotá con `test-all.sh <suite...>` a las suites que tocás. Si tocaste varias, la lentitud es el precio de no romper producción. |
| **"No hay toolchain, no puedo correr nada."** | `test-all.sh` corre lo que puede y salta el resto; y "saltado" no cierra el trabajo. | Seguí §6: verificá con `command -v`, intentá Docker / instalar, y si la suite tocada no corre, marcá BLOQUEADO — no "hecho". |
| **"Escribí el test pero lo leí y está bien, no hace falta correrlo."** | Un test que no se corre no protege nada (regla 2 de §5). Leer ≠ ejecutar. | Corré la suite y mostrá que tu test se ejecutó (§5). |
| **"Corrí un test puntual y alcanza."** | Un test aislado en verde no prueba que no rompiste a sus vecinos. | El cierre exige la **suite del componente** en verde (o `test-all.sh`). |
| **"Backend: corrí `cargo test`, fmt/clippy es cosmético."** | CI **bloquea el merge** con `cargo fmt --check` y `cargo clippy -D warnings`. | Corré y pegá ambos en verde (§1.a). Sin ellos, el cambio Rust no está terminado. |
| **"No es repo git, no sé qué toqué."** | La ausencia de git no reduce el alcance de testing. | Enumerá a mano cada archivo que creaste/modificaste esta sesión y mapealo con §1. Ante cualquier duda, `test-all.sh` COMPLETO. |
| **"Un test falló por entorno / es flaky / preexistente."** | No existe "falla preexistente/no relacionada" como pase. Un fallo de setup (deps, sync) es rojo, no flaky. | Resolvé el setup (`npm install`, `npx svelte-kit sync`, instalar toolchain) y volvé a verde. Si creés que el rojo es previo a tu cambio, PROBALO: corré la suite en el estado base sin tu diff y pegá ambas salidas. Sin esa prueba, todo rojo es tuyo. |
| **"test-all.sh dio `fallaron: 0`, listo."** | Da `fallaron: 0` con todo saltado (§4). | Demostrá que las suites que TOCÁS están en `pasaron:`, no en `saltadas:`. |

---

## 8. Sin git: cómo determinás el alcance

`/opt/faro` **no es repo git todavía**, así que no hay `git diff` que funde el
mapeo path→suite. Eso **no reduce** el testing requerido:

- Llevá una **lista explícita de cada archivo que creaste/modificaste** en esta
  sesión y mapeala a suites con §1. Pegá esa lista junto al RESUMEN para que el
  alcance sea auditable.
- **Nombrá TODAS las suites que tu cambio toca** en la invocación de
  `test-all.sh`, o corré sin argumentos. Omitir una suite tocada del comando
  para que no aparezca en `saltadas:` está **PROHIBIDO** (omitir == ocultar).
- Si no podés determinar con certeza qué suites cubre tu cambio, o tocaste más
  de un subsistema: corré `bash /opt/faro/scripts/test-all.sh` **sin
  argumentos**. Ante la duda, todo.

---

## 9. "Docs" que SON código (gates extra que CI sí corre)

`test-all.sh` no cubre estos gates, pero CI los bloquea. No los trates como
"solo docs":

- **`frontend/src/lib/sdk-docs.ts`** — fuente única de `/docs`, `/docs.md`,
  `/llms.txt`. Es código de la suite **frontend** (vitest) y debe seguir a la API
  de los SDKs. Tocarlo dispara `npm test` / `npm run check` del frontend.
  Mantenimiento: [`sdks/MANTENIMIENTO-DOCS.md`](sdks/MANTENIMIENTO-DOCS.md).
- **`.env.example`** — fuente única de las env-vars. Tras editarlo:

  ```bash
  bash /opt/faro/scripts/gen-env-reference.sh
  bash /opt/faro/scripts/check-env-reference.sh
  ```

- **`docs/**`** — sin huérfanas. Enlazá el doc nuevo desde un índice y corré:

  ```bash
  bash /opt/faro/scripts/check-orphan-docs.sh
  ```

- **`.md` en general** — CI corre lychee (links), cspell (typos) y markdownlint
  vía `docs.yml`; ver [`CONTRIBUTING.md`](CONTRIBUTING.md) para correrlos local.

---

## 10. Definición de Terminado (checklist — pegalo relleno con evidencia)

No declares el trabajo hecho hasta poder marcar TODO esto con evidencia real.
Una casilla que no podés marcar se escribe `NO CORRIDO — <qué>: <motivo>` — no se
deja en blanco ni se marca de adorno.

```text
[ ] 1. Enumeré los archivos que toqué (sin git: lista manual) y los mapeé a
       suites con la tabla §1 — incluyendo suites de código fuente, no solo de test.
[ ] 2. Corrí la(s) suite(s) que mi cambio toca (o test-all.sh sin args ante la duda).
[ ] 3. Backend/CLI (si toqué backend/** o cli/**): corrí y pegué en verde
       `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings`.
       ("(si aplica)" = si tocaste esos paths. Si no hay cargo: NO marcar,
        escribir "NO CORRIDO — fmt/clippy: cargo ausente"; es gate bloqueante en CI.)
[ ] 4. Frontend (si toqué frontend/**): corrí npm run check + npm run build además
       de npm test.
[ ] 5. Backend integration (si toqué handlers/queries/schema): levanté ClickHouse
       y corrí integration; no me quedé en cargo test --lib.
[ ] 6. Pegué la SALIDA REAL, literal, POST-edición de cada suite, con su veredicto
       y EXIT=$?, más el bloque RESUMEN COMPLETO de test-all.sh.
[ ] 7. Verde real: cada suite que toqué aparece en `pasaron:` (no en `saltadas:`
       ni `fallaron:`). Por cada una: "Suite de MI cambio: <x> → pasó".
[ ] 8. Toda suite que NO pude correr está como "NO CORRIDO — <suite>: <motivo>",
       con el command -v / error del intento Docker. No marqué verde nada no corrido.
[ ] 9. Cumplí las 5 reglas anti-degradación (§5): test al lado del código, dentro
       del glob, módulo de tests al final en Rust, sin flaky; y mi test nuevo
       aparece en el conteo.

PATHS TOCADOS:
<lista manual de archivos creados/modificados>

EVIDENCIA:
<cola literal de cada suite + EXIT=$? + RESUMEN completo de scripts/test-all.sh>
```

---

## 11. Referencias

- [`docs/testing.md`](docs/testing.md) — la red de regresión: mapa de suites,
  toolchains, ClickHouse y las reglas anti-degradación. **Fuente de verdad.**
- [`scripts/test-all.sh`](scripts/test-all.sh) — el runner único.
- [`.github/workflows/ci.yml`](.github/workflows/ci.yml) — backend
  (`fmt` + `clippy` + `nextest` contra ClickHouse + `cargo audit`), frontend
  (`check` + `build` + `test` + `npm audit`), compose, migrations.
- [`.github/workflows/sdk-tests.yml`](.github/workflows/sdk-tests.yml) — un job
  por SDK con lint/build/test (`go vet`/`-race`, `dart analyze`, `ruff`/`build`,
  `gradle build`). Bloquea PRs y publishes.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — flujo, convenciones, vías Docker para
  backend integration y gates de docs.
