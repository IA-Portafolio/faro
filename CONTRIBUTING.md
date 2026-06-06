# Contribuir a Faro

Faro es un proyecto propietario de IA Portafolio (ver [LICENSE](LICENSE)).
Las contribuciones externas se aceptan caso por caso y requieren acuerdo
previo. Esta guía documenta el flujo interno.

## Antes de empezar

1. Lee el [README](README.md) para entender qué hace Faro.
2. Lee [docs/development.md](docs/development.md) para levantar el stack
   localmente.
3. Para cambios no triviales (>200 líneas o que afecten el esquema de
   ClickHouse, la API REST pública o el contrato OTLP) abre un issue
   antes de codear, para validar la dirección.

## Flujo de trabajo

```bash
git checkout -b <tipo>/<descripcion-corta>     # feat/, fix/, docs/, chore/, refactor/
# trabajar
cargo fmt && cargo clippy -- -D warnings && cargo nextest run   # si tocaste backend
cd frontend && npm run check && npm run build && npm test   # si tocaste frontend
git commit -m "tipo: resumen en imperativo y minúsculas"
git push -u origin <branch>
gh pr create --fill
```

CI (`ci.yml`) corre fmt + clippy + tests del backend (unit + integration
contra ClickHouse efímero), typecheck + build + vitest del frontend, y
validación de `docker-compose`. Los jobs se filtran por path con
`dorny/paths-filter`: una PR que solo toca `frontend/` no levanta el stack
del backend, una PR de docs no corre nada salvo el job aggregator `ci`.

### Correr los tests del backend localmente

Los integration tests del backend ([backend/tests/](backend/tests/)) ejercitan
los handlers reales contra un ClickHouse efímero. Las dos formas:

**Opción A** — con Rust instalado y un CH local (puerto 8123, usuario `faro`):

```bash
cd backend
cargo nextest run   # paraleliza los 11 binarios cross-binary; ver docs/development.md
# o, si no tenés nextest instalado: cargo test --tests
```

**Opción B** — sin Rust ni CH locales, todo en Docker:

```bash
docker compose -f docker-compose.test.yml -p faro-test up \
  --abort-on-container-exit --exit-code-from backend-test
```

El compose monta `backend/` y `clickhouse/` como volúmenes, aplica el schema
sentencia-por-sentencia con `scripts/run-integration-tests.sh` y corre
`cargo test --tests`. La primera corrida es lenta (~10 min cold build de
Rust); las siguientes ~30-60 s con `cargo-cache` y `target-cache` persistidos
en volúmenes. Útil cuando tocás un test específico y querés iterar sin
empujar a CI.

### Validar docs antes de empujar

`docs.yml` corre tres gates contra cualquier `.md` cambiado: links rotos
(lychee), typos (cspell con dict en+es-ES) y estilo markdown (markdownlint).
Si tocás docs y querés iterar local sin empujar:

```bash
# Link check (sólo refs internos, sin tocar la red):
docker run --rm -v "$PWD:/work" -w /work lycheeverse/lychee:latest \
  --no-progress --offline --config lychee.toml './**/*.md'

# Spell check (instala dict-es-es en el container, no toca el repo):
docker run --rm -v "$PWD:/work" -w /work node:20-slim bash -c \
  'npm init -y >/dev/null && npm install --silent cspell@8 @cspell/dict-es-es && \
   npx cspell --no-progress --gitignore --config .cspell.json "**/*.md"'

# Markdown lint:
docker run --rm -v "$PWD:/work" -w /work node:20-slim \
  npx --yes markdownlint-cli2@latest --config .markdownlint.jsonc \
    "**/*.md" "!**/node_modules/**" "!**/CHANGELOG.md" "!sdks/**/CHANGELOG.md"
```

Cuando cspell flagea un término técnico legítimo (e.g. `kubernetes`,
`grpc`), agregalo al array `words` de [.cspell.json](.cspell.json) en el
mismo PR. No usés `// cspell:ignore` inline salvo para palabras realmente
locales a un archivo.

## Convenciones

### Commits

Estilo [Conventional Commits](https://www.conventionalcommits.org/es/v1.0.0/)
suelto. Tipos usados:

- `feat:` nueva capacidad visible para el usuario / API nueva.
- `fix:` bug fix observable.
- `refactor:` cambio interno sin alterar comportamiento.
- `docs:` solo documentación.
- `chore:` tooling, deps, CI.
- `perf:` mejora de performance.

Mantén el resumen ≤ 72 caracteres y en minúsculas. El cuerpo (opcional)
explica el _porqué_, no el _qué_.

### Código

- **Rust**: `cargo fmt` define el estilo, `clippy` con `-D warnings` es
  obligatorio. Prefiere `?` sobre `unwrap()` en código de runtime.
- **TypeScript / Svelte**: el `tsconfig.json` y `svelte-check` son ley.
  Sin `any` salvo en límites con APIs externas no tipadas.
- **SQL ClickHouse**: migraciones idempotentes (`IF NOT EXISTS`). Las que
  no lo sean serán rechazadas — `deploy.yml` las re-ejecuta en cada push.

### Tamaño de PR

Mantén los PRs pequeños y enfocados. Si tu cambio toca más de tres
subsistemas (backend, frontend, schema, infra, SDKs), divídelo. La
excepción son los renames mecánicos.

### Documentación y configuración

Dos checks adicionales en `docs.yml` (además de markdownlint/cspell/lychee)
garantizan que la documentación y la configuración no se desincronicen del
código:

- **`docs/` sin huérfanas** — todo archivo bajo `docs/` (incluyendo
  imágenes) tiene que estar enlazado desde algún índice del repo
  (README.md, docs/adr/README.md, otro doc). Si añadís un doc nuevo,
  enlazalo desde donde corresponda — el job `orphan-docs` corre
  [`scripts/check-orphan-docs.sh`](scripts/check-orphan-docs.sh) y falla
  el PR si nadie lo referencia. Localmente:
  ```bash
  bash scripts/check-orphan-docs.sh
  ```

- **Variables de entorno tienen una sola fuente** — [`.env.example`](.env.example)
  es la fuente única de verdad. Cuando agregás, renombrás o cambiás el
  default de una variable, edita `.env.example` (respetá el formato:
  header `# ---- Sección ----`, comentario descriptivo encima del var,
  línea en blanco entre vars) y regenerá la página:
  ```bash
  bash scripts/gen-env-reference.sh
  git add docs/reference/environment.md
  ```
  El job `env-reference` corre
  [`scripts/check-env-reference.sh`](scripts/check-env-reference.sh) y
  falla el PR con el diff si los dos archivos están desincronizados.
  README, `docs/deployment.md`, `infra/README.md` y `.env.prod.template`
  linkean a la página generada — no copies tablas de env-vars en otros
  lados.

- **La doc de los SDKs sigue al código** — si cambiás la API pública de un
  SDK (método nuevo/renombrado/eliminado, firma, opción de `init()`,
  capacidad, disponibilidad de `track/identify/page/screen/alias`, o un SDK
  nuevo), actualizá [`frontend/src/lib/sdk-docs.ts`](frontend/src/lib/sdk-docs.ts)
  en el mismo PR. Es la fuente única de `/docs`, `/docs.md` y `/llms.txt` y se
  mantiene a mano. Reglas y checklist:
  [`sdks/MANTENIMIENTO-DOCS.md`](sdks/MANTENIMIENTO-DOCS.md).

## SDKs

Cada SDK es independiente. Para publicar:

```bash
git tag sdk-<lang>-v<semver>
git push origin sdk-<lang>-v<semver>
```

El workflow `publish-sdks.yml` empuja el paquete al registry y crea una
release en GitHub. Bumpea la versión en el `CHANGELOG.md` del SDK (si
existe) en el mismo commit que el tag.

### Esquema canónico de tags de SDK

Solo hay **un** formato válido para tags de publishing — usar otra forma
hace que `publish-sdks.yml` no dispare ningún job y el paquete no salga.
El workflow `validate-tag.yml` falla rápido si alguien empuja un tag
malformado:

```
sdk-<lang>-v<major>.<minor>.<patch>[-<pre>][+<build>]
```

- `<lang>` ∈ `node | nextjs | expo | python | go | flutter | kotlin`
- `<major>.<minor>.<patch>` debe ser semver válido (sin la `v` interna).
- `<pre>` opcional, p. ej. `-rc.1`, `-beta.3`, `-alpha.0`.
- `<build>` opcional (raro), p. ej. `+build.42`.

| Tag                         | ¿Válido? | Por qué                                                |
| --------------------------- | -------- | ------------------------------------------------------ |
| `sdk-node-v0.1.0`           | ✅       | esquema canónico                                       |
| `sdk-nextjs-v0.3.0`         | ✅       | esquema canónico                                       |
| `sdk-python-v1.0.0-rc.1`    | ✅       | pre-release permitido                                  |
| `sdk-kotlin-v2.5.1-beta.4`  | ✅       | pre-release permitido                                  |
| `sdks/go/v0.1.0`            | ✅       | **auto-generado** por `publish-sdks.yml` para `go get` |
| `sdk-node-0.1.0`            | ❌       | falta la `v` antes del semver                          |
| `node-v0.1.0`               | ❌       | falta el prefijo `sdk-`                                |
| `sdk-rust-v0.1.0`           | ❌       | `rust` no está en el whitelist de SDKs                 |
| `sdk-node-v0.1`             | ❌       | semver requiere tres números (`major.minor.patch`)     |
| `sdk-node-v1.0.0.1`         | ❌       | semver no permite cuatro números                       |
| `sdk-Node-v0.1.0`           | ❌       | el lang va en minúsculas                               |

### Caso especial: SDK de Go

Go modules indexa por tags con prefijo de path. El tag canónico
`sdk-go-v0.1.0` no es suficiente para `go get` — `publish-sdks.yml`
auto-crea **también** `sdks/go/v0.1.0` apuntando al mismo commit. **No
crees tags `sdks/go/v*` a mano**: si ya existe el canónico, el workflow
lo gestiona; si no, `go get` no podrá resolver el módulo del subdir.

### Tags no-SDK

Tags fuera del namespace `sdk-*` y `sdks/go/*` (p. ej. `v1.0.0` para la
plataforma entera, o `backend-v...`) están **permitidos** y el validador
los deja pasar sin tocarlos — no hay un workflow que los consuma todavía.

## Reportar bugs y vulnerabilidades

- **Bugs normales**: abre un issue con los pasos para reproducir.
- **Vulnerabilidades**: NO abrir issue público. Sigue [SECURITY.md](SECURITY.md).

## Contacto

Mantenedor: [@victalejo](https://github.com/victalejo) — victoralejocj@gmail.com.
