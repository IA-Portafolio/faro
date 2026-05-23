# Faro

Plataforma centralizada de observabilidad — logs, trazas, métricas, agrupación de errores, monitoreo de disponibilidad de APIs y alertas basadas en umbrales — todo en un único stack auto-hospedado.

Inspirada en Monoscope y proyectos similares, pero construida sobre un stack más pequeño y opinado:

| Capa         | Tecnología                            |
| ------------ | ------------------------------------- |
| Almacenamiento | ClickHouse 24.x                     |
| Backend      | Rust (axum, tokio, reqwest)           |
| Ingesta      | OTLP/HTTP+JSON + HTTP/JSON nativo     |
| Frontend     | SvelteKit (Svelte 4) + CSS plano      |
| Cola/caché   | Redis (reservado para uso futuro)     |
| Despliegue   | Docker Compose                        |

## Qué incluye

- **Logs** — logs estructurados de alta cardinalidad con búsqueda de texto completo, filtros por severidad y servicio, live tail (SSE) y retención de 30 días.
- **Tracing distribuido** — ingesta de spans OTLP, listado de trazas, vista en cascada de spans y retención de 14 días.
- **Métricas** — gauges, counters, sums, histogramas y summaries vía OTLP, con agregaciones al vuelo (avg/sum/min/max/count) y bucketing por tiempo, retención de 90 días.
- **Agrupación de errores** — huella digital automática de logs WARN+/ERROR en issues estilo Sentry con flujo de resolver/ignorar.
- **Monitores de API** — chequeos HTTP sintéticos en intervalos configurables con estadísticas de uptime% y latencia.
- **Reglas de alerta** — queries declarativas de ClickHouse con umbral + ventana, disparo/resolución automática de incidentes, notificaciones por webhook (Slack/Discord/genérico).
- **Dashboard** — totales, sparkline del volumen de logs, vista general de servicios.

## Arranque rápido

```bash
cp .env.example .env             # ajusta puertos / token si lo deseas
docker compose up -d --build
```

Cuando todo esté saludable:

| Servicio       | URL                       |
| -------------- | ------------------------- |
| Dashboard      | http://localhost:3000     |
| API REST       | http://localhost:8080     |
| OTLP/HTTP      | http://localhost:4318     |
| ClickHouse     | http://localhost:8123     |

ClickHouse inicializa la base de datos `faro` y todas las tablas en el primer arranque desde `clickhouse/init/*.sql`.

## Enviando datos

### Logs HTTP nativos

```bash
curl -X POST http://localhost:8080/api/v1/ingest/logs \
  -H "Authorization: Bearer dev-ingest-token" \
  -H "Content-Type: application/json" \
  -d '{
    "service": "billing",
    "logs": [
      {
        "level": "INFO",
        "message": "charge succeeded",
        "attributes": { "customer_id": "cus_42", "amount": "19.99" }
      },
      {
        "level": "ERROR",
        "message": "payment provider 502",
        "attributes": {
          "exception.type": "UpstreamError",
          "exception.message": "bad gateway",
          "exception.stacktrace": "at provider.charge (provider.rs:42)\nat handler.bill (handler.rs:88)"
        }
      }
    ]
  }'
```

### SDK de OpenTelemetry (cualquier lenguaje)

Apunta tu exportador OTLP a `http://faro-backend:4318` con el protocolo `http/json`:

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
export OTEL_SERVICE_NAME=billing
```

Los logs van a `/v1/logs`, las trazas a `/v1/traces`, las métricas a `/v1/metrics` — las rutas estándar de OTLP/HTTP.

### OpenTelemetry Collector

```yaml
exporters:
  otlphttp/faro:
    endpoint: http://faro-backend:4318
    encoding: json
service:
  pipelines:
    logs:    { exporters: [otlphttp/faro] }
    traces:  { exporters: [otlphttp/faro] }
    metrics: { exporters: [otlphttp/faro] }
```

## Arquitectura

```
┌─────────────┐    ┌─────────────────────────────────────────┐
│ Tus apps    │───▶│ Backend Faro (Rust, axum)               │
│ SDKs OTel   │    │  ┌──────────┐  ┌──────────────────────┐ │
│ Collectors  │    │  │ ingesta  │  │ workers de escritura │ │
│ Cliente HTTP│    │  │ :4318    │─▶│ por lotes (por tabla)│ │
└─────────────┘    │  │ :8080    │  └─────┬────────────────┘ │
                   │  └──────────┘        │                  │
                   │  ┌──────────┐        ▼                  │
                   │  │ API de   │  ┌──────────────┐         │
                   │  │ consulta │◀─│ ClickHouse   │◀────────┤
                   │  │ :8080    │  └──────────────┘         │
                   │  └──────────┘                           │
                   │  ┌──────────┐  ┌──────────────┐         │
                   │  │ runner   │  │ evaluador de │         │
                   │  │ monitores│  │ alertas+envío│         │
                   │  └──────────┘  └──────────────┘         │
                   └─────────────────────────────────────────┘
                                  ▲
                                  │
                          ┌───────┴────────┐
                          │ UI SvelteKit   │
                          │ :3000          │
                          └────────────────┘
```

- **Dos listeners HTTP**: `:4318` solo sirve receptores OTLP; `:8080` sirve la API REST/SSE y el endpoint opcional de ingesta nativa. Mantenerlos separados permite exponer OTLP detrás de una regla de firewall distinta a la del dashboard.
- **Batching**: los handlers de ingesta empujan filas a canales mpsc acotados; las tareas de escritura por tabla hacen flush cada 750 ms (configurable) o 5 000 filas, lo que ocurra primero.
- **Indexador de errores**: una tarea en segundo plano se suscribe al bus de broadcast de logs en memoria, toma los registros WARN+/ERROR (o cualquier cosa con atributos `exception.*`) y los escribe en `error_events` con una huella SHA-256 normalizada sobre `exception_type + mensaje normalizado + primeros 8 frames del stack`.
- **Runner de monitores**: lee `api_monitors` cada 10 s y agenda cada monitor en su propio intervalo; los resultados se envían por la misma pipeline de batching.
- **Evaluador de alertas**: lee `alert_rules` cada 15 s y ejecuta el `query` de cada regla (con sustitución de `:window_seconds`) en la cadencia de su `interval_seconds`. Las transiciones de estado se persisten en `alert_incidents` y los payloads de webhook se envían por POST a cada URL en `notification_targets`.

## Superficie de la API REST

| Método | Ruta                                       | Notas |
| ------ | ------------------------------------------ | ----- |
| GET    | `/healthz`                                 |       |
| POST   | `/api/v1/ingest/logs`                      | Token Bearer desde `FARO_INGEST_TOKEN` |
| GET    | `/api/v1/logs`                             | filtros: `service`, `min_severity`, `query`, `trace_id`, `last_minutes`, `limit` |
| GET    | `/api/v1/logs/live`                        | Stream de Server-Sent Events de nuevos logs |
| GET    | `/api/v1/logs/stats`                       | conteos agregados por minuto/servicio/severidad |
| GET    | `/api/v1/traces`                           | listado de trazas (reagregado desde spans) |
| GET    | `/api/v1/traces/:trace_id`                 | listado completo de spans para una traza |
| GET    | `/api/v1/metrics/series?name=...`          | series con bucketing por tiempo y agregación |
| GET    | `/api/v1/metrics/names`                    | listado de nombres de métricas conocidas |
| GET    | `/api/v1/errors`                           | issues de errores agrupados |
| GET    | `/api/v1/errors/:fingerprint`              | issue + eventos recientes |
| POST   | `/api/v1/errors/:fingerprint/status`       | marcar como resuelto/ignorado/no resuelto |
| GET    | `/api/v1/monitors`                         |       |
| POST   | `/api/v1/monitors`                         |       |
| PUT    | `/api/v1/monitors/:id`                     |       |
| DELETE | `/api/v1/monitors/:id`                     | borrado lógico |
| GET    | `/api/v1/monitors/:id/results`             |       |
| GET    | `/api/v1/monitors/:id/uptime`              | uptime% + latencia p95 sobre un rango |
| GET    | `/api/v1/alerts/rules`                     |       |
| POST   | `/api/v1/alerts/rules`                     |       |
| PUT    | `/api/v1/alerts/rules/:id`                 |       |
| DELETE | `/api/v1/alerts/rules/:id`                 |       |
| GET    | `/api/v1/alerts/incidents`                 |       |
| GET    | `/api/v1/services`                         |       |
| GET    | `/api/v1/dashboard`                        |       |

## Reglas de alerta de ejemplo

Pico de tasa de errores:

```json
{
  "name": "High error rate",
  "source": "logs",
  "query": "(SELECT countIf(severity_number >= 17) FROM faro.logs WHERE timestamp > now() - INTERVAL :window_seconds SECOND)",
  "condition": "gt",
  "threshold": 25,
  "window_seconds": 300,
  "interval_seconds": 60,
  "severity": "error",
  "notification_targets": ["https://discord.com/api/webhooks/..."]
}
```

Latencia p95 desde spans:

```json
{
  "name": "API p95 > 800ms",
  "source": "spans",
  "query": "(SELECT toFloat64(quantile(0.95)(duration_ns))/1e6 FROM faro.spans WHERE service_name='api' AND timestamp > now() - INTERVAL :window_seconds SECOND)",
  "condition": "gt",
  "threshold": 800,
  "window_seconds": 300,
  "interval_seconds": 60,
  "severity": "warn"
}
```

Uptime de monitor por debajo del 99%:

```json
{
  "name": "Checkout endpoint down",
  "source": "monitors",
  "query": "(SELECT sum(success)/count()*100 FROM faro.monitor_results WHERE monitor_id = 'YOUR-UUID' AND timestamp > now() - INTERVAL :window_seconds SECOND)",
  "condition": "lt",
  "threshold": 99,
  "window_seconds": 300,
  "interval_seconds": 60,
  "severity": "critical"
}
```

## Estructura del repositorio

```
faro/
├── docker-compose.yml
├── .env.example
├── clickhouse/
│   ├── init/        # SQL ejecutado en el primer arranque de ClickHouse
│   └── config/      # overrides de perfil en users.d (async inserts)
├── backend/         # workspace de Rust
│   ├── Cargo.toml
│   ├── Dockerfile
│   └── src/
│       ├── main.rs           # bootstrap, dos listeners
│       ├── config.rs / error.rs / state.rs
│       ├── storage/          # cliente HTTP de ClickHouse + tipos de fila
│       ├── ingest/           # receptores HTTP + OTLP
│       ├── api/              # endpoints REST
│       ├── workers/          # writer por lotes, runner de monitores, evaluador de alertas, indexador de errores
│       ├── fingerprint.rs    # agrupación de errores
│       ├── notify.rs         # despacho de webhooks
│       └── stream.rs         # live tail por SSE
└── frontend/        # app SvelteKit
    ├── package.json
    └── src/
        ├── lib/api.ts        # cliente REST tipado
        ├── lib/components/
        └── routes/           # dashboard, logs, traces, metrics, errors, monitors, alerts
```

## Referencia de configuración

| Variable de entorno           | Valor por defecto             | Propósito |
| ----------------------------- | ----------------------------- | --------- |
| `FARO_API_ADDR`               | `0.0.0.0:8080`                | Listener REST / SSE |
| `FARO_BIND_ADDR`              | `0.0.0.0:4318`                | Listener OTLP |
| `CLICKHOUSE_URL`              | `http://clickhouse:8123`      | Endpoint HTTP de ClickHouse |
| `CLICKHOUSE_DATABASE`         | `faro`                        |  |
| `CLICKHOUSE_USER`             | `faro`                        |  |
| `CLICKHOUSE_PASSWORD`         | `faro`                        |  |
| `FARO_INGEST_TOKEN`           | *(requerido)*                 | Token Bearer para `/api/v1/ingest/logs` |
| `FARO_BATCH_MAX_ROWS`         | `5000`                        | Umbral de flush por tabla |
| `FARO_BATCH_FLUSH_MS`         | `750`                         | Tiempo máximo de espera por tabla |
| `RUST_LOG`                    | `info,faro=debug`             |  |
| `PUBLIC_API_BASE`             | `http://localhost:8080`       | URL base frontend → backend (en tiempo de build) |

## Limitaciones / trabajo futuro

- **Sin aislamiento multi-tenant** (intencional por diseño) — cualquiera con el token de ingesta puede escribir, cualquiera con acceso a la API puede leer todo.
- **OTLP/gRPC y OTLP/HTTP+protobuf** no están implementados; solo OTLP/HTTP+JSON. La mayoría de los SDKs soportan JSON vía `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`.
- **DSL de queries de alertas** es SQL crudo de ClickHouse — flexible pero inseguro; trátalo como solo-admin.
- **Sin buffer de ingesta durable** — si el backend cae entre el envío al canal y el flush a ClickHouse, las filas en vuelo se pierden. Redis (ya presente en el compose) está cableado para un futuro buffer respaldado por Streams.
- **Sin autenticación en la API del dashboard.** Ponlo detrás de un proxy OAuth / proxy inverso si lo expones públicamente.

## Licencia

MIT.
