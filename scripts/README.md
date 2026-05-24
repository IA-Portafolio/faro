# scripts/

Tooling auxiliar del repo. Cada script tiene un header con propósito,
contrato y uso típico — esto es solo un índice para descubrirlos.

| Script | Para qué | Cuándo correr |
| ------ | -------- | ------------- |
| [`check-orphan-docs.sh`](check-orphan-docs.sh) | Verifica que cada archivo bajo `docs/` esté referenciado desde algún `.md` del repo. | Localmente antes de empujar un PR que añade docs; CI lo corre en el job `orphan-docs` de [`docs.yml`](../.github/workflows/docs.yml). |
| [`gen-env-reference.sh`](gen-env-reference.sh) | Regenera [`docs/reference/environment.md`](../docs/reference/environment.md) a partir de [`.env.example`](../.env.example). El `.env.example` es la fuente única de verdad para variables de entorno. | Cada vez que añadís, renombrás o cambiás el default de una variable en `.env.example`. |
| [`check-env-reference.sh`](check-env-reference.sh) | Verifica que la página generada esté en sync con `.env.example`. Imprime el diff si no. | Localmente; CI lo corre en el job `env-reference` de [`docs.yml`](../.github/workflows/docs.yml). |
| [`run-integration-tests.sh`](run-integration-tests.sh) | Setup + ejecución de los integration tests del backend dentro del container de `docker-compose.test.yml`. | Lo invoca el container `backend-test`. No correrlo a mano salvo dentro del container. |
| [`smoke-post-deploy.sh`](smoke-post-deploy.sh) | Round-trip post-deploy contra la instancia pública (`login → ingest → query → healthz`). Detecta el escenario "el deploy pasó readyz pero la ingesta o la auth están rotas". | Lo invoca el step "Smoke test post-deploy" de [`deploy.yml`](../.github/workflows/deploy.yml). Configurable con `FARO_SMOKE_*` (ver la [referencia de variables](../docs/reference/environment.md)). |

Para contexto del flujo de PR (qué corre el CI y qué tenés que correr
localmente antes), ver [CONTRIBUTING.md → Documentación y
configuración](../CONTRIBUTING.md).
