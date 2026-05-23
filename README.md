# Faro

Centralised observability platform — logs, traces, metrics, error grouping, API uptime monitoring and threshold-based alerting — in a single self-hosted stack.

Inspired by Monoscope and similar projects, but built on a smaller, opinionated stack:

| Layer       | Tech                                  |
| ----------- | ------------------------------------- |
| Storage     | ClickHouse 24.x                       |
| Backend     | Rust (axum, tokio, reqwest)           |
| Ingestion   | OTLP/HTTP+JSON + native HTTP/JSON     |
| Frontend    | SvelteKit (Svelte 4) + vanilla CSS    |
| Queue/cache | Redis (placeholder for future use)    |
| Deploy      | Docker Compose                        |

## What you get

- **Logs** — high-cardinality structured logs with full-text search, severity / service filters, live tail (SSE), 30-day retention.
- **Distributed tracing** — OTLP span ingestion, trace list, span waterfall view, 14-day retention.
- **Metrics** — gauges, counters, sums, histograms and summaries via OTLP, with on-the-fly aggregations (avg/sum/min/max/count) and time bucketing, 90-day retention.
- **Error grouping** — automatic fingerprinting of WARN+/ERROR logs into Sentry-style issues with resolve/ignore workflow.
- **API monitors** — synthetic HTTP checks on configurable intervals with uptime% and latency stats.
- **Alert rules** — declarative ClickHouse queries with threshold + window, automatic firing/resolving incidents, webhook notifications (Slack/Discord/generic).
- **Dashboard** — totals, log volume sparkline, services overview.

## Quick start

```bash
cp .env.example .env             # tweak ports / token if you like
docker compose up -d --build
```

When everything is healthy:

| Service        | URL                       |
| -------------- | ------------------------- |
| Dashboard      | http://localhost:3000     |
| REST API       | http://localhost:8080     |
| OTLP/HTTP      | http://localhost:4318     |
| ClickHouse     | http://localhost:8123     |

ClickHouse initialises the `faro` database and all tables on first boot from `clickhouse/init/*.sql`.

## Sending data

### Native HTTP logs

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

### OpenTelemetry SDK (any language)

Point your OTLP exporter at `http://faro-backend:4318` with the `http/json` protocol:

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
export OTEL_SERVICE_NAME=billing
```

Logs go to `/v1/logs`, traces to `/v1/traces`, metrics to `/v1/metrics` — the standard OTLP/HTTP paths.

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

## Architecture

```
┌─────────────┐    ┌─────────────────────────────────────────┐
│ Your apps   │───▶│ Faro backend (Rust, axum)               │
│ OTel SDKs   │    │  ┌──────────┐  ┌──────────────────────┐ │
│ Collectors  │    │  │ ingest   │  │ batch writer workers │ │
│ HTTP client │    │  │ :4318    │─▶│ (per table)          │ │
└─────────────┘    │  │ :8080    │  └─────┬────────────────┘ │
                   │  └──────────┘        │                  │
                   │  ┌──────────┐        ▼                  │
                   │  │ query    │  ┌──────────────┐         │
                   │  │ API      │◀─│ ClickHouse   │◀────────┤
                   │  │ :8080    │  └──────────────┘         │
                   │  └──────────┘                           │
                   │  ┌──────────┐  ┌──────────────┐         │
                   │  │ monitors │  │ alert eval   │         │
                   │  │ runner   │  │ + dispatch   │         │
                   │  └──────────┘  └──────────────┘         │
                   └─────────────────────────────────────────┘
                                  ▲
                                  │
                          ┌───────┴────────┐
                          │ SvelteKit UI   │
                          │ :3000          │
                          └────────────────┘
```

- **Two HTTP listeners**: `:4318` only serves OTLP receivers; `:8080` serves the REST/SSE API and the optional native ingest endpoint. Keeping them separate means you can expose OTLP behind a different firewall rule from the dashboard API.
- **Batching**: ingestion handlers push rows onto bounded mpsc channels; per-table writer tasks flush every 750 ms (configurable) or 5 000 rows, whichever comes first.
- **Error indexer**: a background task subscribes to the in-memory log broadcast bus, picks WARN+/ERROR records (or anything with `exception.*` attributes) and writes them into `error_events` with a normalised SHA-256 fingerprint over `exception_type + normalised message + first 8 stack frames`.
- **Monitor runner**: reads `api_monitors` every 10 s, schedules each monitor on its own interval; results are sent through the same batching pipeline.
- **Alert evaluator**: reads `alert_rules` every 15 s, runs each rule's `query` (with `:window_seconds` substitution) on its `interval_seconds` cadence. State transitions are persisted to `alert_incidents` and webhook payloads are POSTed to each `notification_targets` URL.

## REST API surface

| Method | Path                                       | Notes |
| ------ | ------------------------------------------ | ----- |
| GET    | `/healthz`                                 |       |
| POST   | `/api/v1/ingest/logs`                      | Bearer token from `FARO_INGEST_TOKEN` |
| GET    | `/api/v1/logs`                             | filters: `service`, `min_severity`, `query`, `trace_id`, `last_minutes`, `limit` |
| GET    | `/api/v1/logs/live`                        | Server-Sent Events stream of new logs |
| GET    | `/api/v1/logs/stats`                       | aggregated counts per minute/service/severity |
| GET    | `/api/v1/traces`                           | trace list (re-aggregated from spans) |
| GET    | `/api/v1/traces/:trace_id`                 | full span list for a trace |
| GET    | `/api/v1/metrics/series?name=...`          | time-bucketed series with aggregation |
| GET    | `/api/v1/metrics/names`                    | list known metric names |
| GET    | `/api/v1/errors`                           | grouped error issues |
| GET    | `/api/v1/errors/:fingerprint`              | issue + recent events |
| POST   | `/api/v1/errors/:fingerprint/status`       | mark resolved/ignored/unresolved |
| GET    | `/api/v1/monitors`                         |       |
| POST   | `/api/v1/monitors`                         |       |
| PUT    | `/api/v1/monitors/:id`                     |       |
| DELETE | `/api/v1/monitors/:id`                     | soft-delete |
| GET    | `/api/v1/monitors/:id/results`             |       |
| GET    | `/api/v1/monitors/:id/uptime`              | uptime% + p95 latency over range |
| GET    | `/api/v1/alerts/rules`                     |       |
| POST   | `/api/v1/alerts/rules`                     |       |
| PUT    | `/api/v1/alerts/rules/:id`                 |       |
| DELETE | `/api/v1/alerts/rules/:id`                 |       |
| GET    | `/api/v1/alerts/incidents`                 |       |
| GET    | `/api/v1/services`                         |       |
| GET    | `/api/v1/dashboard`                        |       |

## Example alert rules

Error rate spike:

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

p95 latency from spans:

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

Monitor uptime below 99%:

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

## Repository layout

```
faro/
├── docker-compose.yml
├── .env.example
├── clickhouse/
│   ├── init/        # SQL run on first ClickHouse boot
│   └── config/      # users.d profile overrides (async inserts)
├── backend/         # Rust workspace
│   ├── Cargo.toml
│   ├── Dockerfile
│   └── src/
│       ├── main.rs           # bootstrap, two listeners
│       ├── config.rs / error.rs / state.rs
│       ├── storage/          # ClickHouse HTTP client + row types
│       ├── ingest/           # HTTP + OTLP receivers
│       ├── api/              # REST endpoints
│       ├── workers/          # batched writer, monitor runner, alert evaluator, error indexer
│       ├── fingerprint.rs    # error grouping
│       ├── notify.rs         # webhook dispatch
│       └── stream.rs         # SSE live tail
└── frontend/        # SvelteKit app
    ├── package.json
    └── src/
        ├── lib/api.ts        # typed REST client
        ├── lib/components/
        └── routes/           # dashboard, logs, traces, metrics, errors, monitors, alerts
```

## Configuration reference

| Env var                       | Default                       | Purpose |
| ----------------------------- | ----------------------------- | ------- |
| `FARO_API_ADDR`               | `0.0.0.0:8080`                | REST / SSE listener |
| `FARO_BIND_ADDR`              | `0.0.0.0:4318`                | OTLP listener |
| `CLICKHOUSE_URL`              | `http://clickhouse:8123`      | HTTP endpoint of ClickHouse |
| `CLICKHOUSE_DATABASE`         | `faro`                        |  |
| `CLICKHOUSE_USER`             | `faro`                        |  |
| `CLICKHOUSE_PASSWORD`         | `faro`                        |  |
| `FARO_INGEST_TOKEN`           | *(required)*                  | Bearer token for `/api/v1/ingest/logs` |
| `FARO_BATCH_MAX_ROWS`         | `5000`                        | Per-table flush threshold |
| `FARO_BATCH_FLUSH_MS`         | `750`                         | Per-table max linger |
| `RUST_LOG`                    | `info,faro=debug`             |  |
| `PUBLIC_API_BASE`             | `http://localhost:8080`       | Frontend → backend base URL (bake time) |

## Limitations / future work

- **No multi-tenant isolation** (intentional per spec) — anyone with the ingest token can write, anyone with API access can read everything.
- **OTLP/gRPC and OTLP/HTTP+protobuf** are not implemented; only OTLP/HTTP+JSON. Most SDKs support JSON via `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`.
- **Alert query DSL** is raw ClickHouse SQL — flexible but unsafe; treat as admin-only.
- **No durable ingest buffer** — if the backend crashes between channel send and ClickHouse flush, in-flight rows are lost. Redis (already in the compose file) is wired for a future Streams-backed buffer.
- **No authentication on the dashboard API.** Front it with an OAuth proxy / reverse proxy if you expose it publicly.

## Licence

MIT.
