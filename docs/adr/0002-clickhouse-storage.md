# ADR-0002: ClickHouse como almacenamiento principal

- **Estado**: Accepted
- **Fecha**: 2026-05-23
- **Autores**: @victalejo

## Contexto

Faro almacena telemetría de alta cardinalidad: logs estructurados,
spans, métricas y eventos de error. Necesitamos un motor que soporte
ingesta sostenida (~miles/seg en una sola instancia), queries
agregadas rápidas sobre rangos de tiempo, y compresión razonable —
todo dentro de un único contenedor, sin operar un cluster.

## Decisión

Usamos **ClickHouse 24.x** como única fuente de verdad para logs,
spans, métricas, eventos de error, resultados de monitores e incidentes
de alertas. Cada tabla usa MergeTree con `PARTITION BY toYYYYMM(...)`
y TTL nativo de ClickHouse para retención (30 días logs, 14 traces,
90 métricas).

## Alternativas consideradas

- **Postgres + TimescaleDB** — bien para métricas, pero la ingesta
  de logs de alta cardinalidad (atributos arbitrarios serializados
  como JSON) sufre. Compresión muy inferior. Escalado vertical limitado.
- **Elasticsearch / OpenSearch** — referencia de la industria para
  logs, pero memory-hungry y operacionalmente más complejo (cluster,
  shards). Para self-hosted en un solo VM, mata.
- **Loki + Mimir + Tempo (stack Grafana)** — tres motores diferentes
  para tres tipos de señal multiplica operación. Faro busca _un_ stack.
- **DuckDB embebido** — interesante para single-node, pero no soporta
  ingesta concurrente con queries sostenidas; está pensado para
  analítica ad-hoc, no para una pipeline live.

## Consecuencias

### Positivas

- Un único motor cubre logs, traces, métricas y alertas con SQL
  estándar. El evaluador de alertas (`workers/alert_evaluator.rs`) es
  trivial porque la "DSL" de reglas es SQL crudo de ClickHouse.
- Compresión out-of-the-box (~10x sobre logs JSON crudos).
- Async inserts (configurados en `clickhouse/config/users.d`) absorben
  ráfagas sin que el backend tenga que implementar otro batching.

### Negativas / costo asumido

- ClickHouse es lo más pesado del compose (~600MB RAM en idle).
- Migraciones son a mano (no hay ORM); las hacemos idempotentes en
  `clickhouse/migrations/*.sql` y `deploy.yml` las aplica en cada push.
- Tooling de ecosistema (clientes, ORMs, dashboards drop-in) es
  menor que en Postgres.

### Trabajo de seguimiento

- Evaluar `ClickHouse Cloud` si la instancia self-hosted no escala.
- Considerar buffers Redis Streams entre backend y CH para sobrevivir
  caídas (placeholder ya está en el compose).
