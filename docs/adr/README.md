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
3. Estado inicial: `Proposed`. Tras discusión: `Accepted` o `Rejected`.
4. **Las ADRs son inmutables una vez `Accepted`** salvo correcciones
   tipográficas o aclaraciones del texto histórico. Si cambias de opinión,
   no edites el contenido viejo: escribe una **ADR nueva** que la reemplace y
   marca la anterior con uno de estos estados terminales:
   - `Superseded by ADR-NNNN` — la nueva ADR la reemplaza (total o parcial).
     Añade arriba un blockquote "**Nota:**" que apunte a la sucesora y
     explique brevemente qué parte cambió.
   - `Deprecated` — la decisión ya no aplica pero ninguna ADR la reemplaza
     (la funcionalidad se eliminó o quedó obsoleta sola).
   - `Rejected` — solo válido si la ADR nunca fue aceptada en primer lugar.

   Eso es lo que hace [Architectural Decision Log](https://adr.github.io/)
   como práctica: el log queda como cronología auditable de cómo evolucionó
   el pensamiento.

## Índice

| #    | Título                                               | Estado     |
| ---- | ---------------------------------------------------- | ---------- |
| 0001 | [Registrar decisiones de arquitectura](0001-record-architecture-decisions.md) | Accepted |
| 0002 | [ClickHouse como almacenamiento principal](0002-clickhouse-storage.md) | Accepted |
| 0003 | [Rust + axum para el backend](0003-rust-axum-backend.md) | Accepted |
| 0004 | [OTLP/HTTP+JSON como contrato de ingesta](0004-otlp-http-json-ingest.md) | Superseded by 0010 |
| 0005 | [Sin autenticación nativa en el dashboard](0005-no-native-auth.md) | Superseded by 0009 |
| 0006 | [OpenAPI spec autogenerada con utoipa](0006-openapi-utoipa.md) | Accepted |
| 0007 | [Self-observability via OTLP a sí mismo](0007-self-observability.md) | Superseded by 0011 |
| 0008 | [Compatibilidad SDK ↔ backend vía Faro-Protocol-Version](0008-sdk-version-compatibility.md) | Accepted |
| 0009 | [Endurecimiento de seguridad del backend](0009-security-hardening.md) | Accepted |
| 0010 | [Añadir OTLP/gRPC como segundo transporte de ingesta](0010-otlp-grpc-ingest.md) | Accepted |
| 0011 | [Self-monitoring vía Prometheus exposition externa](0011-prometheus-self-monitoring.md) | Accepted |
| 0012 | [Product events como 6º pilar de observabilidad](0012-product-events-sixth-pillar.md) | Accepted |
