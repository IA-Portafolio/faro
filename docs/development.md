# Development

Guía para trabajar en Faro localmente. Para producción ver
[deployment.md](deployment.md); para CI/CD ver [`infra/README.md`](../infra/README.md).

## Prerequisitos

| Herramienta     | Versión    | Para |
| --------------- | ---------- | ---- |
| Docker          | 24+        | Compose, ClickHouse, Redis |
| Docker Compose  | v2         | orquestación |
| Rust            | stable     | backend (pineado en `rust-toolchain.toml`) |
| Node            | 20         | frontend (pineado en `.nvmrc`) |

Opcionales por SDK: Python 3.11, Go 1.22, Flutter stable, JDK 17.

## Setup inicial

```bash
git clone https://github.com/IA-Portafolio/faro
cd faro
cp .env.example .env
docker compose up -d --build
```

Espera ~30s al primer arranque (ClickHouse aplica `clickhouse/init/*.sql`).
Verifica:

```bash
curl http://localhost:8080/healthz       # backend
open  http://localhost:3000              # dashboard
```

## Desarrollo iterativo

### Backend (Rust)

Para trabajar en el backend con recompilación rápida, levanta solo las
dependencias y corre el binario en host:

```bash
docker compose up -d clickhouse redis
cd backend
cargo run                                # usa CLICKHOUSE_URL=http://localhost:8123
```

Variables que conviene tener exportadas (o usa `.env` con `direnv` /
`dotenvx`):

```bash
export CLICKHOUSE_URL=http://localhost:8123
export CLICKHOUSE_USER=faro
export CLICKHOUSE_PASSWORD=faro
# El backend crea el proyecto seed con este token en el primer arranque (BD vacía).
# Es el token que el backend acepta en ingesta:
export FARO_BOOTSTRAP_INGEST_TOKEN=dev-ingest-token
# El que envían los SDKs (Node/Python lo leen). Debe coincidir con el de arriba:
export FARO_INGEST_TOKEN=dev-ingest-token
export RUST_LOG=info,faro=debug
```

Checks antes de commitear:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo nextest run     # o `cargo test` si no tienes nextest instalado
```

CI corre fmt + clippy + `cargo nextest run --profile ci`.

### Tests en paralelo (`cargo nextest`)

`cargo test` corre los binarios de tests **uno a uno**: con 11 archivos en
`backend/tests/*.rs`, eso es ~5–10× más lento de lo necesario porque cada
binario libera sus cores antes que arranque el siguiente. `cargo-nextest`
mete todos los tests en un único pool y los reparte entre los cores
disponibles cross-binary.

La fixture `tests/common/mod.rs` ya genera un `project_id` UUID y un
`project_token` UUID por test, y bindea los listeners HTTP a `127.0.0.1:0`
(puerto efímero). Eso significa que **N tests pueden correr concurrentes
contra el mismo ClickHouse compartido** sin contaminarse: cada uno escribe
y consulta solo filas de su propio `project_id`.

Instalar (una sola vez):

```bash
cargo install cargo-nextest --locked
```

Correr toda la suite:

```bash
cd backend
cargo nextest run               # perfil default
cargo nextest run --profile ci  # 2 retries por test, output compacto + junit.xml
```

Subsets concretos:

```bash
cargo nextest run --test workers_alert_evaluator
cargo nextest run -E 'test(/^stream_sse/)'      # filtro por expresión
cargo nextest run -E 'binary(workers_monitor_runner)'
```

Configuración en `backend/.config/nextest.toml` (test-threads, retries,
slow-timeout). Si en el futuro un test necesita serialización (estado
realmente global), márcalo con `test-groups` en ese archivo en vez de
caer a `--test-threads=1` global (penaliza a todo el resto).

> nextest no corre doctests (limitación conocida del runner). Hoy el
> backend no tiene doctests, así que no perdemos nada; si se agregan,
> añade un step extra `cargo test --doc` en CI.

### Frontend (SvelteKit)

```bash
cd frontend
npm install
PUBLIC_API_BASE=http://localhost:8080 npm run dev
```

Abre <http://localhost:5173> (puerto por defecto de Vite, distinto del
contenedor de Docker que usa 3000).

Checks:

```bash
npm run check                            # svelte-check + tsc
npm run build
```

### ClickHouse

Cliente interactivo:

```bash
docker exec -it faro-clickhouse clickhouse-client --user=faro --password=faro --database=faro
```

Útil para inspeccionar:

```sql
SELECT count(), service_name FROM logs GROUP BY service_name;
SHOW CREATE TABLE error_events;
SELECT * FROM alert_incidents ORDER BY started_at DESC LIMIT 10;
```

### Migraciones

Los archivos en `clickhouse/init/` solo corren la primera vez (volumen
vacío). Para cambios posteriores, escribe migraciones idempotentes
(`CREATE TABLE IF NOT EXISTS`, `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`)
en `clickhouse/migrations/` — el workflow `deploy.yml` las aplica en cada
push y son seguras de re-ejecutar.

Aplicar localmente:

```bash
for m in clickhouse/migrations/*.sql; do
  docker exec -i faro-clickhouse clickhouse-client --user=faro --password=faro --database=faro --multiquery < "$m"
done
```

Verificar que `init/` + `migrations/` se aplican limpios y son idempotentes
(corre `init/*.sql`, luego `migrations/*.sql` dos veces, luego `SHOW TABLES`
contra un catálogo cerrado):

```bash
bash clickhouse/test-migrations.sh        # requiere CH local o el container faro-clickhouse arriba
```

El mismo script corre en CI bajo el job `migrations` de `.github/workflows/ci.yml`
contra una instancia fresca de ClickHouse. El check del catálogo es
**bidireccional**: falla si falta una tabla esperada y también si aparece una
tabla que no está registrada. Si agregás una tabla nueva en `init/` o
`migrations/`, sumala al array `EXPECTED` del script (en orden alfabético) —
sino el test falla con "tablas no registradas en el catálogo".

## Variables de entorno y dónde viven

[`.env.example`](../.env.example) es la **fuente única de verdad** para
todas las variables que entienden el backend, el compose, el frontend y
los scripts (~45 vars en 15 secciones). La página
[`docs/reference/environment.md`](reference/environment.md) — única
tabla de env-vars en todo el repo — se autogenera desde ahí. README,
[`deployment.md`](deployment.md), [`infra/README.md`](../infra/README.md)
y [`.env.prod.template`](../.env.prod.template) linkean a la página
generada en vez de mantener tablas paralelas.

Cuando agregás, renombrás o cambiás el default de una variable:

```bash
# 1. Edita .env.example (formato: header `# ---- Sección ----`,
#    comentario descriptivo encima del var, línea en blanco entre vars).
# 2. Regenera la página de reference.
bash scripts/gen-env-reference.sh
git add .env.example docs/reference/environment.md
```

El job `env-reference` en [`docs.yml`](../.github/workflows/docs.yml)
(via [`scripts/check-env-reference.sh`](../scripts/check-env-reference.sh))
falla el PR con el diff completo si los dos archivos se desincronizan.

**Por qué este diseño**: antes había tablas paralelas en README,
deployment.md, .env.example, .env.prod.template e infra/README.md. Cada
variable nueva quedaba en una sola y se desincronizaba; los devs
encontraban defaults distintos según qué documento leyeran. La solución
es tener una autoridad (`.env.example`, formato planchable por humanos),
una salida generada (`reference/environment.md`, formato lindo para
leer), y un check de CI que garantiza que las dos son la misma cosa.

## Enviar tráfico de prueba

Generar logs continuos:

```bash
while true; do
  curl -s -X POST http://localhost:8080/api/v1/ingest/logs \
    -H "Authorization: Bearer dev-ingest-token" \
    -H "Content-Type: application/json" \
    -d "{\"service\":\"demo\",\"logs\":[{\"level\":\"INFO\",\"message\":\"tick $(date +%s)\"}]}"
  sleep 1
done
```

Generar trazas vía OTLP (con un SDK de OpenTelemetry apuntando a
`http://localhost:4318`, encoding `http/json`).

## SDKs

Cada SDK vive en `sdks/<lang>/` con su propio README. Para desarrollarlos
localmente, ver el README del SDK correspondiente. Para publicar:

```bash
git tag sdk-<lang>-v<semver>
git push origin sdk-<lang>-v<semver>
```

Ver [`infra/README.md`](../infra/README.md) sección 2 para la matriz de
registries y secrets necesarios.

## Reset completo

Cuando quieras empezar de cero (borra todos los datos):

```bash
docker compose down -v
docker compose up -d --build
```

## Estructura del proyecto

Ver README principal → "Estructura del repositorio".
