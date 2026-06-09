# Faro

Self-hosted observability platform — logs, traces, metrics, error grouping, HTTP uptime monitoring, and threshold-based alerts — all in a single stack.

Inspired by Monoscope and similar projects, but built on a smaller, opinionated stack:

| Layer        | Technology                            |
| ------------ | ------------------------------------- |
| Storage      | ClickHouse 24.x                       |
| Backend      | Rust (axum, tokio, reqwest)           |
| Ingest       | OTLP/HTTP+JSON + native HTTP/JSON     |
| Frontend     | SvelteKit (Svelte 5) + plain CSS      |
| Queue/cache  | Redis (reserved for future use)       |
| Deployment   | Docker Compose                        |

> Documentación principal en español: [README.md](README.md). This file is a
> condensed English version aimed at users discovering Faro through the
> published SDKs.

## What it includes

- **Logs** — high-cardinality structured logs with full-text search,
  severity/service filters, live tail (SSE) and 30-day retention.
- **Distributed tracing** — OTLP span ingestion, trace listing,
  waterfall span view, 14-day retention.
- **Metrics** — gauges, counters, sums, histograms, summaries via OTLP
  with on-the-fly aggregations (avg/sum/min/max/count) and time
  bucketing. 90-day retention.
- **Error grouping** — automatic fingerprinting of WARN+/ERROR logs
  into Sentry-style issues with resolve/ignore workflow.
- **API monitors** — synthetic HTTP checks at configurable intervals
  with uptime% and latency stats.
- **Alert rules** — declarative ClickHouse queries with threshold +
  window, auto fire/resolve of incidents, and webhook notifications
  (Slack/Discord/generic).
- **Dashboard** — totals, log volume sparkline, services overview.

## Quick start

```bash
cp .env.example .env             # tweak ports / token if you want
docker compose up -d --build
```

When everything is healthy:

| Service        | URL                       |
| -------------- | ------------------------- |
| Dashboard      | <http://localhost:3000>     |
| REST API       | <http://localhost:8080>     |
| OTLP/HTTP      | <http://localhost:4318>     |
| ClickHouse     | <http://localhost:8123>     |

ClickHouse initializes the `faro` database and all tables on first boot
from `clickhouse/init/*.sql`.

## Sending data

### Native HTTP logs

```bash
curl -X POST http://localhost:8080/api/v1/ingest/logs \
  -H "Authorization: Bearer dev-ingest-token" \
  -H "Content-Type: application/json" \
  -d '{
    "service": "billing",
    "logs": [
      { "level": "INFO", "message": "charge succeeded",
        "attributes": { "customer_id": "cus_42" } }
    ]
  }'
```

### OpenTelemetry SDK (any language)

Point your OTLP exporter to `http://faro-backend:4318` with
`http/json` encoding:

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
export OTEL_SERVICE_NAME=billing
```

Logs go to `/v1/logs`, traces to `/v1/traces`, metrics to `/v1/metrics`
— the standard OTLP/HTTP paths.

### Faro SDKs

We publish first-party SDKs for the common runtimes:

| Language    | Package                                        |
| ----------- | ---------------------------------------------- |
| Node.js     | [`@iaportafolio/node`](sdks/node)              |
| Next.js     | [`@iaportafolio/nextjs`](sdks/nextjs)          |
| Expo / RN   | [`@iaportafolio/expo`](sdks/expo)              |
| Python      | [`faro-sdk`](sdks/python)                      |
| Go          | [`github.com/IA-Portafolio/faro/sdks/go`](sdks/go) |
| Flutter     | [`faro_sdk`](sdks/flutter)                     |
| Kotlin / Android | [`com.iaportafolio:faro`](sdks/kotlin)    |

Each SDK has its own README in `sdks/<lang>/`.

## Architecture

![Faro architecture diagram](docs/architecture.png)

- **Two HTTP listeners**: `:4318` only serves OTLP receivers; `:8080`
  serves the REST/SSE API and the optional native ingest endpoint.
  Keeping them separate lets you expose OTLP behind different firewall
  rules than the dashboard.
- **Batching**: ingest handlers push rows to bounded mpsc channels;
  per-table writer tasks flush every 750 ms (configurable) or 5,000
  rows, whichever comes first.
- **Error indexer**: a background task subscribes to the in-memory log
  broadcast bus, picks WARN+/ERROR entries (or anything with
  `exception.*` attributes) and writes them to `error_events` with a
  SHA-256 fingerprint over `exception_type + normalized message + first 8
  stack frames`.
- **Monitor runner**: reads `api_monitors` every 10 s and schedules each
  monitor on its own interval; results flow through the same batching
  pipeline.
- **Alert evaluator**: reads `alert_rules` every 15 s and runs each
  rule's `query` (with `:window_seconds` substitution) at the cadence
  of its `interval_seconds`. State transitions are persisted to
  `alert_incidents` and webhook payloads are POSTed to each URL in
  `notification_targets`.

## Limitations

- **No multi-tenant isolation** (by design) — anyone with the ingest
  token can write, anyone with API access can read everything.
- **OTLP ingest** supports HTTP/JSON (`:4318`) and gRPC/protobuf
  (`:4317`). See
  [ADR-0004](docs/adr/0004-otlp-http-json-ingest.md) and
  [ADR-0010](docs/adr/0010-otlp-grpc-ingest.md).
- **Alert query DSL** is raw ClickHouse SQL — flexible but unsafe;
  treat it as admin-only.
- **No durable ingest buffer** — if the backend dies between channel
  push and ClickHouse flush, in-flight rows are lost. Redis is wired
  into the compose for a future Streams-backed buffer.
- **Dashboard auth** is native: Argon2id password hashing, cookie-based
  sessions and optional TOTP 2FA, bootstrapped from `FARO_BOOTSTRAP_ADMIN_*`.
  A reverse / auth proxy is still recommended as defense-in-depth when
  exposed publicly. See
  [ADR-0009](docs/adr/0009-security-hardening.md) (supersedes the original
  "no native auth" decision in
  [ADR-0005](docs/adr/0005-no-native-auth.md)).

## License

Proprietary. All rights reserved. Use, copy, modification or
distribution of the source code requires prior written authorization
from the owner. See [LICENSE](LICENSE).
