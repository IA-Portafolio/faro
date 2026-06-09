# Deploy: página `/docs` (referencia de SDKs & API)

**Fecha:** 2026-06-05
**Autor del cambio:** documentación interactiva de los 7 SDKs y todos sus métodos.
**Alcance del deploy:** **solo el servicio `frontend`** del stack de producción `faro`.
El backend, ClickHouse y Redis **no se tocan** (llevan 7 días estables/healthy).

---

## Qué se añadió / cambió

| Archivo | Tipo | Descripción |
| --- | --- | --- |
| `frontend/src/lib/sdk-docs.ts` | **nuevo** | Datos de referencia de los 7 SDKs (métodos, firmas, opciones, severidades, matriz de producto). Extraído del código real de `sdks/`. |
| `frontend/src/routes/docs/+page.svelte` | **nuevo** | Página interactiva en la ruta `/docs` (selector de SDK, buscador, tablas de métodos, snippets copiables). |
| `frontend/src/lib/components/Sidebar.svelte` | editado | Nuevo item de nav **«SDKs & API»** (sección *Operación*) + icono `code`. |
| `frontend/src/lib/palette.ts` | editado | Entrada **«Ir a SDKs & API»** en la paleta de comandos (⌘K). |
| `frontend/.dockerignore` | **nuevo** | Excluye `node_modules`/`.svelte-kit`/`build` del contexto Docker. **Necesario:** sin él, `COPY . .` (posterior a `npm install` en el Dockerfile) metería el `node_modules` del host (glibc) en la imagen Alpine (musl) y rompería el build por mismatch de binarios nativos de Vite/Rolldown. |

Validación previa al deploy (con Node 20.20.2 instalado en `/usr/local`):

- `npm run check` (svelte-check) → **0 errores** (55 warnings a11y preexistentes, ninguno en archivos nuevos).
- `npm run build` → OK, incluye `entries/pages/docs/_page.svelte.js`.
- Dev server: `GET /docs` → HTTP 200.

---

## Artefactos de backup (creados antes del deploy)

> El stack **no taguea imágenes por SHA**: un rebuild sobrescribe `faro-frontend:latest`.
> Por eso el rollback se basa en la imagen tagueada y el tarball de source de abajo.

- **Imagen previa (rollback inmediato):**
  - `faro-frontend:rollback-pre-docs` → image id `4533f6632ba9` (creada 2026-05-29)
  - `faro-frontend:backup-20260605-180810` (mismo id, alias con timestamp)
- **Source previo:** `/opt/faro/backups/frontend-src-pre-docs-20260605-180810.tar.gz`

---

## Comandos de deploy (solo frontend)

```bash
cd /opt/faro
COMPOSE_PROD="docker compose -f docker-compose.prod.yml --env-file .env.prod"

# 1. Build solo del frontend (no toca backend/CH/redis)
$COMPOSE_PROD build frontend

# 2. Recreate solo del contenedor frontend
$COMPOSE_PROD up -d frontend

# 3. Estado
$COMPOSE_PROD ps
```

> Nota: el target `make prod-deploy` reconstruye **todo** el stack (`up -d --build --remove-orphans`).
> Aquí se acota a `frontend` a propósito para minimizar el blast radius en producción.

---

## Verificación post-deploy

```bash
# Contenedor arriba
docker ps --filter name=faro-frontend

# /docs responde dentro de la red (SSR off → devuelve el shell SPA con 200)
docker exec faro-frontend wget -qS -O /dev/null http://127.0.0.1:3000/docs 2>&1 | head -3

# Público (puede redirigir a /login si no hay sesión — es lo esperado, la ruta existe)
curl -s -o /dev/null -w "HTTP %{http_code}\n" https://faro.iaportafolio.com/docs
```

Smoke test completo (opcional, valida login→ingest→query; el check de healthz corre siempre):

```bash
FARO_BASE_URL=https://faro.iaportafolio.com bash scripts/smoke-post-deploy.sh
```

Cómo llegar en la UI: sidebar → *Operación* → **SDKs & API**, o ⌘K → "SDKs", o `/docs` directo.
La página está detrás de auth, igual que el resto del dashboard.

---

## Rollback

Si algo sale mal, restaurar la imagen previa y recrear (sin rebuild):

```bash
cd /opt/faro
COMPOSE_PROD="docker compose -f docker-compose.prod.yml --env-file .env.prod"

# Reapunta latest a la imagen previa y recrea el contenedor
docker tag faro-frontend:rollback-pre-docs faro-frontend:latest
$COMPOSE_PROD up -d --no-build --force-recreate frontend
docker ps --filter name=faro-frontend
```

Para revertir también el **código fuente**:

```bash
cd /opt/faro
mv frontend frontend.broken-$(date +%s)
tar -xzf backups/frontend-src-pre-docs-20260605-180810.tar.gz -C /opt/faro
# (luego rebuild si quieres reconstruir desde el source revertido)
```

Limpieza de tags de backup una vez confirmado que el deploy es estable (opcional):

```bash
docker rmi faro-frontend:backup-20260605-180810   # conserva rollback-pre-docs si prefieres
```

---

## Iteración 2 — pública + amigable con LLMs (2026-06-05, mismo día)

La página `/docs` se hizo **pública** y se añadieron endpoints en **texto/Markdown
renderizados en servidor** para que LLMs/crawlers lean la doc sin ejecutar el SPA
(`GET /docs` sin JS devuelve solo el shell vacío).

| Archivo | Tipo | Descripción |
| --- | --- | --- |
| `frontend/src/lib/sdk-docs-markdown.ts` | nuevo | Genera Markdown/llms.txt desde `sdk-docs.ts` (fuente única). |
| `frontend/src/routes/docs.md/+server.ts` | nuevo | `GET /docs.md` → referencia completa (text/markdown), pública. |
| `frontend/src/routes/llms.txt/+server.ts` | nuevo | `GET /llms.txt` → índice (convención llms.txt), público. |
| `frontend/src/routes/+layout.svelte` | editado | `/docs` exenta del redirect a `/login` (flag `isPublic`); el resto sigue tras auth. |
| `frontend/src/routes/docs/+page.svelte` | editado | `<svelte:head>` (meta/alternate) + enlaces visibles a `/docs.md` y `/llms.txt`. |

Endpoints públicos (verificados sin auth):

- `https://faro.iaportafolio.com/llms.txt` → 200 `text/plain`
- `https://faro.iaportafolio.com/docs.md` → 200 `text/markdown` (≈559 líneas, 7 SDKs, 145 métodos)
- `https://faro.iaportafolio.com/docs` → 200 (sin redirect a login)

> Los enlaces absolutos del Markdown usan `url.origin`, que en prod resuelve al dominio
> público gracias a `ORIGIN=https://faro.iaportafolio.com` en el compose del frontend.

**Nuevo punto de rollback:** la imagen docs-v1 (solo página, aún privada) quedó tagueada
como `faro-frontend:rollback-docs-v1`. Orden de rollback según cuán atrás quieras volver:

```bash
# volver a docs-v1 (página privada, sin endpoints públicos):
docker tag faro-frontend:rollback-docs-v1 faro-frontend:latest
# volver al estado original pre-/docs:
docker tag faro-frontend:rollback-pre-docs faro-frontend:latest
# …luego en cualquier caso:
docker compose -f docker-compose.prod.yml --env-file .env.prod up -d --no-build --force-recreate frontend
```

---

## Iteración 3 — fix del redirect a login en `/docs` (2026-06-05)

**Síntoma:** `/docs` seguía redirigiendo a `/login` para usuarios anónimos.
**Causa raíz:** el wrapper `api()` en `frontend/src/lib/api.ts` hacía
`window.location.assign('/login?…')` ante **cualquier 401**, dentro de `me()`
(que el layout llama al montar) — antes de que el `catch` del layout pudiera
tratar la ruta como pública.

| Archivo | Cambio |
| --- | --- |
| `frontend/src/lib/api.ts` | En 401, no redirige si la ruta es pública (`/login`, `/docs`). |
| `frontend/src/routes/+layout.svelte` | Visitante anónimo en `/docs` → shell público sin sidebar (barra mínima con login); con sesión, chrome normal. |

**Verificado con navegador headless real (anónimo, sin cookies):**

- `/docs` → se queda en `/docs`, h1 "SDKs & referencia de API", contenido visible, sin sidebar, sin password. ✅
- `/logs` → redirige a `/login?next=/logs` (el gate sigue intacto para el resto). ✅

Tags de rollback acumulados (de más nuevo a más viejo):
`rollback-docs-v2` (público con bug de login) · `rollback-docs-v1` (página privada) ·
`rollback-pre-docs` (estado original).
