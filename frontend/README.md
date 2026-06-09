# Faro · Frontend (`faro-frontend`)

Dashboard de **Faro**, la plataforma self-hosted de observabilidad + product
analytics. Es una **SPA en SvelteKit 5** (TypeScript) que se renderiza solo en el
cliente y habla con el backend (Rust/Axum) por su **API REST** usando una cookie
de sesión. No hay render en servidor (`ssr = false`).

> Este README es el **mapa + glosario** del frontend: qué es y qué hace cada cosa,
> y qué significan los nombres crípticos (`[fp]`, `distinct_id`, `slug`, funnel,
> flamegraph, OTLP, SSE…). Cada archivo de `src/` lleva además una cabecera JSDoc
> en español con su propósito.

---

## Cómo se ejecuta

```bash
npm run dev      # servidor de desarrollo (Vite) en 0.0.0.0:5173
npm run build    # build de producción (adapter-node → build/)
npm run preview  # sirve el build localmente
npm run check    # type-check (svelte-kit sync + svelte-check)
npm test         # tests unitarios (Vitest)
```

> Gate de CI / `AGENTS.md`: tocar `src/**` obliga a `npm test` **+** `npm run
> check` **+** `npm run build` en verde. La lógica testeable vive en `src/lib/*.ts`
> (hay un `*.test.ts` por módulo).

## Variables de entorno

| Variable           | Para qué sirve | Default |
|--------------------|----------------|---------|
| `PUBLIC_API_BASE`  | URL base del backend al que pega el cliente. La lee `src/lib/api.ts`. Si no está, cae al host actual en el puerto `:8080`. Es `PUBLIC_*` → se inyecta en el bundle del navegador. | `http://localhost:8080` |
| `ORIGIN`           | Origen público del propio frontend; lo exige `@sveltejs/adapter-node` para validar formularios/CSRF en producción. | `http://localhost:3000` |
| `PORT`             | Puerto donde escucha el server Node del build de producción. | `3000` |

`src/.env.local` fija `PUBLIC_API_BASE` para desarrollo. En Docker estas variables
las pone `docker-compose*.yml` (ver raíz del repo).

---

## Mapa de carpetas

```text
src/
├── app.html              Plantilla HTML raíz (shell de la SPA).
├── app.css               Estilos globales y variables de tema (claro/oscuro).
├── lib/                  Lógica + componentes reutilizables (alias de import: $lib).
│   ├── *.ts              Módulos de lógica pura y cliente de API (testeados).
│   ├── components/       Componentes Svelte compartidos entre páginas.
│   └── __mocks__/        Mocks de los módulos `$app/*` para los tests de Vitest.
└── routes/               Rutas = páginas (file-based routing de SvelteKit).
    ├── +layout.svelte    Marco común (auth, sidebar, paleta, tema).
    ├── +page.svelte      Una página por carpeta.
    └── [param]/          Segmento dinámico (p. ej. [id], [slug], [fp]).
```

**Convención SvelteKit:** dentro de `routes/`, `+page.svelte` es la página de esa
URL, `+layout.svelte` envuelve a sus hijas, `+server.ts` es un endpoint, y una
carpeta `[algo]` es un parámetro dinámico de la URL.

---

## Rutas (páginas) — qué es cada URL

### Observabilidad

| Ruta | Qué es |
|------|--------|
| `/` | **Resumen / dashboard**: contadores globales (logs, errores, servicios, issues, incidentes) + volumen de logs. |
| `/logs` | Explorador de **logs** con filtros, regex, modo live (SSE), histograma por severidad y drawer de detalle. |
| `/traces` | Lista de **trazas** (tracing distribuido) con filtros por servicio/estado/duración. |
| `/traces/[id]` | Detalle de una traza. `[id]` = `trace_id`; se dibuja como **flamegraph**. |
| `/service-map` | **Mapa de servicios**: grafo de dependencias (nodos=servicios, aristas=llamadas), grosor=volumen, color=error. |
| `/metrics` | Explorador de **métricas** (series temporales) por nombre/servicio/agregación. |
| `/errors` | Lista de **Issues** (grupos de error). |
| `/errors/[fp]` | Detalle de un Issue. `[fp]` = **fingerprint** (hash que agrupa errores equivalentes). |
| `/insights` | **Hallazgos** por servicio: cruza conversión de funnel + errores + latencia p95. |

### Producto (product analytics)

| Ruta | Qué es |
|------|--------|
| `/events` | Explorador de **product events** (filtros por nombre, `distinct_id`, properties; modo live por SSE). |
| `/funnels` | Constructor de **embudos de conversión**: secuencia de eventos → conversión, drop-off, tiempo de conversión. |
| `/retention` | **Retención por cohortes** (heatmap D1/D7/D30). |
| `/experiments` | Análisis **A/B** sobre una feature flag y un evento de conversión. |
| `/users` | Lista de **usuarios de producto** (usuarios finales, por `distinct_id`). |
| `/users/[distinct_id]` | Perfil + timeline de eventos de un usuario de producto. |
| `/sessions` | Lista de **sesiones** (duración, pageviews, errores, si tienen replay). |
| `/sessions/[session_id]/traces` | Trazas generadas durante una sesión (requiere `?project=`). |
| `/replays/[session_id]` | **Session replay**: reproduce la grabación de la sesión con rrweb-player. |

### Monitorización y configuración

| Ruta | Qué es |
|------|--------|
| `/monitors` | CRUD de **monitores** (checks de disponibilidad/SLO) con uptime. |
| `/settings` | Configuración (redirige a la primera pestaña). |
| `/settings/appearance` | Tema y rango temporal por defecto (preferencias personales). |
| `/settings/security` | Sesiones activas y **2FA (TOTP)**. |
| `/settings/projects` | CRUD de **proyectos** + rotación del token de ingesta. |
| `/settings/projects/[slug]/origins` | Lista blanca de **orígenes** (dominios) permitidos para ingesta. `[slug]` = proyecto. |
| `/settings/projects/[slug]/redaction` | Reglas de **redacción de PII** (builtin + custom). |
| `/settings/users` | CRUD de **usuarios del workspace** (≠ usuarios de producto). |
| `/settings/alerts` | Reglas de **alerta** e incidentes. |
| `/settings/integrations` | **Canales de notificación** (Telegram, etc.). |

### Acceso, docs y alias

| Ruta | Qué es |
|------|--------|
| `/login` | Acceso email+password con 2FA opcional. `login/+layout.svelte` quita el chrome global. |
| `/docs` | **Referencia pública de SDKs** (acceso anónimo). |
| `/docs.md`, `/llms.txt` | Mismo contenido de `/docs` en texto plano (endpoints `+server.ts`). |
| `/alerts`, `/projects` | **Alias históricos** → redirigen a `/settings/alerts` y `/settings/projects`. |

---

## Componentes (`src/lib/components/`)

| Componente | Qué hace |
|-----------|----------|
| `Sidebar` | Navegación lateral: menú del producto, selector de proyecto, tema. |
| `CommandPalette` | Paleta de comandos **⌘K / Ctrl+K** (buscador de acciones y saltos). |
| `KeyboardShortcuts` | Manejador global de teclado (sin UI): secuencias `g`+tecla, ⌘K, Esc. |
| `KeyboardHelp` | Diálogo de ayuda de atajos (se abre con `?`). |
| `TimeRangePicker` | Selector de rango temporal global (5m…7d). |
| `Chart` | Gráfico de líneas/área en SVG puro. |
| `LogVolumeHistogram` | Histograma de logs apilado por severidad, con drag-to-select. |
| `EventVolumeHistogram` | Ídem pero apilado por `event_name` (product events). |
| `Flamegraph` | Flamegraph jerárquico de los spans de una traza. |
| `LogDetailDrawer` / `EventDetailDrawer` | Panel lateral con el detalle de un log / evento. |
| `SeverityBadge` | Badge de color para el nivel de severidad de un log. |
| `Toasts` | Notificaciones efímeras (toasts). |
| `OnboardingEmpty` | Estado vacío / onboarding cuando aún no hay datos. |
| `Skeleton`, `SkeletonCards`, `SkeletonLogRows`, `SkeletonTable` | Placeholders de carga. |

## Módulos de lógica (`src/lib/*.ts`)

| Módulo | Qué hace |
|--------|----------|
| `api.ts` | **Cliente HTTP único** contra el backend + todos los tipos compartidos. Maneja 401→`/login`. |
| `stores.ts` | Stores globales (`currentUser`, `selectedProject`, `timeRange`) + formato de timestamp/duración/severidad. |
| `theme.ts` | Tema claro/oscuro/sistema (localStorage + backend). |
| `keyboard.ts` | Motor de atajos y secuencias de teclado. |
| `palette.ts` | Catálogo y búsqueda de comandos de la paleta ⌘K. |
| `url-filters.ts` | Sincroniza filtros de página con el query string (deep-link). |
| `insights.ts` | Formato y resumen de los insights por servicio. |
| `retention.ts` | Cálculo de retención por cohortes (tasa ponderada, madurez, color). |
| `sessions.ts` | Helpers de la vista de sesiones y sus enlaces (replay/eventos/trazas). |
| `product-users.ts` | Helpers de usuarios de producto y timeline por sesión. |
| `toasts.ts` | Store + API de los toasts. |
| `sdk-docs.ts`, `sdk-docs-markdown.ts`, `sdk-snippets.ts` | Fuente de verdad de la doc de SDKs (`/docs`) y snippets de instalación. |

---

## Glosario (los "nombres raros")

- **Pilar / dato:** Faro indexa varios tipos de telemetría: **logs**, **traces**
  (con **spans**), **metrics**, **errors**, **product events** y **sessions**.
- **span:** una operación temporizada dentro de una traza. Una **traza** (`trace_id`)
  agrupa todos sus spans; el **flamegraph** los dibuja anidados por padre.
- **Issue / `fingerprint` (`[fp]`):** los errores se agrupan por un *fingerprint*
  (hash de su "forma"); cada grupo es un **Issue** con estado abierto/resuelto/ignorado.
- **product event:** un evento de analítica de producto (p. ej. `checkout_completed`)
  con `properties` (JSON) arbitrarias.
- **`distinct_id`:** id estable del **usuario final** que envía el SDK. Es lo que
  identifica a un usuario en `/users/[distinct_id]` — distinto del usuario del
  workspace (los de `/settings/users`, que inician sesión en el dashboard).
- **`slug`:** identificador legible de un **proyecto** (un proyecto aísla datos y
  tiene su propio token de ingesta).
- **session / session replay:** una sesión agrupa la actividad de un usuario; si
  tiene **replay**, se grabó con rrweb y se puede reproducir en `/replays/...`.
- **funnel:** embudo de conversión = secuencia ordenada de eventos; se mide cuántos
  usuarios avanzan paso a paso y el **drop-off** (abandono).
- **retention / cohorte:** una cohorte son los usuarios vistos por primera vez un
  día; la retención mide cuántos vuelven a D1/D7/D30.
- **feature flag / experiment:** una *flag* reparte variantes; `/experiments` mide
  su impacto A/B sobre un evento de conversión.
- **insight:** hallazgo automático que cruza funnel + errores + latencia por servicio.
- **monitor / alert / incident:** un *monitor* chequea disponibilidad/SLO; al
  romperse genera un *incident* y dispara *alerts* por los canales de notificación.
- **origins:** lista blanca de dominios desde los que se acepta ingesta (CORS del SDK web).
- **redaction:** borrado de PII (datos personales) antes de almacenar.
- **OTLP (HTTP/gRPC):** *OpenTelemetry Protocol*, el formato estándar con el que los
  SDKs mandan telemetría al backend (no lo usa este frontend, pero aparece en la doc).
- **SSE (Server-Sent Events):** stream HTTP unidireccional; el modo "live" de logs y
  events lo usa (`EventSource`) para recibir datos nuevos en tiempo real.
- **2FA / TOTP:** segundo factor por código temporal (apps tipo Authenticator).
