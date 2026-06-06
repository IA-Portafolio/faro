# Mantener la documentación de los SDKs sincronizada

> **Regla de oro:** cada vez que cambie la **API pública** de un SDK (`sdks/<lang>`),
> ese mismo commit/PR debe actualizar el catálogo de documentación en
> [`frontend/src/lib/sdk-docs.ts`](../frontend/src/lib/sdk-docs.ts).
> Un cambio de SDK sin su actualización de doc se considera **incompleto**.

## Por qué

La documentación **no se auto-genera** del código de los SDKs. El archivo
`frontend/src/lib/sdk-docs.ts` es un **espejo mantenido a mano** de la API real,
y es la **fuente única** que alimenta tres salidas:

| Salida | URL | Para quién |
| --- | --- | --- |
| Página navegable (con buscador) | `/docs` | Humanos |
| Markdown completo | `/docs.md` | **LLMs / agentes** |
| Índice estilo llms.txt | `/llms.txt` | **LLMs / agentes** |

Si el código del SDK avanza y `sdk-docs.ts` no, la doc **miente**: los humanos
copian firmas que ya no existen y los LLMs implementan integraciones rotas. Por
eso la sincronización es obligatoria y va en el mismo cambio, no "después".

## Qué cuenta como cambio de API pública (disparadores)

Si tu cambio en `sdks/<lang>` hace **cualquiera** de estas cosas, actualiza la doc:

- Añadir, renombrar o eliminar un método público.
- Cambiar la **firma** de un método (parámetros, opcionalidad, tipo de retorno).
- Añadir/cambiar una **opción de `init()`** o su default.
- Añadir una **capacidad** nueva (logs, tracing, métricas, feature flags, RUM, replay…).
- Cambiar la **disponibilidad** de `track` / `identify` / `page` / `screen` / `alias` en un SDK.
- Cambiar los **defaults de perfil** (flush / batch / cola) de un SDK.
- Añadir un **SDK nuevo**.

Cambios internos que **no** tocan la superficie pública (refactor, perf, fix sin
cambio de firma) no requieren tocar la doc.

## Dónde editar

Edita **solo** la fuente de datos. Lo demás se genera a partir de ella:

| Archivo | ¿Editar a mano? | Qué es |
| --- | --- | --- |
| [`frontend/src/lib/sdk-docs.ts`](../frontend/src/lib/sdk-docs.ts) | **SÍ** | Fuente única: `sdks[]`, `commonOptions`, `severities`, `productMatrix`, `profileDefaults`. |
| `frontend/src/lib/sdk-docs-markdown.ts` | No (salvo formato) | Serializa la fuente a Markdown para `/docs.md` y `/llms.txt`. |
| `frontend/src/routes/docs/+page.svelte` | No | Renderiza la fuente en la UI `/docs`. |
| `frontend/src/routes/docs.md/+server.ts`, `…/llms.txt/+server.ts` | No | Endpoints públicos. |

### Estructura de `sdk-docs.ts`

Cada SDK es un objeto en el array `sdks`:

```ts
{
  id, name, language, pkg, install, profile,   // metadatos
  blurb, capabilities, lang, initExample,        // cabecera + snippet
  groups: [                                      // métodos agrupados
    { title: 'Logging', note?, methods: [
      { signature: 'info(msg, attrs?)', summary: '…', returns?: '…' },
    ]},
  ],
}
```

## Checklist por tipo de cambio

| Hiciste esto en el SDK… | …toca esto en `sdk-docs.ts` |
| --- | --- |
| Método nuevo | Añadir `{ signature, summary, returns? }` al `group` adecuado del SDK. |
| Método renombrado | Actualizar `signature` (y `summary` si cambió el comportamiento). |
| Método eliminado | Borrar su entrada del `group`. |
| Firma cambiada | Actualizar `signature` y/o `returns`. |
| Opción de `init()` nueva/cambiada | Editar `commonOptions` (si es común) o el `initExample`/`blurb` del SDK. |
| Capacidad nueva | Añadir el string a `capabilities` del SDK. |
| Cambio en track/identify/page/screen/alias | Actualizar la fila del SDK en `productMatrix`. |
| Defaults de perfil cambiados | `profileDefaults` (si cambia el baseline) o el `profile` del SDK. |
| SDK nuevo | Añadir un objeto a `sdks[]` **y** una fila a `productMatrix`. |

> Las firmas se documentan tal como las expone el lenguaje (camelCase en
> TS/Kotlin/Go, snake_case en Python). Extrae la firma del **código real**, no
> del README — los métodos en `sdk-docs.ts` se sacaron leyendo `sdks/<lang>`.

## Verificar antes de hacer commit

```bash
cd frontend
npm run check     # svelte-check + tsc → 0 errores
npm run build     # build de producción completo
```

Comprueba que tu método aparece en el Markdown público (arranca el server
compilado y míralo, o búscalo en el build):

```bash
PORT=3099 ORIGIN=http://localhost:3099 node build &   # tras `npm run build`
curl -s http://localhost:3099/docs.md | grep -i "nombreDeTuMetodo"
```

## Desplegar el cambio

La doc viaja en el SDK frontend. Para publicarla en producción se reconstruye y
recrea **solo** el servicio `frontend` — ver el runbook completo (con backup y
rollback) en [`DEPLOY-DOCS-PAGE.md`](../DEPLOY-DOCS-PAGE.md):

```bash
cd /opt/faro
docker compose -f docker-compose.prod.yml --env-file .env.prod build frontend
docker compose -f docker-compose.prod.yml --env-file .env.prod up -d frontend
```

No hace falta tocar el backend, ClickHouse ni Redis.

---

_TL;DR: tocas un método público de un SDK → actualizas `frontend/src/lib/sdk-docs.ts`
en el mismo cambio → `npm run check && npm run build` → deploy del frontend._
