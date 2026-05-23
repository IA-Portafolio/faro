# ADR-0001: Registrar decisiones de arquitectura

- **Estado**: Accepted
- **Fecha**: 2026-05-23
- **Autores**: @victalejo

## Contexto

Faro arrancó con varias decisiones tomadas en frío (ClickHouse vs
Postgres, Rust vs Go, OTLP/JSON vs gRPC, sin auth nativa, etc.). El
historial de git no captura el _porqué_; los commits dicen _qué_ pero
no _por qué se descartaron las alternativas_. En 6 meses, cualquiera
(incluido yo mismo) que vuelva a tocar el código va a pensar "esto
podría hacerse con X" sin saber que ya se evaluó X y se descartó.

## Decisión

Adoptamos [Architecture Decision Records](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
para capturar decisiones técnicas significativas. Cada ADR vive en
`docs/adr/<NNNN>-<slug>.md`, sigue la plantilla en `template.md` y se
referencia desde el índice en `docs/adr/README.md`.

Criterio para qué amerita una ADR: cualquier decisión que sea costosa
de revertir (elección de DB, lenguaje, contrato de API, modelo de
deployment) o que históricamente ha generado fricción al explicársela
a alguien nuevo.

## Alternativas consideradas

- **Comentarios en código** — invisibles desde fuera del archivo, no
  capturan alternativas descartadas, se vuelven obsoletos sin que
  nadie note.
- **Wiki externa (Notion, Confluence)** — se desincroniza del código,
  requiere permisos separados, no aparece en `grep`.
- **Mensajes de commit** — solo capturan el _qué_; el _por qué_ se
  diluye en el body y nadie lo lee meses después.

## Consecuencias

### Positivas
- Decisiones grandes quedan documentadas junto al código que las
  implementa.
- PRs futuros pueden citar ADRs (`closes ADR-0007`, `revisar a la luz
  de ADR-0012`).
- Onboarding de gente nueva es leer 10 archivos cortos, no engineering
  via osmosis.

### Negativas / costo asumido
- Disciplina: hay que recordar escribir ADRs antes de que la decisión
  se vuelva implícita. Mitigamos requiriéndolas en el PR template para
  cambios grandes.

### Trabajo de seguimiento
- Backfill de ADRs para decisiones ya tomadas (ClickHouse, Rust, OTLP,
  no-auth) — ADRs 0002 a 0005 cubren esto.
