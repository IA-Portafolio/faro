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
export FARO_INGEST_TOKEN=dev-ingest-token
export RUST_LOG=info,faro=debug
```

Checks antes de commitear:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

CI corre exactamente estos tres comandos.

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
