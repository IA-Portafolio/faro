# docs/

Documentación de Faro. Organizada en tres tipos:

- **Guías** — instructivos paso a paso para una tarea concreta.
- **Reference** — listas de verdad autogeneradas o curadas que el código
  consume implícita o explícitamente.
- **ADR** — decisiones de arquitectura, formato Michael Nygard, una por
  archivo numerado.

## Guías

| Doc | Para qué |
| --- | -------- |
| [development.md](development.md) | Levantar el stack local, dev loop de backend/frontend/ClickHouse, migraciones, enviar tráfico de prueba. |
| [testing.md](testing.md) | La red de regresión completa: qué cubre cada componente, cómo correr todo con [`scripts/test-all.sh`](../scripts/test-all.sh), y las reglas para que la cobertura no se degrade al sumar features. |
| [deployment.md](deployment.md) | Topología productiva, primer despliegue, deploy continuo desde el self-hosted runner, persistencia, rollback, observabilidad. |
| [anomaly-detection.md](anomaly-detection.md) | Cómo funciona el detector de anomalías por z-score (ventanas, baseline, fire/resolve, hysteresis). |
| [feature-flags-experiments.md](feature-flags-experiments.md) | Feature flags, exposures, A/B testing y rollback recomendado cuando treatment sube errores. |
| [product-analytics.md](product-analytics.md) | Vistas y APIs de product analytics: users, retention, sessions, replay e insights combinados. |

## Reference

| Doc | Origen | Para qué |
| --- | ------ | -------- |
| [reference/environment.md](reference/environment.md) | autogenerado desde [`.env.example`](../.env.example) por [`scripts/gen-env-reference.sh`](../scripts/gen-env-reference.sh) | Todas las variables de entorno que entienden el backend, el compose, el frontend y los scripts. Una sola página, con default y descripción por var. |

> **No edites archivos bajo `reference/` a mano.** Son autogenerados.
> CI rechaza el PR si el archivo está desincronizado con su fuente
> ([`scripts/check-env-reference.sh`](../scripts/check-env-reference.sh)
> en el job `env-reference` de [`docs.yml`](../.github/workflows/docs.yml)).
> Para regenerar: `bash scripts/gen-env-reference.sh`.

## ADR

Ver [adr/README.md](adr/README.md) para el índice. Las ADRs se numeran
secuencialmente (`NNNN-slug.md`) y capturan **por qué** se eligió algo,
**qué alternativas** se descartaron y **qué consecuencias** trae.

## Reviews

| Doc | Para qué |
| --- | -------- |
| [superpowers/reviews/2026-05-24-10-b-session-review.md](superpowers/reviews/2026-05-24-10-b-session-review.md) | Revision de SDK tracking/events, ingesta de product events e identidad. |
| [superpowers/reviews/2026-05-24-10-j-observability-integration-review.md](superpowers/reviews/2026-05-24-10-j-observability-integration-review.md) | Revision de insights 10.J y metricas virtuales derivadas de product events. |

## Invariantes

- **Cada archivo bajo `docs/` debe estar enlazado** desde algún índice
  (este README, el README raíz, una ADR, otro doc). El job `orphan-docs`
  en [`docs.yml`](../.github/workflows/docs.yml)
  ([`scripts/check-orphan-docs.sh`](../scripts/check-orphan-docs.sh))
  bloquea el merge si añadís un doc sin enlazar. Un doc no descubrible
  es como no existir.

- **No copies tablas de la reference en otros lados.** README,
  `deployment.md`, `infra/README.md` y `.env.prod.template` linkean a
  `reference/environment.md` en vez de mantener tablas paralelas que se
  desincronizan a la primera variable nueva.
