## Qué cambia

<!-- 1-3 frases. Qué cambia y por qué. Links a issue/discusión si aplica. -->

## Cómo lo probé

<!-- Comandos exactos, screenshots, o pasos manuales. Si no se pudo probar
manualmente (ej. cambio de infra), dilo explícitamente. -->

## Checklist

- [ ] CI pasa (`ci.yml`: fmt, clippy, test, build, compose).
- [ ] Si toca el esquema de ClickHouse, hay migración idempotente en `clickhouse/migrations/`.
- [ ] Si toca la API REST o el contrato OTLP, el README está actualizado.
- [ ] Si toca un SDK, su README/CHANGELOG está actualizado.
- [ ] Si introduce un breaking change, está marcado en `CHANGELOG.md`.

## Notas para el reviewer

<!-- Decisiones de diseño, alternativas descartadas, deuda asumida. -->
