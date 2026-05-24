# ADR-0007: Self-observability via OTLP a sí mismo

- **Estado**: Superseded by [ADR-0011](0011-prometheus-self-monitoring.md) (2026-05-24)
- **Fecha**: 2026-05-23
- **Autores**: @victalejo

> **Nota**: la práctica de auto-monitoreo en **producción** se movió a
> Prometheus exposition + Grafana externo (ADR-0011). El razonamiento de
> "Faro contra Faro" rompía cuando la propia pipeline se caía — el sistema
> que debe alertar no puede depender del sistema vigilado. La capa OTLP
> de este ADR (`FARO_SELF_OBSERVE=true`, módulo `crate::telemetry`) **sigue
> existiendo** como opt-in para correlación dev de logs + spans + métricas
> en la misma UI; lo que cambió es cuál es el camino esperado en prod.

## Contexto

Hoy, cuando algo va mal en el backend de Faro, la única herramienta de
diagnóstico es leer `docker compose logs -f backend`. Las latencias por
endpoint, errores por handler, throughput de los workers, tiempo
gastado en queries de ClickHouse — nada de eso está visible salvo que
abramos un strace. Faro es una plataforma de observabilidad pero **no
se observa a sí misma** — es vergonzoso y operacionalmente caro.

## Decisión

El backend emite **trazas, logs y métricas** de su propia ejecución a
un endpoint OTLP. Por defecto apunta al listener OTLP local
(`:4318`) — es decir, Faro se observa a sí mismo. Es opt-in vía la
variable de entorno `FARO_SELF_OBSERVE=true`.

Variables relevantes:

| Variable                       | Default                 | Para qué |
| ------------------------------ | ----------------------- | -------- |
| `FARO_SELF_OBSERVE`            | `false`                 | Activa/desactiva la emisión OTel |
| `FARO_SELF_OBSERVE_ENDPOINT`   | `http://localhost:4318` | Dónde mandar la telemetría (default: nosotros mismos) |
| `OTEL_SERVICE_NAME`            | `faro-backend`          | Nombre del servicio en la telemetría |

## Alternativas consideradas

- **Endpoint `/metrics` Prometheus** — más simple de implementar, pero
  acopla a Prometheus como scraper y duplica el camino con OTLP que ya
  tenemos. Faro almacena métricas OTLP nativamente; tiene más sentido
  enviárnoslas vía OTLP.
- **Dejarlo siempre activo (no env var)** — riesgo de loop de arranque
  en frío: en el primer boot, ClickHouse puede no estar lista, y los
  spans de ingesta fallidos se vuelven más spans fallidos. Opt-in lo
  evita.
- **Apuntar a un colector externo solo** — cierra la puerta al
  dogfooding (Faro contra Faro). Mejor configurable, default
  loopback.

## Consecuencias

### Positivas

- **Dogfooding real** — cualquier regresión en la pipeline de ingesta
  rompe primero la observabilidad del propio Faro, dándonos señal antes
  que cualquier cliente externo.
- **Latencias por handler / por SQL query** visibles en la propia UI
  de traces.
- **Errores agrupados** del backend aparecen en la vista de Errors,
  con stack traces si las anotamos.
- Probar contra otro Faro o un Tempo es trivial: cambiar
  `FARO_SELF_OBSERVE_ENDPOINT`.

### Negativas / costo asumido

- **5 deps nuevas** (`opentelemetry`, `opentelemetry_sdk`,
  `opentelemetry-otlp`, `opentelemetry-semantic-conventions`,
  `tracing-opentelemetry`). Pesan en tiempo de compilación.
- **Overhead de instrumentación** — cada span/log emitido cuesta
  serializar JSON + un round-trip HTTP. Mitigado por el batch exporter
  (default OTel: hasta 30s o 2048 spans).
- **Riesgo de loop infinito si el endpoint apunta a `:4318` y el
  backend muere a la mitad de procesar telemetría** — pero el batch
  exporter tira y olvida, no bloquea el path crítico.
- **Privacidad operacional**: spans contienen URLs, parámetros y a
  veces payload preview. Lo que el backend emita queda en su propia
  DB; está bien para self-hosted pero algo a documentar si se cambia
  el endpoint.

### Trabajo de seguimiento

Esta ADR cierra con el **scaffolding** del PR
`feat/self-observability`:

- ✅ Crates añadidos a `Cargo.toml`.
- ✅ `crate::telemetry::init_otel()` con env-var gate y guard de
  shutdown.
- ✅ Llamado desde `main()` antes de bootstrap.
- ✅ Resource attributes (`service.name`, `service.version`).
- ✅ Propagador `tracecontext` (W3C) registrado globalmente.

Queda por hacer (PRs incrementales):

- [ ] Capa `OpenTelemetryLayer` sobre `tracing-subscriber` para
      duplicar spans hacia el exporter (no solo stderr).
- [ ] Instrumentar handlers axum con `tower-http::trace::TraceLayer`
      + `make_span_with` que extraiga route/method.
- [ ] Instrumentar las queries a ClickHouse (cliente HTTP en
      `storage/`) con spans que capturen el SQL prefix y duración.
- [ ] Métricas: contador de spans/logs/metrics ingesteados, gauge de
      tamaño de canales mpsc, histograma de latencia de flush por
      tabla.
- [ ] Documentar en `docs/deployment.md` cómo activarlo y cómo
      apuntar a un colector externo.
- [ ] Health-gate: si `:4318` no responde tras 30 s, deshabilitar el
      exporter para evitar back-pressure indefinido.
