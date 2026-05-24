# Atajos para tareas comunes. Se documentan acá para que `make` sea el
# único comando que necesites recordar; los detalles viven en
# docs/development.md.

COMPOSE       ?= docker compose
COMPOSE_PROD  ?= docker compose -f docker-compose.prod.yml --env-file .env.prod
BACKEND       ?= cd backend &&
FRONTEND      ?= cd frontend &&

.DEFAULT_GOAL := help

.PHONY: help
help: ## Lista de targets disponibles
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z0-9_.-]+:.*?## / {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

## ─── Stack completo (Docker Compose) ─────────────────────────────────

.PHONY: up
up: ## Levanta el stack completo (dev)
	cp -n .env.example .env || true
	$(COMPOSE) up -d --build

.PHONY: down
down: ## Detiene el stack (preserva volúmenes)
	$(COMPOSE) down

.PHONY: reset
reset: ## Detiene el stack y BORRA todos los datos (volúmenes incluidos)
	$(COMPOSE) down -v
	$(COMPOSE) up -d --build

.PHONY: logs
logs: ## Tail de logs del backend
	$(COMPOSE) logs -f --tail=200 backend

.PHONY: logs-all
logs-all: ## Tail de logs de TODOS los servicios
	$(COMPOSE) logs -f --tail=100

.PHONY: ps
ps: ## Estado de los containers
	$(COMPOSE) ps

## ─── Backend (Rust) ──────────────────────────────────────────────────

.PHONY: backend
backend: ## Corre el backend en host contra ClickHouse de docker
	$(COMPOSE) up -d clickhouse redis
	$(BACKEND) cargo run

.PHONY: backend-test
backend-test: ## cargo nextest (requiere ClickHouse arriba; install: cargo install cargo-nextest)
	# nextest paraleliza tests cross-binary (los 11 archivos de tests/*.rs
	# en un único pool) vs `cargo test` que los serializa. La fixture aísla
	# por project_id → seguro contra CH compartido. Config:
	# backend/.config/nextest.toml.
	$(BACKEND) cargo nextest run --all-features --no-fail-fast

.PHONY: backend-check
backend-check: ## cargo fmt + clippy
	$(BACKEND) cargo fmt --all -- --check
	$(BACKEND) cargo clippy --all-targets -- -D warnings

.PHONY: backend-fmt
backend-fmt: ## Formatea con cargo fmt
	$(BACKEND) cargo fmt --all

## ─── Frontend (SvelteKit) ────────────────────────────────────────────

.PHONY: frontend
frontend: ## Dev server en :5173 apuntando a localhost:8080
	$(FRONTEND) PUBLIC_API_BASE=http://localhost:8080 npm run dev

.PHONY: frontend-check
frontend-check: ## svelte-check + tsc
	$(FRONTEND) npm run check

.PHONY: frontend-build
frontend-build: ## Build de producción
	$(FRONTEND) npm run build

## ─── ClickHouse ──────────────────────────────────────────────────────

.PHONY: ch
ch: ## Cliente interactivo de ClickHouse
	docker exec -it faro-clickhouse clickhouse-client --user=faro --password=faro --database=faro

.PHONY: migrate
migrate: ## Aplica todas las migraciones de clickhouse/migrations
	@for m in clickhouse/migrations/*.sql; do \
		[ -f "$$m" ] || continue; \
		echo "→ $$(basename $$m)"; \
		docker exec -i faro-clickhouse clickhouse-client --user=faro --password=faro --database=faro --multiquery < "$$m"; \
	done

## ─── Tráfico de prueba ───────────────────────────────────────────────

.PHONY: send-log
send-log: ## Envía un log de ejemplo al ingest nativo
	curl -X POST http://localhost:8080/api/v1/ingest/logs \
		-H "Authorization: Bearer dev-ingest-token" \
		-H "Content-Type: application/json" \
		-d '{"service":"demo","logs":[{"level":"INFO","message":"hello from make"}]}'

## ─── SDK release ─────────────────────────────────────────────────────

.PHONY: release-sdk
release-sdk: ## Tag y push de release de SDK. Uso: make release-sdk SDK=node VER=0.1.0
	@test -n "$(SDK)" || (echo "Falta SDK=<node|nextjs|expo|python|go|flutter|kotlin>"; exit 1)
	@test -n "$(VER)" || (echo "Falta VER=<semver>"; exit 1)
	git tag "sdk-$(SDK)-v$(VER)"
	git push origin "sdk-$(SDK)-v$(VER)"

## ─── Producción (sobre el host de prod) ──────────────────────────────

.PHONY: prod-logs
prod-logs: ## Logs del backend en prod
	$(COMPOSE_PROD) logs -f --tail=200 backend

.PHONY: prod-ps
prod-ps: ## Estado de containers en prod
	$(COMPOSE_PROD) ps

.PHONY: prod-deploy
prod-deploy: ## Rebuild + restart en prod (úsalo si el auto-deploy falló)
	$(COMPOSE_PROD) up -d --build --remove-orphans
