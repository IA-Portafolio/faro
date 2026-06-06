<!--
  AUTOGENERADO por scripts/gen-env-reference.sh a partir de .env.example.
  NO EDITAR A MANO. Cambiá .env.example y corré:
      bash scripts/gen-env-reference.sh
-->

# Reference · Variables de entorno

Esta página enumera **todas** las variables de entorno que entienden el
backend de Faro, el `docker-compose` y los scripts de operación. Se
genera automáticamente desde [`.env.example`](../../.env.example), que
es la fuente única de verdad — README, `docs/deployment.md`,
`infra/README.md` y los templates de prod linkean acá en lugar de
mantener sus propias tablas.

Convenciones de la columna **Default**:

- `` `valor` `` — variable activa en `.env.example`; ese es el valor
  efectivo si copiás el archivo a `.env` sin tocarlo.
- `` `valor` · opcional`` — variable comentada en `.env.example`; el
  default lo aplica el código (`backend/src/config.rs`,
  `backend/src/telemetry.rs` o el propio `docker-compose.yml`).
  Descomentala sólo para anular el default.
- _(vacío)_ / _(sin setear)_ — la variable no se setea por defecto y el
  código activa el comportamiento "opcional" correspondiente
  (ej. `FARO_METRICS_TOKEN` deja `/metrics` abierto).

Para añadir una variable nueva: edita
[`.env.example`](../../.env.example), corre
`bash scripts/gen-env-reference.sh` y commitea la página resultante. CI
falla el PR si los dos archivos están desincronizados.

## ClickHouse

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `CLICKHOUSE_USER` | `faro` | Usuario del cliente HTTP. El compose lo crea con `CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT=1` para que pueda emitir DDL. |
| `CLICKHOUSE_PASSWORD` | `faro` | Password del usuario. En producción reemplazalo por algo generado con `openssl rand -base64 32`. |
| `CLICKHOUSE_HTTP_PORT` | `8123` | Puerto HTTP del contenedor de ClickHouse expuesto al host (API REST). Es lo que usa el backend para queries y bulk inserts. |
| `CLICKHOUSE_NATIVE_PORT` | `9000` | Puerto del protocolo nativo de ClickHouse expuesto al host. Sólo para inspección manual con el CLI; el backend no lo usa. |

## Redis

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `REDIS_PORT` | `6379` | Puerto de Redis expuesto al host. Está reservado para el futuro buffer durable de ingesta — hoy el contenedor corre pero ningún componente lo consume. |

## Puertos del backend

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_API_PORT` | `8080` | Puerto donde el backend sirve la API REST (`/api/v1/*`), SSE y `/healthz`. |
| `FARO_OTLP_HTTP_PORT` | `4318` | Puerto del listener OTLP/HTTP+JSON. Acepta `/v1/logs`, `/v1/traces`, `/v1/metrics` de cualquier exportador OpenTelemetry oficial. |
| `FARO_OTLP_GRPC_PORT` | `4317` | Puerto del listener OTLP/gRPC. Misma superficie OTLP pero con codec protobuf — los SDKs oficiales de OTel lo usan por default. |
| `FARO_INGEST_TOKEN` | `dev-ingest-token` | Token de ingesta que los SDKs/clientes ENVÍAN al backend (como `Authorization: Bearer`, header `x-faro-token`, o query `?_token=` para los beacons del browser al cerrar la pestaña). OJO: el backend NO lee esta variable — autentica matcheando el token recibido contra el `ingest_token` de cada proyecto. Su valor debe COINCIDIR con el de un proyecto; en dev/single-project es el mismo que `FARO_BOOTSTRAP_INGEST_TOKEN` (abajo). La leen los SDKs Node y Python. En producción: `openssl rand -hex 32`. |

## Bootstrap (primer arranque)

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_BOOTSTRAP_PROJECT_SLUG` | `default` | Slug del proyecto seed (segmento de URL y prefijo en logs). |
| `FARO_BOOTSTRAP_PROJECT_NAME` | `Default` | Nombre humano del proyecto seed (lo que muestra el dashboard). |
| `FARO_BOOTSTRAP_INGEST_TOKEN` | `dev-ingest-token` | Token de ingesta del proyecto seed que el backend crea en el primer arranque (cuando la BD está vacía). ESTE sí lo lee el backend. Es el token que los clientes deben enviar (ver `FARO_INGEST_TOKEN`). Para crear más proyectos o rotar tokens: `POST /api/v1/projects` y `POST /api/v1/projects/{slug}/rotate`. En producción tiene que ser un secret real (`openssl rand -hex 32`). |
| `FARO_BOOTSTRAP_ADMIN_EMAIL` | `admin@local.test` | Email del admin seed. Será el primer login del dashboard. |
| `FARO_BOOTSTRAP_ADMIN_PASSWORD` | `admin12345` | Password del admin seed (almacenado hasheado). Cambialo apenas entres por primera vez al dashboard. |
| `FARO_BOOTSTRAP_ADMIN_NAME` | `Admin` | Nombre humano del admin seed (aparece en la UI). |

## Frontend

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FRONTEND_PORT` | `3000` | Puerto del dashboard SvelteKit expuesto al host. |
| `FRONTEND_ORIGIN` | `http://localhost:3000` | Origin permitido por SvelteKit (header `Origin` para CSRF). Cambialo al dominio público cuando despliegues con HTTPS (ej. `https://faro.example.com`). |
| `PUBLIC_API_BASE` | `http://localhost:8080` | URL base que el frontend usa para llamar al backend. Se inlinea en tiempo de build, así que un cambio acá requiere rebuild del contenedor del frontend. |

## Logging

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `RUST_LOG` | `info,faro=debug` | Filtro de `tracing-subscriber` (formato `RUST_LOG`). `info` global y `debug` para los módulos del propio crate `faro`. |

## Cliente ClickHouse (backend)

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `CLICKHOUSE_URL` | `http://localhost:8123` · opcional | URL del servicio ClickHouse vista por el backend. En el docker-compose de dev ya viene hardcodeada apuntando al hostname del servicio (`http://clickhouse:8123`); sólo hace falta setearla cuando corres el backend fuera del compose. |
| `CLICKHOUSE_DATABASE` | `faro` · opcional | Nombre de la base de datos a usar. Si no existe, las migraciones la crean en el primer arranque. |
| `FARO_CLICKHOUSE_POOL_MAX_IDLE` | `64` · opcional | Máximo de conexiones HTTP idle que el cliente del backend mantiene cacheadas hacia ClickHouse. Cap a 1 para evitar que un typo (`0`) desactive el pool y mate throughput. |

## Listeners (override de los puertos del compose)

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_API_ADDR` | `0.0.0.0:8080` · opcional | Address completo del listener REST. Anula a `FARO_API_PORT` si lo necesitás bindear a una interfaz específica. |
| `FARO_BIND_ADDR` | `0.0.0.0:4318` · opcional | Address del listener OTLP/HTTP. Anula a `FARO_OTLP_HTTP_PORT`. |
| `FARO_OTLP_GRPC_ADDR` | `0.0.0.0:4317` · opcional | Address del listener OTLP/gRPC. Anula a `FARO_OTLP_GRPC_PORT`. |

## Limites de SSE y rate limit de ingesta

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_SSE_MAX_PER_PROJECT` | `10` · opcional | Tope de subscriptores SSE simultáneos por proyecto. Sin tope un cliente abriendo tabs en loop puede inflar el broadcast channel y el conteo de conexiones HTTP. |
| `FARO_SSE_MAX_GLOBAL` | `100` · opcional | Tope global de subscriptores SSE en el proceso. Acota el daño aun cuando un atacante itere por proyectos para evitar el cap por-proyecto. |
| `FARO_INGEST_RATE_PER_SECOND` | `5000` · opcional | Records/segundo aceptados por proyecto en cualquier endpoint de ingesta (OTLP/HTTP, OTLP/gRPC, `/logs`). Burst = 2× este valor. |

## Batching del writer

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_BATCH_MAX_ROWS` | `5000` · opcional | Umbral de flush por tabla — el writer hace insert cuando acumula este número de filas o cuando vence `FARO_BATCH_FLUSH_MS`, lo que ocurra primero. |
| `FARO_BATCH_FLUSH_MS` | `750` · opcional | Tiempo máximo (ms) que el writer espera para flushear una tabla aunque no haya alcanzado `FARO_BATCH_MAX_ROWS`. |
| `REDIS_URL` | `redis://redis:6379` · opcional | URL de Redis. Hoy el backend NO la usa para nada productivo — está reservada para el futuro buffer durable de ingesta. El compose la inyecta como `redis://redis:6379` y el código la lee, pero no la usa. |

## Seguridad / endurecimiento

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_METRICS_TOKEN` | _(sin setear)_ · opcional | Si está definido, `/metrics` exige `Authorization: Bearer <token>` y rechaza cualquier otra cosa con 401. Vacío/sin definir = endpoint abierto (apropiado para dev o cuando `/metrics` no es accesible públicamente). |
| `FARO_ENABLE_HSTS` | `false` · opcional | Si `true`, el backend agrega `Strict-Transport-Security` a las respuestas del dashboard. El browser cachea HSTS por un año por origen, así que encenderlo en dev rompe testing en HTTP. Activar SÓLO en producción con TLS estable. |
| `FARO_PUBLIC_BASE_URL` | `http://localhost:8080` · opcional | URL pública del backend, usada por el backend mismo para armar enlaces absolutos (ej. el reset-password de email). Cambialo al dominio público cuando expongas la API. |

## Detector de anomalías por z-score

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_ANOMALY_ENABLED` | `true` · opcional | Activa el worker que compara la tasa actual de cada (proyecto, servicio, señal) contra la misma franja horaria de los últimos 7 días y dispara incidentes en `faro.alert_incidents` cuando el z-score se desvía. |
| `FARO_ANOMALY_INTERVAL_SECS` | `300` · opcional | Cadencia (segundos) con la que el worker re-evalúa todas las series. |
| `FARO_ANOMALY_WINDOW_MINUTES` | `5` · opcional | Ventana en minutos para la observación actual y para cada muestra histórica. 5 balancea sensibilidad a spikes cortos vs ruido de Poisson. |
| `FARO_ANOMALY_Z_FIRE` | `3.0` · opcional | Z-score a partir del cual se dispara el incidente. |
| `FARO_ANOMALY_Z_RESOLVE` | `1.5` · opcional | Z-score por debajo del cual se resuelve el incidente. Hysteresis para no aletear en el borde. |
| `FARO_ANOMALY_MIN_BASELINE_ERRORS` | `2.0` · opcional | Baseline mínimo de errors/min para considerar la serie "interesante". Evita que una sola observación contra media casi-cero produzca z-score astronómico. |
| `FARO_ANOMALY_MIN_BASELINE_P95_MS` | `20.0` · opcional | Baseline mínimo de p95 (ms) de spans para considerar la serie. |
| `FARO_ANOMALY_MIN_BASELINE_LOGS` | `50.0` · opcional | Baseline mínimo de logs/min para considerar la serie. |

## Rollback recomendado por feature flags

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_FEATURE_ROLLBACK_ENABLED` | `true` · opcional | Activa el worker que cruza `$feature_exposure` con `error_events` vía `trace_id` y dispara un incidente cuando la variante B del flag tiene mucha más tasa de errores que A. |
| `FARO_FEATURE_ROLLBACK_INTERVAL_SECS` | `300` · opcional | Cadencia (segundos) con la que se re-evalúan flags expuestas recientemente. |
| `FARO_FEATURE_ROLLBACK_WINDOW_MINUTES` | `15` · opcional | Ventana reciente (minutos) de exposures, product events trazados y errores. |
| `FARO_FEATURE_ROLLBACK_RATIO` | `5.0` · opcional | Ratio B/A de tasa de errores a partir del cual se recomienda rollback. |
| `FARO_FEATURE_ROLLBACK_RESOLVE_RATIO` | `2.0` · opcional | Ratio B/A por debajo del cual se resuelve el incidente abierto. |
| `FARO_FEATURE_ROLLBACK_MIN_SAMPLE` | `20` · opcional | Mínimo de usuarios expuestos por variante para evaluar la señal. |
| `FARO_FEATURE_ROLLBACK_MIN_TREATMENT_ERRORS` | `5` · opcional | Mínimo de errores ligados a la variante B para evitar ruido de muestras chicas. |

## Compactador de fingerprints

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_FP_COMPACTOR_ENABLED` | `true` · opcional | Activa el worker que agrupa errores semánticamente equivalentes (Jaccard ≥ umbral) que el hash exacto de `fingerprint.rs` deja como issues separados. |
| `FARO_FP_COMPACTOR_INTERVAL_SECS` | `1800` · opcional | Cadencia (segundos) del compactor. |
| `FARO_FP_COMPACTOR_JACCARD` | `0.85` · opcional | Umbral de similitud MinHash/Jaccard para fusionar dos fingerprints. |

## Detector de servicios stale

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_STALE_DETECTOR_ENABLED` | `true` · opcional | Activa el worker que detecta servicios que dejaron de emitir tráfico. |
| `FARO_STALE_DETECTOR_INTERVAL_SECS` | `3600` · opcional | Cadencia (segundos) del detector. |
| `FARO_STALE_THRESHOLD_HOURS` | `24` · opcional | Horas sin tráfico tras las cuales un servicio se marca como stale. |

## Unificación de usuarios multi-device (goal 10.E.1)

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_USER_UNIFIER_ENABLED` | `true` · opcional | Activa el worker que mantiene `product_users` y `product_user_aliases` a partir de `product_events`. Sin esto, esas dos tablas quedan vacías y el endpoint `GET /api/v1/product-users/:id/events` (todos los eventos del user en cualquier device) responde sólo lo que matchea `distinct_id` directo, sin expandir a los anon_ids ligados. |
| `FARO_USER_UNIFIER_INTERVAL_SECS` | `60` · opcional | Cadencia (segundos) entre ticks del unificador. Cada tick mira eventos desde `last_watermark - 30s` para no perder eventos justo después de un tick. Tope interno: 5 000 users/tick. |

## Session aggregator (goal 10.F.1)

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_SESSION_AGGREGATOR_ENABLED` | `true` · opcional | Activa el worker que sesionaliza `product_events` y mantiene `faro.product_sessions`. Si el SDK manda `session_id` en el evento, se respeta; si no, se cortan sesiones por gap > `FARO_SESSION_GAP_MINUTES`. |
| `FARO_SESSION_AGGREGATOR_INTERVAL_SECS` | `300` · opcional | Cadencia (segundos) del agregador. 5 min es lo bastante fino para que las sesiones "vivas" se actualicen en near-real-time sin escanear `product_events` en cada request. |
| `FARO_SESSION_GAP_MINUTES` | `30` · opcional | Minutos de inactividad tras los cuales se considera que una sesión terminó y la siguiente actividad del mismo `(project, distinct_id)` arranca una nueva. 30 es la convención de GA/Mixpanel; cambiarlo recalcula retroactivamente el conteo de sesiones en ticks sucesivos. |
| `FARO_SESSION_LOOKBACK_MINUTES` | `360` · opcional | Cuánto hacia atrás mira el worker en cada tick. Debe ser >= a la duración máxima realista de una sesión activa — si una sesión sigue viva más allá de este horizonte, su `started_at` (y el `session_id` sintético) puede drift al fall out de la ventana en runs sucesivos. Para sesiones cuyo `session_id` viene del SDK no aplica el drift. |

## Notificaciones · Telegram

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `TELEGRAM_BOT_TOKEN` | `123456:ABC-DEF...` · opcional | Token global del bot de Telegram. Si está definido, los destinos `tg://<chat_id>` usan este bot. Los destinos `tg://<chat_id>@<token>` siempre pueden traer su propio token y no necesitan que esté configurado a nivel global. La integración guardada en BD (Settings → Integraciones) tiene prioridad sobre esta variable. |
| `TELEGRAM_API_BASE` | `https://api.telegram.org` · opcional | Base de la API de Telegram. Configurable sólo para pruebas — en producción usar el default. |

## Self-observability

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_SELF_OBSERVE` | `false` · opcional | Si `true`, el backend exporta sus propios logs y trazas vía OTLP al endpoint configurado abajo. Opt-in para evitar back-pressure cuando el listener no responde en frío (ver ADR-0007). |
| `FARO_SELF_OBSERVE_ENDPOINT` | `http://localhost:4318` · opcional | Dónde mandar la telemetría del propio backend. Default: el listener OTLP local — el backend se observa a sí mismo. |
| `OTEL_SERVICE_NAME` | `faro-backend` · opcional | Nombre de servicio que aparece en la telemetría auto-emitida. |

## Smoke test post-deploy (CI)

| Variable | Default | Descripción |
| -------- | ------- | ----------- |
| `FARO_SMOKE_EMAIL` | `smoke@example.com` · opcional | Email del usuario dedicado para el smoke test. |
| `FARO_SMOKE_PASSWORD` | _(sin setear)_ · opcional | Password del usuario de smoke (setear en GitHub Secrets, no en plano). |
| `FARO_SMOKE_INGEST_TOKEN` | _(sin setear)_ · opcional | Token Bearer del proyecto dedicado al smoke (slug "smoke" o similar). |
