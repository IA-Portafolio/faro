# Architecture Decision Records (ADR)

Decisiones técnicas significativas se documentan acá. Una ADR captura
**por qué** se eligió algo, **qué alternativas** se descartaron y
**qué consecuencias** trae. Es más útil que un comentario en el código
cuando vuelves al proyecto en 6 meses preguntándote "¿y por qué hicimos
esto así?".

Formato: [Michael Nygard](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions).
Plantilla: [`template.md`](template.md).

## Cómo añadir una ADR

1. Copia `template.md` a `<NNNN>-<slug>.md` (NNNN = siguiente número
   secuencial con padding de 4 dígitos).
2. Llena las secciones. Sé explícito con _por qué_, no _qué_.
3. Estado inicial: `Proposed`. Tras discusión: `Accepted` / `Rejected`.
4. Si una ADR posterior reemplaza a esta, cámbiala a
   `Superseded by ADR-NNNN` (no la borres).

## Índice

| #    | Título                                               | Estado     |
| ---- | ---------------------------------------------------- | ---------- |
| 0001 | [Registrar decisiones de arquitectura](0001-record-architecture-decisions.md) | Accepted |
| 0002 | [ClickHouse como almacenamiento principal](0002-clickhouse-storage.md) | Accepted |
| 0003 | [Rust + axum para el backend](0003-rust-axum-backend.md) | Accepted |
| 0004 | [OTLP/HTTP+JSON como contrato de ingesta](0004-otlp-http-json-ingest.md) | Accepted |
| 0005 | [Sin autenticación nativa en el dashboard](0005-no-native-auth.md) | Accepted |
| 0006 | [OpenAPI spec autogenerada con utoipa](0006-openapi-utoipa.md) | Accepted |
| 0007 | [Self-observability via OTLP a sí mismo](0007-self-observability.md) | Accepted |
