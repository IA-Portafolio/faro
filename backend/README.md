# Faro · Backend (`faro`)

Servidor de **Faro** en **Rust (Axum + Tokio)** con **ClickHouse** como
almacenamiento. Recibe telemetría de los SDKs, la persiste y la expone al
dashboard por una API REST.

> Este README es el **mapa + glosario** del backend. Además, cada módulo de
> `src/` lleva una cabecera `//!` que explica qué hace; este documento da la vista
> de pájaro de cómo encajan.

## Las dos superficies (puertos)

| Puerto | Qué sirve |
|--------|-----------|
| `8080` | **API REST** del dashboard (`/api/v1/*`), SSE en vivo, `/healthz` y la doc OpenAPI/Scalar. |
| `4318` | Ingesta **OTLP/HTTP+JSON** (`/v1/logs`, `/v1/traces`, `/v1/metrics`) + endpoints nativos. |
| `4317` | Ingesta **OTLP/gRPC** (lo que usan por defecto los SDKs oficiales de OpenTelemetry). |

## Cómo se ejecuta (tests/lint)

```bash
cargo run                                   # arranca el servidor (necesita ClickHouse + config)
cargo test --lib                            # tests unitarios (sin ClickHouse)
cargo fmt --all -- --check                  # formato
cargo clippy --all-targets -- -D warnings   # lint (cero warnings)

# Integration tests (necesitan ClickHouse): stack efímero en Docker
docker compose -f docker-compose.test.yml -p faro-test up --build \
  --abort-on-container-exit --exit-code-from backend-test
```

> Gate de CI / `AGENTS.md`: tocar `src/**` obliga a `cargo test` + `fmt` +
> `clippy`. Si tocás handlers/queries/schema, además los integration tests contra
> ClickHouse.

---

## Mapa de módulos (`src/`)

### Arranque y plumbing
| Módulo | Qué es |
|--------|--------|
| `main.rs` | Binario: arranca logging, config, ClickHouse, workers y los 3 listeners; shutdown ordenado. |
| `lib.rs` | Raíz de la crate `faro`: declara y reexporta los módulos. |
| `config.rs` | `Config` leída del entorno (`FARO_*`, `CLICKHOUSE_*`): direcciones, pooling, topes SSE, bootstrap. |
| `state.rs` | Estado compartido (`AppState`/`SharedState`): cliente CH, caches, canales de ingesta, bus SSE, rate limiters. |
| `error.rs` | `ApiError` / `ApiResult<T>`: modos de fallo y su mapeo a HTTP + JSON. |
| `openapi.rs` | Spec OpenAPI (utoipa) que alimenta la referencia Scalar en `/api/v1/docs`. |
| `observability.rs`, `telemetry.rs` | Auto-observabilidad de Faro (nombres de métricas propias, exportación). |
| `versions.rs` | Compatibilidad de versión SDK ↔ backend (header de versión). |

### API REST — `api/`
`api/mod.rs` arma el router raíz y monta un sub-router por recurso. Cada archivo
expone los endpoints de un recurso (su cabecera `//!` lista las rutas):

`dashboard` · `logs` · `traces` · `services` · `metrics` · `errors` (Issues) ·
`events` (product events) · `funnels` · `retention` · `cohorts` · `experiments` ·
`insights` · `feature_flags` · `product_users` · `sessions` · `replays` ·
`monitors` · `alerts` · `projects` · `users` · `account`/`security` (2FA) ·
`preferences` · `integrations` · `health`.

### Ingesta — `ingest/`
| Módulo | Qué es |
|--------|--------|
| `mod.rs` | Router OTLP + utilidades: `resolve_project` (token→proyecto) y `check_origin` (whitelist CORS). |
| `logs`, `spans`, `metrics`, `events` | Receptores nativos por tipo de telemetría. |
| `replay` | Ingesta de session replays (chunks rrweb, límite 16 MiB). |
| `otlp`, `otlp_grpc`, `otlp_types` | Compatibilidad con OpenTelemetry (HTTP/JSON y gRPC). |
| `rate_limit` | Rate limiting de ingesta por proyecto. |

### Workers en segundo plano — `workers/`
Tareas tokio que corren fuera del request/response (cada `start_*` un bucle):

| Worker | Qué hace |
|--------|----------|
| `ingest_writer` | Escribe por lotes a ClickHouse (el otro extremo de los canales de ingesta). |
| `error_indexer` | Detecta logs de error en el bus en vivo, calcula `fingerprint` y los indexa como Issues. |
| `alert_evaluator` | Evalúa reglas de alerta y abre/cierra incidentes. |
| `monitor_runner` | Ejecuta los checks HTTP de los monitores. |
| `anomaly_detector` | Detecta anomalías (alimenta los insights). |
| `feature_rollback_detector` | Detecta flags que disparan errores y sugiere rollback. |
| `fingerprint_compactor` | Compacta/agrupa fingerprints de error. |
| `session_aggregator` | Agrega eventos en sesiones (duración, pageviews, replay…). |
| `stale_detector` | Marca servicios que dejaron de emitir. |
| `user_unifier` | Unifica identidades de product users (`distinct_id` ↔ anónimos). |

### Almacenamiento — `storage/`
| Módulo | Qué es |
|--------|--------|
| `client.rs` | Cliente HTTP sobre ClickHouse (inserta `JSONEachRow`, lee JSON; pool acotado). |
| `models.rs` | Structs de fila (`LogRow`, `SpanRow`, `ProductEventRow`, …) que mapean las tablas `faro.*`. |

### Notificaciones — `notify/` + canales
`notify/` tiene un emisor por destino: `telegram`, `slack`, `discord`,
`email_resend`, `opsgenie`, `pagerduty`, `webhook`. `notification_channels.rs` e
`integrations.rs` gestionan su configuración y cache.

### Dominio / seguridad (módulos raíz)
| Módulo | Qué es |
|--------|--------|
| `auth.rs` | Sesiones por cookie, hashing de password, gestión de usuarios. |
| `totp.rs` | 2FA por TOTP (secretos, verificación, códigos de recuperación). |
| `projects.rs` | Cache y resolución de proyectos por token de ingesta. |
| `feature_flags.rs` | Evaluación y cache de feature flags servidas a los SDKs. |
| `fingerprint.rs` | Hash determinista que agrupa errores equivalentes en un Issue. |
| `minhash.rs` | MinHash para similitud (agrupado aproximado de errores). |
| `redaction.rs` | Borrado de PII (reglas builtin + custom) antes de almacenar. |
| `origin_check.rs` | Validación del header `Origin` contra la whitelist del proyecto. |
| `stream.rs` | Streaming SSE en vivo (logs / events) sobre canales broadcast. |

---

## Glosario (términos del backend)

- **OTLP (HTTP/gRPC):** *OpenTelemetry Protocol*, el formato estándar de ingesta;
  por eso hay listeners en `:4318` (HTTP/JSON) y `:4317` (gRPC).
- **ingesta por batching:** los handlers HTTP empujan filas a canales en memoria
  (`IngestChannels`) y los `ingest_writer` las vuelcan a ClickHouse por lotes (N filas
  o T ms), para no hacer un `INSERT` por evento.
- **bus en vivo / SSE:** un canal `broadcast` por el que pasan logs/events nuevos;
  `stream.rs` los reexpone como Server-Sent Events para el modo "live" del dashboard.
- **fingerprint / Issue:** hash que agrupa errores del mismo defecto; cada grupo es
  un Issue (lo calcula `fingerprint.rs`, lo indexa `error_indexer`).
- **proyecto / token de ingesta:** un proyecto (por `slug`) aísla datos y tiene un
  token Bearer; `resolve_project` traduce token→proyecto en cada request de ingesta.
- **origin check:** el RUM SDK corre en un browser; se valida su header `Origin`
  contra la whitelist del proyecto (el `Origin` no es falsificable desde JS).
- **redaction:** borrado de datos personales (PII) según reglas, antes de persistir.
- **bootstrap:** al arrancar, el backend crea (si no existen) un proyecto, un token
  y un admin por defecto a partir de las variables `FARO_BOOTSTRAP_*`.
- **monitor / alert / incident:** un monitor es un check HTTP periódico; una regla de
  alerta evalúa métricas/monitores y, al dispararse, abre un incident y notifica.
- **product event / distinct_id:** evento de analítica de producto y el id estable
  del usuario final que lo emite (lo unifica `user_unifier`).

Para la jerga de producto vista desde la UI, ver también
[`../frontend/README.md`](../frontend/README.md).
