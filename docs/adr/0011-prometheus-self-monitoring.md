# ADR-0011: Self-monitoring vía Prometheus exposition externa

- **Estado**: Accepted
- **Fecha**: 2026-05-24
- **Autores**: @victalejo
- **Reemplaza**: parcialmente a [ADR-0007](0007-self-observability.md) en lo
  relativo a qué stack monitorea al backend en producción.

## Contexto

ADR-0007 decidió hacer **dogfooding puro**: Faro emite OTLP de su propia
ejecución hacia su propio listener `:4318` y se observa con su propia UI. El
argumento clave de descartar un endpoint `/metrics` Prometheus era: "duplica
el camino con OTLP que ya tenemos".

El dogfooding funciona como gimnasia (cualquier regresión rompe primero la
visibilidad del propio Faro), pero tiene un problema operacional:

1. **Cuando la ingesta de Faro se cae, perdemos visibilidad de que se cayó**.
   El runner de alertas, el writer a ClickHouse y la propia UI dependen de
   que la pipeline esté viva. Si está rota, no hay forma de alertar desde
   adentro: el sistema que debería avisar es el que está caído.
2. **El operador típico tiene Prometheus + Grafana ya corriendo** para el
   resto de su infra. Pedirle que abra otra UI (la de Faro) para ver salud
   del propio Faro contradice la práctica estándar de "monitoring lives
   outside the thing being monitored".
3. **Las métricas que importan para SRE son de cardinalidad baja y
   acotadas** (`ingest_records_total`, `clickhouse_insert_duration_seconds`,
   `rate_limited_total`). Encajan perfectamente en el modelo
   counter/histogram/gauge de Prometheus, y sobrarían en un store
   columnar de eventos como ClickHouse.

Faro hoy corre detrás de Caddy en `infra-iaportafolio`, y un Prometheus +
Grafana en `iaportafolio` lo scrapea — exactamente el patrón "monitor desde
afuera". Esa práctica ya está consolidada; esta ADR la documenta.

## Decisión

El backend expone **`/metrics` en formato Prometheus textual** en el listener
de API (`:8080`). Un Prometheus externo scrapea ese endpoint; un Grafana
externo lo grafica y alerta. Esa es la ruta **primaria** de auto-monitoreo en
producción.

`FARO_SELF_OBSERVE=true` (ADR-0007) **sigue existiendo** y se mantiene como
opt-in para escenarios de desarrollo o cuando se quiere correlacionar
métricas con trazas/logs en la misma UI. Pero ya no es el camino esperado
para alerting operacional.

Implementación:

- `crate::observability::install()` registra un recorder global de
  `metrics_exporter_prometheus` y devuelve un layer `axum-prometheus` que se
  monta sobre el router de API y sobre el router OTLP/HTTP (no sobre
  OTLP/gRPC, que pasa por tonic).
- `GET /metrics` queda **excluido del middleware de auth** (es público) pero
  puede protegerse con `FARO_METRICS_TOKEN=<bearer>` si el endpoint es
  accesible desde fuera de la red privada.
- Las labels permitidas se documentan en el módulo (`project`, `signal`,
  `outcome`, `table`, `operation`) y **nunca** incluyen `trace_id`,
  `user_id`, ni texto libre — evitar explosión de cardinalidad es invariante
  del módulo.

## Alternativas consideradas

- **Mantener solo OTLP self-observe** (status quo de ADR-0007) — sigue
  válido para correlación dev pero no resuelve "alertar cuando Faro se
  cae". El propio ADR-0007 listaba esto en "Negativas / costo asumido"
  (el riesgo de loop), simplemente lo aceptaba. Hoy ya tenemos la
  experiencia operacional para no aceptarlo más.
- **Empujar métricas a un Pushgateway** — más complejo y pierde la
  semántica pull de Prometheus (cuando el target no responde, sabes que
  está caído; con push, la ausencia es ambigua).
- **Sidecar de OTel Collector que traduzca OTLP→Prometheus exposition** —
  añade un proceso al compose y duplica los mismos counters por dos caminos.
  Más simple servirlos directo desde el backend.
- **Reemplazar self-observe OTLP por solo Prometheus** — perdería el
  dogfooding (cuando funciona, es genuinamente útil) y la capacidad de ver
  trazas correlacionadas. Mantener ambos cuesta poco.

## Consecuencias

### Positivas

- **Alerting confiable**: Prometheus corre en otro host; cuando Faro se cae,
  Alertmanager dispara. No hay punto único de falla.
- **Encaja con la infraestructura estándar** del operador típico (Prom +
  Grafana ya existen). Cero UI nueva para SRE.
- **Cardinalidad acotada por diseño**: las labels están documentadas y
  centralizadas en `observability::names`; cualquier intento de añadir
  `request_id` o similar requiere editar ese módulo y pasa por code review.
- **El layer `axum-prometheus` mide ambos routers HTTP** (API + OTLP/HTTP)
  sin código extra por handler.

### Negativas / costo asumido

- **Dos caminos de telemetría** que pueden divergir: uno o varios counters
  podrían existir solo en Prometheus o solo en OTLP. Mitigado porque los
  nombres viven en un solo módulo y las métricas críticas (records
  ingesteados, errores de ClickHouse) se incrementan vía `metrics::counter!`,
  que llega al recorder Prometheus pero no al exporter OTLP.
- **OTLP/gRPC no se mide** por el layer axum (tonic no es axum). Los
  handlers gRPC incrementan los counters de aplicación a mano; latencia HTTP
  por servicio gRPC no está expuesta. Trade-off aceptable hasta que la
  proporción gRPC vs HTTP justifique instrumentar tonic explícitamente.
- **`/metrics` es público por default** si `FARO_METRICS_TOKEN` no se
  configura. En instancias accesibles desde internet hay que setearlo (o
  ponerle el firewall delante). Documentado en `docs/deployment.md`.

### Trabajo de seguimiento

- **[ADR-0007](0007-self-observability.md)**: marcada como `Superseded by
  ADR-0011` en lo relativo a "qué stack monitorea Faro en producción". El
  resto del contenido (variables `FARO_SELF_OBSERVE*`, scaffolding de
  `crate::telemetry`) sigue válido como camino dev/opt-in.
- **Dashboards de Grafana** versionados en `observability/grafana/` (JSON +
  provisioning) para que un operador nuevo no tenga que reconstruirlos a
  mano.
- **Reglas de alerta** (`observability/prometheus/rules.yml`) para:
  `clickhouse_errors_total > 0` (filas perdidas), latencia p99 de insert >
  umbral, ratio `rate_limited / accepted` > umbral por proyecto.
- **Instrumentar tonic** con histogramas de latencia por servicio gRPC si
  el volumen lo justifica; hoy solo tenemos counters.
