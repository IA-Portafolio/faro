# Changelog

Todos los cambios notables del **core** de Faro (backend + frontend) se
documentan acá. Los SDKs llevan su propio versionado independiente vía
tags `sdk-<lang>-v<semver>` y releases automáticas en GitHub.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y el proyecto usa [Semantic Versioning](https://semver.org/lang/es/).

## [Unreleased]

### Fixed
- **Saltos directos del Command Palette no aparecían al teclear la sintaxis
  canónica** (`traces:<id>`, `logs:trace=<id>`, `errors:<fp>`). Bug latente
  introducido en el UX pass anterior: `allCommands.filter((c) => matches(c,
  query))` descartaba el jump porque la substring literal `traces:abc` no
  estaba en el haystack del comando (`Abrir traza abc /traces/abc` no
  contiene el `:`). El nuevo `search()` cortocircuita el grupo "Salto directo"
  con score=1000, así que el jump siempre se muestra primero cuando
  `jumpCommands(query)` lo emitió.
- **`auth::logout` (y `revoke_user_sessions`, `replace_recovery_codes`,
  `api/users::revoke_other_sessions`, `api/account::invalidate_all_recovery_codes`)
  fallaban silenciosamente en ClickHouse 24+**. El SQL usaba
  `toUInt64(now64(3) * 1000)` para el campo `version`, pero CH 24 rechaza la
  multiplicación de `DateTime64(3)` por `UInt16` (`Code: 43`,
  `Illegal types of arguments of function multiply`). El error iba al `let _ =
  ...` del handler, así que la fila de revoke nunca se insertaba: una sesión
  "deslogueada" seguía válida hasta el `expires_at` natural (30 días).
  Reemplazado por `toUInt64(toUnixTimestamp64Milli(now64(3)))` en las 5
  ocurrencias. Bug detectado por el nuevo integration test
  `auth_session::logout_revokes_session` ([backend/tests/auth_session.rs](backend/tests/auth_session.rs)).

### Added
- **Usuario unificado multi-device (goal 10.E.1)**. Hasta ahora un mismo
  humano que entraba desde web (anon-A → `identify(user_42)`) y desde mobile
  (anon-B → `identify(user_42)`) quedaba como dos historias separadas en
  `product_events`, sin forma de responder "todos los eventos de user_42 en
  cualquier device" sin un full scan. Cambios:
  * **Schema**:
    [`clickhouse/init/86-product-events-aux.sql`](clickhouse/init/86-product-events-aux.sql)
    extiende `faro.product_users` con `anonymous_ids Array(String)`,
    `sources Array(LowCardinality(String))` y `event_count UInt64`, y agrega
    `faro.product_user_aliases` (`ReplacingMergeTree(linked_at)` con PK
    `(project, anon)`) para el lookup reverso anon → distinct_id. Migración
    forward-only:
    [`clickhouse/migrations/015-product-users-multi-device.sql`](clickhouse/migrations/015-product-users-multi-device.sql).
    `EXPECTED[]` del test de migraciones ampliado con `product_user_aliases`.
  * **Worker** [`backend/src/workers/user_unifier.rs`](backend/src/workers/user_unifier.rs):
    cada `FARO_USER_UNIFIER_INTERVAL_SECS` (default 60s) agrega
    `product_events` en una ventana deslizante con overlap +30s, une
    `anonymous_ids`/`sources` con la fila existente (`product_users FINAL`),
    preserva `first_seen`, y re-INSERTA empujando `last_seen`. Cap defensivo
    de 5 000 users/tick para evitar IN-lists patológicos en bursts de
    onboarding. Cableado en
    [`backend/src/main.rs`](backend/src/main.rs) tras los demás workers.
  * **API**
    [`backend/src/api/product_users.rs`](backend/src/api/product_users.rs):
    `GET /api/v1/product-users` lista usuarios con filtros por rango,
    substring (distinct_id/properties) y `source` (semántica AND para
    aislar "cross-device only"); `GET /api/v1/product-users/:distinct_id`
    devuelve el row canónico + breakdown por device; `GET
    /api/v1/product-users/:distinct_id/events` resuelve los anon_ids del
    user (fallback a `product_user_aliases` si el worker aún no procesó)
    y filtra `product_events` por `distinct_id IN (…) OR anonymous_id IN
    (…)`. Es el endpoint que materializa el goal.
  * **Config**: nuevas vars
    `FARO_USER_UNIFIER_ENABLED` (default `true`) y
    `FARO_USER_UNIFIER_INTERVAL_SECS` (default `60`).
- **6º pilar: product events (schema)**. Nuevo schema en ClickHouse que
  completa lo que el backend (`backend/src/ingest/events.rs`,
  `backend/src/api/events.rs`, `backend/src/api/funnels.rs`) ya consumía pero
  cuyas tablas no existían:
  [`clickhouse/init/85-product-events.sql`](clickhouse/init/85-product-events.sql)
  define `faro.product_events` con `properties`/`user_properties`/`context`
  como JSON String (no Map, ver ADR-0012), `distinct_id`+`anonymous_id`
  separados (patrón PostHog), `trace_id`/`span_id` opcionales para linkear con
  spans OTel, TTL 365 días y `PROJECTION by_event`.
  [`clickhouse/init/86-product-events-aux.sql`](clickhouse/init/86-product-events-aux.sql)
  agrega `product_users` (ReplacingMergeTree con `last_seen` como versión),
  `product_sessions` (ReplacingMergeTree con `ended_at`), y las MVs
  `mv_product_events_per_day` (countState) +
  `mv_product_unique_users_per_day` (uniqExactState para cohorts viables).
  Migrations paralelas:
  [`clickhouse/migrations/013-product-events.sql`](clickhouse/migrations/013-product-events.sql),
  [`clickhouse/migrations/014-product-aux-tables.sql`](clickhouse/migrations/014-product-aux-tables.sql).
  `EXPECTED[]` de
  [`clickhouse/test-migrations.sh`](clickhouse/test-migrations.sh) ampliado
  con las 7 nuevas entradas para que el gate `migrations` del CI bloquee si
  alguna falta. Rationale completo en
  [ADR-0012](docs/adr/0012-product-events-sixth-pillar.md). Funcionalidad
  end-to-end (worker de sesionalización, SDKs, frontend) llegará en
  iteraciones siguientes.
- **Detección de docs huérfanas en CI** ([scripts/check-orphan-docs.sh](scripts/check-orphan-docs.sh)).
  Verifica que cada archivo bajo `docs/` (md/mdx + imágenes) esté
  referenciado por basename o ruta repo-relativa desde algún `.md`/`.mdx`
  fuera de `node_modules`/builds. Job nuevo `docs` en
  [.github/workflows/ci.yml](.github/workflows/ci.yml) lo dispara cuando
  el PR toca `docs/**`, `*.md` o el propio script, y bloquea el merge si
  añadís un doc que nadie linkea. Un doc no descubrible es como no
  existir.
- **Fuente única de verdad para variables de entorno**. [`.env.example`](.env.example)
  reorganizado con 15 secciones y comentario descriptivo por variable
  cubre las ~45 vars que entiende Faro (compose, backend, frontend,
  workers, notificaciones, self-observability, smoke test). Las vars
  comunes están descomentadas; los tunables avanzados commented-out con
  el default del código indicado. Página
  [`docs/reference/environment.md`](docs/reference/environment.md) se
  autogenera con [`scripts/gen-env-reference.sh`](scripts/gen-env-reference.sh).
  CI ([`scripts/check-env-reference.sh`](scripts/check-env-reference.sh)
  vía job `env-reference`) falla el PR con diff si los dos archivos
  están desincronizados. README, `docs/deployment.md`, `infra/README.md`
  y `.env.prod.template` ahora linkean a la página generada en lugar de
  mantener tablas paralelas que se desincronizaban.
- **Pirámide de integration tests del backend** ([backend/tests/](backend/tests/)).
  El crate se reorganizó como `lib + bin` para que `tests/*.rs` pudiera
  importar `faro::*` y ejercitar los handlers reales contra ClickHouse
  efímero. Helper compartido [tests/common/mod.rs](backend/tests/common/mod.rs)
  arranca el router en puertos efímeros con un `project_slug` UUID por test
  (aislamiento sin cleanup). 7 archivos, 36 tests verde en ~6 s:
    - `ingest_logs.rs` — POST `/api/v1/ingest/logs` → SELECT en CH; rechazo
      sin bearer / token desconocido.
    - `ingest_otlp.rs` — payloads OTLP/HTTP+JSON reales para logs, traces
      y metrics → `faro.logs` / `faro.spans` / `faro.metrics`.
    - `api_logs_query.rs` — filtros `service` / `min_severity` / `query` /
      `trace_id` + orden DESC + `limit`.
    - `auth_session.rs` — login + `/me` + logout (verificado contra DB con
      `FINAL`) + expiración por `expires_at` en el pasado.
    - `projects_token.rs` — `POST /projects/:slug/rotate` invalida el viejo
      (401) y emite uno que autentica (200).
    - `sql_injection.rs` — 5 payloads × 14 combinaciones (endpoint × param)
      sobre `logs/traces/metrics/errors`; canarios anti-leak (`password_hash`,
      `argon2`); test final confirma que `faro.logs`/`faro.users` siguen
      consultables tras los `DROP TABLE` payloads.
    - `otlp_negative.rs` — parser OTLP malformado: JSON inválido, campos
      required ausentes, severity fuera de rango, timestamps no-numéricos,
      `body` como string raw, tipos mezclados. Asserts contra 400/422 sin
      panic ni 5xx.
- **vitest configurado en el frontend** ([frontend/vitest.config.ts](frontend/vitest.config.ts)).
  Config dedicada con `environment: 'node'` + alias para resolver
  `$app/environment`, `$app/navigation` y `$env/dynamic/public` a los stubs
  en `src/lib/__mocks__/`. Scripts `npm test` / `npm run test:watch`. 57
  tests en `palette.test.ts` + `stores.test.ts` corren en <1 s.
- **Command Palette: scoring + `search()` + `nextHighlight()` + tests determinísticos**
  ([frontend/src/lib/palette.ts](frontend/src/lib/palette.ts),
  [palette.test.ts](frontend/src/lib/palette.test.ts)).
  `search(commands, query)` filtra + ordena por score AND-por-tokens (palabra
  completa 100 > prefijo de label 80 > substring label 60 > sub 30 > keywords
  25 > shortcut 15 > group 5). Saltos directos cortocircuitan en 1000 ya que
  `jumpCommands(query)` pre-validó la relevancia. Desempates determinísticos:
  score → longitud de label → id lexicográfico → orden de entrada. Helper
  `nextHighlight(current, length, dir)` extraído del componente para que la
  navegación con flechas (↑/↓, también `j`/`k` y `ctrl+n`/`ctrl+p`) sea
  testeable sin DOM. 36 tests nuevos cubriendo `matches`, `score`, `search`,
  `jumpCommands` (todas las sintaxis + strip de comillas) y `nextHighlight`
  (clamp sin wrap, lista vacía).
- **Smoke test post-deploy contra el dominio público**
  ([scripts/smoke-post-deploy.sh](scripts/smoke-post-deploy.sh) + step nuevo
  en [.github/workflows/deploy.yml](.github/workflows/deploy.yml)). Round-trip
  real tras `readyz`: `POST /api/v1/auth/login` setea cookie `faro_session`,
  `POST /api/v1/ingest/logs` con bearer de proyecto devuelve `accepted >= 1`,
  `GET /api/v1/logs?query=<marker>` poll de 30 s encuentra el log del paso
  anterior, `GET /healthz` confirma que `protocol.current` matchea
  `FARO_EXPECTED_PROTOCOL` (default 1, bumpea cuando subas `PROTOCOL_CURRENT`
  en [backend/src/versions.rs](backend/src/versions.rs)). Cubre el caso
  "readyz verde pero ingesta/auth/wire están rotos". Exit codes deterministas
  por tipo de fallo (1 config, 2 healthz, 3 login, 4 ingest, 5 query). Las
  creds del smoke user + ingest token viven en `/opt/faro/.env.prod` —
  variables documentadas en [.env.prod.template](.env.prod.template); mientras
  no estén, el script imprime `::warning::` y se salta los pasos auth-dependientes
  (sólo corre healthz/protocol). Step `Smoke diagnostics on failure` vuelca
  logs de backend + CH + healthz/readyz para que el rollback manual tenga
  contexto. **Rollback automático queda fuera de scope a propósito**: las
  imágenes Docker no están tageadas por commit-SHA, así que un rollback real
  supone `git revert + push` (más invasivo que el propio fix; ver mensaje
  de error del step para los detalles).
- **`docker-compose.test.yml`** + [`scripts/run-integration-tests.sh`](scripts/run-integration-tests.sh)
  para correr el backend stack (CH efímero + cargo test) en local sin tener
  Rust instalado. Aplica el schema sentencia-por-sentencia (CH HTTP no acepta
  multi-statement por default) y verifica que `faro.logs` existe antes de
  invocar `cargo test`.
- **`cargo-nextest` paraleliza los tests del backend cross-binary**
  ([backend/.config/nextest.toml](backend/.config/nextest.toml),
  [.github/workflows/ci.yml](.github/workflows/ci.yml)). `cargo test` corría
  los 11 binarios de [backend/tests/*.rs](backend/tests/) **uno a uno** (aunque
  dentro de cada uno usara `num_cpus` threads). Con la fixture aislando por
  `project_id` UUID y bind a puertos efímeros, todos los tests son seguros
  de correr concurrentes contra el mismo ClickHouse compartido — `nextest`
  los pone todos en un único pool con num_cpus workers. Speedup esperado
  5-10× sobre runners de CI. Perfil `ci` con `retries=2` para amortiguar
  flakes puntuales del CH service en GHA + `junit.xml`. Comando local
  equivalente: `cd backend && cargo nextest run` (instalar una vez con
  `cargo install cargo-nextest --locked`); `cargo test --tests` sigue
  funcionando como fallback. Doc completa en
  [docs/development.md](docs/development.md) → "Tests en paralelo".
- **6 invariantes mínimas testeadas en los 7 SDKs** ([sdks/](sdks/)). Cada
  SDK tiene ahora una suite que cubre: (1) init con opts inválidas → error
  claro, (2) log + flush + assert payload del wire, (3) queue overflow,
  (4) retry on 5xx, (5) auto-captura de excepciones (vía
  `captureException` que es lo que invoca el handler global), (6) `close()`
  graceful sin pérdida de eventos en cola. Totales: Node 12, Next.js 10,
  Expo 10, Python 12, Go 12, Flutter 11, Kotlin 10. Bugs encontrados y
  arreglados al pasar (todos paridad cross-SDK):
  - **Node/Next.js/Expo/Flutter/Kotlin** ahora validan `endpoint`/`token`/
    `service` en `init()` y lanzan un error claro (antes: TypeError críptico
    o silencio). Python/Go ya validaban.
  - **Kotlin** específicamente: `Channel(capacity=1)` ignoraba `maxQueueSize`
    y perdía eventos bajo carga; `init()` no era re-iniciable (scope+channel
    `val` cancelados tras `close()`); `send()` no re-encolaba ante 5xx;
    `flush()` no esperaba al batch en vuelo (close mataba mid-POST);
    `beforeSend → null` no descartaba el evento (`?:` lo rescataba). Los 5
    fixes en [sdks/kotlin/src/main/kotlin/com/iaportafolio/faro/Faro.kt](sdks/kotlin/src/main/kotlin/com/iaportafolio/faro/Faro.kt)
    + tests que los habrían capturado antes del primer release a Maven
    Central.
  - **Expo** timer interno con `.unref()` (paridad SDK Node; evita crashes
    de libuv en tests Windows).

### Changed
- **CI `ci.yml` con `dorny/paths-filter@v3`** ([.github/workflows/ci.yml](.github/workflows/ci.yml)).
  Un job `changes` detecta qué cambió y los demás jobs se condicionan con
  `if: needs.changes.outputs.X == 'true'`. Una PR de docs no levanta CH ni
  recompila Rust; una PR que toca solo `frontend/` no corre los tests del
  backend. Job final `ci` agrega resultados para branch protection: marca
  SOLO `ci` como required check (los individuales aparecen como `skipped`
  cuando sus paths no cambiaron, lo cual normalmente bloquearía el merge si
  estuvieran listados como required).
- **Backend reorganizado como `lib + bin`** ([backend/src/lib.rs](backend/src/lib.rs)).
  Los módulos antes privados al binario ahora son `pub mod` en `lib.rs`;
  `main.rs` los consume con `use faro::*`. Refactor estructural sin cambio
  de comportamiento — habilitó la suite de integration tests.
- **CI `sdk-tests.yml` integration job ahora cachea la imagen `faro-backend`
  vía Docker buildx + GHA cache**
  ([.github/workflows/sdk-tests.yml](.github/workflows/sdk-tests.yml),
  [docker-compose.sdk-integration.yml](docker-compose.sdk-integration.yml)).
  Pre-buildea con `docker/build-push-action@v6` (`load: true`,
  `cache-from`/`cache-to=type=gha,scope=sdk-integration-backend,mode=max`)
  y arranca compose con `--no-build`. El builder propio de `docker compose`
  no entiende los exporters de buildx, por eso la separación. Cold ~10 min
  sigue igual (alguien tiene que compilar las deps la primera vez); runs
  incrementales con cambios sólo en `src/*.rs` bajan a 1-2 min (hit en la
  layer de `cargo chef cook`). PRs heredan automáticamente la cache de la
  default branch. El servicio backend del compose ganó un
  `image: faro-backend:integration` para que el daemon local encuentre la
  imagen pre-cargada; `build:` se conserva para `docker compose up --build`
  en flujo local.
- **`CommandPalette.svelte` usa `search()` y `nextHighlight()` de `palette.ts`**
  en vez del antiguo `allCommands.filter((c) => matches(c, query))` y los
  `Math.min/max` inline. Reordena los resultados dentro de cada grupo por
  score sin cambiar la visualización (los grupos siguen apareciendo en el
  orden de `groupOrder`).
- **CI del frontend**: el `if grep -q '"test"'` defensivo se eliminó.
  `npm test` ahora es paso obligatorio, no opcional.

### Security
- **Endurecimiento integral del backend** ([ADR-0009](docs/adr/0009-security-hardening.md)).
  Siete cambios coordinados: parametrización server-side de TODOS los queries con input
  de usuario (`escape_sql()` eliminado, sintaxis `{name:Type}` de ClickHouse); security
  headers (CSP estricto + HSTS gated + X-Frame-Options + X-Content-Type-Options +
  Referrer-Policy) sobre el router del dashboard; auth nativa email/password con Argon2id
  + cookie HttpOnly/Secure (supersedea [ADR-0005](docs/adr/0005-no-native-auth.md)); 2FA
  TOTP opcional con 10 recovery codes one-shot y rate limit 5/min/user; PII redaction
  server-side por proyecto (7 built-ins + custom regex); allowlist de orígenes por
  proyecto para el RUM SDK; `cargo audit --deny warnings` en CI.
  Defaults backward-compatible: las features opt-in (2FA, redaction, origin check,
  HSTS) quedan off por proyecto/instancia hasta que el operador las active.
  Migraciones idempotentes: `clickhouse/migrations/00{7,8,9}-*.sql`.
- **Sanitización del body en `highlightBody()`** del frontend ahora acepta
  `string | null | undefined`. Un ingester antiguo que omita el campo `body` ya no
  rompe el render con `body.matchAll is not a function`.

### Added
- **Dark mode + sistema de temas** persistido por usuario en DB
  (`faro.user_preferences.theme` ∈ `light | dark | system`). Tokens CSS dual
  en `app.css` (`:root` dark default + `[data-theme="light"]` + media query
  `prefers-color-scheme: light` para SSR sin JS). Toggle compacto al pie del
  sidebar (☀ ◐ ☾) y selector ampliado en `/settings/appearance`. Bootstrap
  desde localStorage antes del primer render (sin flash) e hidratación desde
  backend tras `me()` solo si difiere — ver `frontend/src/lib/theme.ts`.
- **Sección `/settings/` con sub-navegación** que agrupa Apariencia,
  Proyectos, Usuarios, Alertas e Integraciones. Las rutas viejas `/alerts`,
  `/users`, `/projects` quedan como redirect a `/settings/*` para no romper
  bookmarks.
- **Atajos de teclado tipo Linear/Sentry**: `g+letra` para navegar
  (`g l`/`t`/`m`/`e`/`o`/`s`/`a`/`p`/`u`/`i`/`r`), `/` enfoca el campo
  marcado con `data-search-input` en la página actual, `?` abre la cheatsheet,
  `Esc` cierra overlays. En las listas: `j/k` mueve selección, `Enter` abre
  detalle. La píldora flotante `g · espera tecla…` confirma estado armado.
  Ver [frontend/src/lib/keyboard.ts](frontend/src/lib/keyboard.ts) y
  `frontend/src/lib/components/KeyboardShortcuts.svelte`.
- **Command Palette (`⌘/Ctrl+K`)** con índice dinámico de proyectos,
  servicios (filtrados por proyecto activo), monitores y reglas de alerta,
  cargado lazy al abrir y cacheado 60 s (TTL invalidado al cambiar proyecto).
  Soporta sintaxis de salto directo: `traces:<id>` (abre traza + ofrece
  ver logs de la traza), `logs:trace=<id>`, `errors:<fingerprint>`. Fuzzy
  multi-token sobre `label + sub + group + keywords`. Atajos asociados
  visibles en cada fila. Tope visual de 80 resultados con aviso "+N más".
- **Flamegraph real para trazas** ([frontend/src/lib/components/Flamegraph.svelte](frontend/src/lib/components/Flamegraph.svelte)):
  reemplaza la "lista de spans con divs proporcionales" por un árbol
  jerárquico construido con `parent_span_id`, sangría por profundidad
  (10 px/nivel), color por servicio vía hash determinístico DJB2 → HSL,
  zoom horizontal con rueda centrado en el cursor + botones `−ǀ100%ǀ+`,
  pan con click-drag (con listeners globales para que soltar fuera no
  deje el drag pegado), regla de tiempos con 6 ticks y guías verticales,
  clipping de spans parcialmente fuera del viewport. Tooltip con duración,
  estado, conteo de eventos y links salientes. El drawer del span ahora
  muestra eventos parseados (JSON) y links navegables a otras trazas.
- **Defaults de exploración persistidos por usuario**: `default_project` y
  `default_time_range` en `faro.user_preferences`, hidratados al login
  **solo si la URL no los traía** (un deep link siempre gana). Editables
  en `/settings/appearance` → "Defaults de exploración" + botón "Aplicar
  a esta sesión".
- **Sincronización filtros ↔ URL** en todas las páginas de exploración.
  `selectedProject` y `timeRange` ya no viven en localStorage; el query
  string `?project=…&range=…` es la fuente de verdad compartida, y cada
  página añade sus filtros locales (`?service=`, `?status=`, `?metric=`,
  `?selected=<timestamp>` en logs). `replaceState` (no `pushState`) para
  no llenar el back-stack mientras se teclea. Ver
  [frontend/src/lib/url-filters.ts](frontend/src/lib/url-filters.ts).
- **Drawer de detalle persistente en `/logs`**
  ([frontend/src/lib/components/LogDetailDrawer.svelte](frontend/src/lib/components/LogDetailDrawer.svelte)):
  resizable con drag (ancho persistido en `localStorage:faro:drawer-width`),
  `padding-right` reactivo en `<body>` para que la lista se *encoja* en vez
  de quedar tapada, navegación `j/k` actualiza el contenido sin cerrar el
  drawer, `Esc` cierra, `Cmd+C` copia el log como JSON. Estructura: header
  (severity + ts + service · env · host), mensaje (auto-format JSON),
  stack trace colapsable, tabla de atributos con `▾` (filtrar) y `⎘`
  (copiar) por valor, sección **"Logs ±2 min alrededor"** que carga al
  vuelo con `fetchLogs({from, to, service?})` y permite saltar a otro log
  del contexto, links contextuales (ver traza, más logs del servicio,
  logs del host). Deep link `/logs?selected=<timestamp>`.
- **Empty states con onboarding contextual**
  ([frontend/src/lib/components/OnboardingEmpty.svelte](frontend/src/lib/components/OnboardingEmpty.svelte))
  en logs/traces/errors/metrics/resumen. Snippet de instalación del SDK
  del proyecto seleccionado con token inyectado (tabs por lenguaje:
  Node, Next.js, Python, Go, Flutter, Kotlin, Expo, OTLP, curl), comando
  curl de prueba y link a `/docs`. Si no hay ningún proyecto creado,
  ofrece CTA grande para crear el primero. Catálogo extraído a
  [frontend/src/lib/sdk-snippets.ts](frontend/src/lib/sdk-snippets.ts)
  (fuente única reutilizada por `/settings/projects` y el onboarding).
- **Sistema global de toasts**
  ([frontend/src/lib/toasts.ts](frontend/src/lib/toasts.ts) +
  `frontend/src/lib/components/Toasts.svelte`). API ergonómica
  `toast.success/error/info/warning(message, opts?)` y
  `toast.fromError(prefix, err)`. Stack arriba-derecha (abajo en mobile),
  4 kinds con borde lateral coloreado, descripción opcional, auto-dismiss
  con duraciones por kind (success/info 4 s, warning 5 s, error 6 s),
  `duration: 0` pegajoso, hover pausa el timer, acción opcional. Disponible
  incluso en `/login` (montado fuera del bloque autenticado). Migradas
  todas las acciones críticas: crear/actualizar/eliminar/rotar de
  projects, users, alerts, monitors, integrations (canales y Telegram),
  cambio de estado de issues, copiado al portapapeles, defaults guardados,
  cambio de tema, exportes de logs, etc.
- **Skeletons en vez de "Cargando…"**: componentes
  `Skeleton.svelte`/`SkeletonTable.svelte`/`SkeletonCards.svelte`/
  `SkeletonLogRows.svelte` con shimmer global (`.skeleton` en `app.css`,
  respeta `prefers-reduced-motion`). Cada página mantiene el shape exacto
  de su tabla/card para evitar reflow al sustituir por datos reales. Solo
  se muestran cuando `data.length === 0` durante la primera carga (no
  parpadean en recargas/live tail).
- **Ingesta OTLP/gRPC** en `:4317` ([ADR-0010](docs/adr/0010-otlp-grpc-ingest.md);
  supersedea la decisión "únicamente JSON" de [ADR-0004](docs/adr/0004-otlp-http-json-ingest.md)).
  Los SDKs oficiales de OpenTelemetry (Java, Go, Python, .NET, Ruby) usan
  gRPC + protobuf por defecto y antes fallaban silenciosamente contra
  `:4318` (HTTP/JSON). Reusa los mismos canales/storage que el path
  HTTP. Auth con `Authorization: Bearer <token>` en gRPC metadata.
  Implementación con `tonic 0.12` + stubs pre-generados de
  `opentelemetry-proto` (sin `protoc` en build). Configurable vía
  `FARO_OTLP_GRPC_ADDR`.
- **Rate limit por proyecto en la ingesta** (token bucket GCRA in-memory
  con `governor`). Cuenta **records** (no requests), aplicado a OTLP/HTTP,
  OTLP/gRPC y `/logs` con el mismo bucket — un cliente no esquiva el
  límite saltando de transporte. HTTP devuelve `429 Too Many Requests` +
  header `Retry-After`; gRPC devuelve `RESOURCE_EXHAUSTED` con metadata
  `retry-after`. Default 5000 rec/s por proyecto (burst 2×). Configurable
  vía `FARO_INGEST_RATE_PER_SECOND`.
- **Endpoint Prometheus `/metrics`** con `axum-prometheus` + crate
  `metrics` ([ADR-0011](docs/adr/0011-prometheus-self-monitoring.md);
  supersedea el "dogfooding puro" de [ADR-0007](docs/adr/0007-self-observability.md)).
  Exporta métricas HTTP estándar (`faro_http_*`) + custom:
  `faro_ingest_records_total{project,signal,outcome}`,
  `faro_rate_limited_total{project,signal}`,
  `faro_clickhouse_insert_duration_seconds{table}`,
  `faro_clickhouse_rows_inserted_total{table}`,
  `faro_clickhouse_errors_total{table,operation}`. Bearer auth opcional
  via `FARO_METRICS_TOKEN`. Stack `prom/prometheus` + `grafana` montado
  en `iaportafolio` server (Traefik) con dashboard `Faro · Overview`.
  Cardinalidad de labels acotada por diseño (sólo project/signal/table) —
  ver [backend/src/observability.rs](backend/src/observability.rs).
- **Compactador MinHash de fingerprints** (`workers::fingerprint_compactor`):
  agrupa errores semánticamente equivalentes con distinto fingerprint
  exacto (e.g. `NullPointerException` con `$$Lambda$123` vs
  `$$Lambda$987` tras rebuild). MinHash K=128 sobre shingles de
  `(exception_type, message, stack)` normalizados; cluster compartido si
  Jaccard ≥ 0.85. Tabla `faro.error_clusters` con `(fingerprint,
  cluster_id, minhash, representative_*)`. Implementación pura sin
  deps extra (SHA-256 + hashing universal Carter-Wegman). Configurable
  vía `FARO_FP_COMPACTOR_*`.
- **Detector de servicios stale** (`workers::stale_detector`): cada hora
  consulta `faro.services_seen` (MV agregada con `maxState(timestamp)`
  sobre logs+spans+metrics) y emite eventos `stale`/`recovered` en
  `faro.service_stale_events` al cruzar el umbral (default 24h sin
  tráfico). Estado inicial recuperado del log de eventos para no
  spamear tras restart. Configurable vía `FARO_STALE_DETECTOR_*`.
- **Pre-agregaciones ClickHouse** (Materialized Views):
  - `errors_hourly{hour, project_id, service_name, severity_text}` con
    count de errores (severity_number ≥ 17).
  - `spans_latency_hourly{hour, project_id, service_name, span_name}`
    con `quantilesTDigestState(0.5, 0.95, 0.99)` + counts de spans y
    errores.
  - `monitor_uptime_daily{day, project_id, monitor_id}` con
    éxitos/fallos + quantiles de duración.
  - `services_seen{project_id, service_name}` con `maxState(timestamp)`
    cruzando 3 MVs paralelas (logs/spans/metrics).
- **Notificaciones extensibles**: refactor de `notify.rs` a módulo
  `notify/` con trait `Notifier` + plugins concretos:
  - `webhook` (genérico, POST JSON con body estructurado o template
    `{placeholders}` interpolados; headers extra arbitrarios).
  - `slack` (Incoming Webhooks, mrkdwn + emoji por severidad).
  - `discord` (webhooks con embeds coloreados).
  - `pagerduty` (Events API v2, `trigger`/`resolve` con `dedup_key`
    estable por regla).
  - `opsgenie` (Alert API v2, soporta cuentas EU, responders, tags).
  - `email_resend` (HTTP API de Resend; HTML + text plain).
  - `telegram` (extraído del módulo viejo, sigue funcionando inline
    con `tg://<chat>@<token>` además de como canal).
- **Canales de notificación multi-instancia**: tabla
  `faro.notification_channels` (`id` slug PK, `kind` selecciona plugin,
  `config` JSON). Cada regla referencia uno con `channel://<id>` en
  `notification_targets`. Endpoints CRUD + test bajo
  `/api/v1/integrations/channels[/:id[/test]]` y UI completa en
  Settings → Integraciones con form dinámico por kind y enmascarado
  automático de secretos.
- **Healthcheck con dependencias**: nuevo endpoint `/readyz` (ping a
  ClickHouse + Redis con latencia por dep), 200 si CH responde, 503
  si CH falla. `/healthz` queda como liveness puro (sin verificar deps).
  Docker Compose healthcheck y smoke test del CI ahora apuntan a
  `/readyz` para que "healthy" signifique "puede servir tráfico".
- **Live Tail con filtros server-side y resaltado** en `/logs`: modo regex
  (compilado server-side con `size_limit` 1 MiB), pausa con "look-back"
  (los eventos del stream van a un backlog mientras está congelado),
  export JSON del último minuto y share-link de la vista actual con
  re-hidratación de filtros desde la URL. El filtro `project` ahora
  también se aplica al SSE (antes se ignoraba).
- **Service map auto-descubierto** en `/service-map`: grafo
  servicio→servicio con `calls`, `error_rate`, `p50/p95/p99` por arista,
  inferido del self-join de `faro.spans` por `parent_span_id`.
  Visualización SVG custom con simulación force-directed propia (sin
  dependencias nuevas en el frontend).
- **Session Replay con rrweb**: tabla `faro.session_replays` (TTL 7d),
  endpoint `POST /api/v1/ingest/replay` (body limit 16 MiB, rate-limit
  compartido con logs), player en `/replays/[session_id]` y link
  contextual desde el detalle de errores cuando hay sesión grabada.
  Opt-in en el SDK con `captureSessionReplay: true` (ver
  [sdks/nextjs/SESSION-REPLAY.md](sdks/nextjs/SESSION-REPLAY.md)).
- **Detector de anomalías por z-score** (`workers::anomaly_detector`):
  compara la tasa actual de errores, p95 latencia y volumen de logs por
  servicio contra la misma franja horaria de los últimos 7 días; dispara
  incidentes automáticos en `faro.alert_incidents` cuando z>3, los
  resuelve con hysteresis (z<1.5). Configurable vía `FARO_ANOMALY_*` —
  ver [docs/anomaly-detection.md](docs/anomaly-detection.md).
- **`farocli`** (`cli/`): binario Rust para tail/query/monitor management
  desde la terminal. `farocli logs -p mi-proy --service api --severity
  ERROR --follow` reusa el SSE de `/logs/live`. Auth por cookie de sesión
  (mismo flujo que el dashboard), persistida en `~/.config/farocli/`.
- SDK `@iaportafolio/nextjs` con instrumentación server y client.
  El RUM del navegador (Web Vitals, breadcrumbs, `FaroErrorBoundary`)
  vive dentro del propio paquete desde `0.3.0` — antes fue un paquete
  aparte `@iaportafolio/browser` que se fusionó (ver
  [sdks/nextjs/CHANGELOG](sdks/nextjs/CHANGELOG.md)).
- Backend: scaffolding de **OpenAPI con `utoipa`** + Swagger UI montado en
  el enrutador principal (#17).
- Backend: scaffolding de **self-observability** — el backend exporta sus
  propios logs/trazas/métricas vía OTLP a sí mismo (#18).
- Backend: **contrato de versión SDK ↔ backend** vía header
  `Faro-Protocol-Version`, para detectar drift entre cliente y servidor
  antes de ingerir datos malformados (#19).
- Deploy: tabla `faro.integrations` añadida y `curl` instalado en la
  imagen del backend para healthchecks dentro del contenedor.
- Workflow `ci.yml` con `cargo fmt/clippy/test`, `npm run check/build` y
  validación de `docker-compose`.
- Configuración Dependabot para 9 ecosistemas (GitHub Actions, Cargo, npm
  x3, pip, gomod, gradle, docker).
- `SECURITY.md` con política de reporte de vulnerabilidades.
- `docs/development.md` y `docs/deployment.md`.
- `rust-toolchain.toml`, `.nvmrc` y `.editorconfig` para pinear toolchain.
- Templates de issue/PR y `CODEOWNERS`.

### Changed
- **`selectedProject` / `timeRange` ya no usan `localStorage`**. Pasan a ser
  writables en memoria hidratados desde (en este orden de prioridad):
  query string → `faro.user_preferences.default_*` → fallback. La URL se
  reescribe con `replaceState` en cada cambio para que F5 reproduzca la
  vista. Compartir un link entre máquinas ahora preserva el contexto;
  cambiar de máquina ya no pierde el filtro (lo lee del backend).
- **`/healthz` ahora es liveness puro** (no verifica deps). El payload
  con versión + rango de protocolo se mantiene para no romper SDKs que
  lo leen al arranque. La verificación de ClickHouse/Redis vive en el
  nuevo `/readyz` — readiness probes externos y `depends_on` del
  frontend en Compose deben usar `/readyz`.
- `notify.rs` (módulo plano) → `notify/` con trait `Notifier` y un
  archivo por plugin. La firma pública `notify::dispatch(state,
  targets, incident)` se preserva; cambian sólo los internals.
  Los targets viejos siguen funcionando: `tg://<chat>` (Telegram),
  `https://...` (webhook genérico inline). El nuevo formato
  `channel://<id>` referencia entradas de `notification_channels`.
- `backend/Cargo.toml`: licencia `MIT` → `LicenseRef-Proprietary` (alineado
  con el `LICENSE` raíz que es propietaria) y `publish = false` para evitar
  empujes accidentales a crates.io.
- `.gitignore` cubre ahora los artefactos de los 7 ecosistemas de SDKs y
  bloquea explícitamente `.env.prod` / `.env.*.local`.
- Scope npm renombrado de `@faro/*` a `@iaportafolio/*` (el scope `@faro`
  estaba tomado en el registry).
- Deploy: el `rsync` del runner excluye `.gradle/` — los permisos del host
  bloqueaban `--delete` sobre ese directorio.

### Fixed
- SDK Kotlin: publicación a Maven Central migrada al Central Portal nuevo
  (`com.gradleup.nmcp`); el OSSRH legacy ya no acepta uploads para
  namespaces nuevos.

## [0.1.0] — 2026-05-22

Primera versión etiquetable del core. Lo incluido aquí es el estado al
momento de crear este changelog; cambios anteriores quedan en el historial
de git (`git log`).

### Added
- Backend Rust (axum + tokio) con dos listeners HTTP:
  - `:8080` API REST + SSE para el dashboard y endpoint nativo de ingesta.
  - `:4318` receptores OTLP/HTTP+JSON para logs, traces, métricas.
- Workers en background: writer batched a ClickHouse, runner de monitores
  HTTP, evaluador de alertas, indexador de error groups.
- Almacenamiento en ClickHouse con esquemas para logs, spans, métricas,
  error events, monitores y reglas de alerta.
- Frontend SvelteKit con vistas de dashboard, logs (live tail), traces,
  métricas, errores, monitores y reglas.
- Webhook notifications (Slack/Discord/genérico) para incidentes.
- Docker Compose para dev (`docker-compose.yml`) y prod
  (`docker-compose.prod.yml`).
- Auto-deploy via self-hosted runner en `infra-iaportafolio` con
  migraciones idempotentes y healthcheck via dominio público.
- SDKs en Node, Python, Go, Flutter, Kotlin y Expo con publicación
  automatizada en sus registries respectivos.
