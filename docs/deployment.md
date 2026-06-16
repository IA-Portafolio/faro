# Deployment

Cómo se despliega Faro en producción. La instancia operada está en
<https://faro.iaportafolio.com>. Para CI/CD del repo ver
[`infra/README.md`](../infra/README.md).

## Topología

```text
Internet
   ↓ HTTPS (Cloudflare / proxy del host)
faro.iaportafolio.com
   ↓
docker compose (docker-compose.prod.yml)
   ├── faro-frontend     (SvelteKit Node adapter, :3000 interno)
   ├── faro-backend      (Rust, :8080 API + /metrics, :4318 OTLP/HTTP, :4317 OTLP/gRPC)
   ├── faro-clickhouse   (volumen persistente)
   └── faro-redis        (reservado, sin uso productivo aún)
```

Todo corre en el host `infra-iaportafolio` bajo `/opt/faro/`.

## Despliegue manual (primera vez)

```bash
ssh infra-iaportafolio
sudo mkdir -p /opt/faro && sudo chown $USER /opt/faro
cd /opt/faro
git clone https://github.com/IA-Portafolio/faro .
cp .env.prod.template .env.prod
# Edita .env.prod y define CLICKHOUSE_PASSWORD y FARO_BOOTSTRAP_INGEST_TOKEN
# (es el token de ingesta que el backend acepta; los SDKs envían ese mismo valor)
nano .env.prod
docker compose -f docker-compose.prod.yml --env-file .env.prod up -d --build
```

Verifica:

```bash
curl https://faro.iaportafolio.com/readyz
```

> `/readyz` (no `/healthz`) valida que ClickHouse responde — falla si CH no
> está listo. `/healthz` es un ping liveness sin dependencias.

## Despliegue continuo

A partir del primer despliegue, **cada push a `main` redespliega
automáticamente** vía el workflow `deploy.yml` corriendo en el self-hosted
runner del host. No hace SSH — opera localmente sobre `/opt/faro/`.

Ver [`infra/README.md`](../infra/README.md) sección 1 para cómo instalar
y operar el runner.

Pasos del deploy automático:

1. `rsync` del checkout hacia `/opt/faro/` (preserva `.env.prod`, volúmenes).
2. Aplica todas las migraciones idempotentes en `clickhouse/migrations/`.
3. `docker compose build` (aprovecha cache de capas).
4. `docker compose up -d --remove-orphans`.
5. Polling de `https://faro.iaportafolio.com/readyz` (hasta 40 reintentos).
6. Smoke test post-deploy (`scripts/smoke-post-deploy.sh`): login + ingest + query round-trip contra el dominio público. Cubre el caso "readyz verde pero la auth/ingesta/CH están rotas".
7. `docker image prune -f`.

## Variables de entorno productivas

`.env.prod` (no se commitea, vive solo en el host). Ver
[`.env.prod.template`](../.env.prod.template) para el formato y la
[referencia completa de variables](reference/environment.md) — la lista
completa con defaults y descripción, autogenerada desde `.env.example`.

Valores que **deben** setearse manualmente en `.env.prod` antes del
primer deploy (el resto tiene un default razonable):

| Variable                  | Cómo generarlo            |
| ------------------------- | ------------------------- |
| `CLICKHOUSE_PASSWORD`     | `openssl rand -base64 32` |
| `FARO_BOOTSTRAP_INGEST_TOKEN` | `openssl rand -hex 32` — token de ingesta del proyecto seed; lo lee el backend |
| `FARO_BOOTSTRAP_ADMIN_EMAIL` / `FARO_BOOTSTRAP_ADMIN_PASSWORD` | Credenciales del primer login del dashboard |
| `FRONTEND_ORIGIN`         | URL pública del dashboard |
| `PUBLIC_API_BASE`         | URL pública del backend   |

> El backend **no** lee `FARO_INGEST_TOKEN`; autentica la ingesta matcheando
> el token recibido contra el `ingest_token` de cada proyecto. En los SDKs,
> `FARO_INGEST_TOKEN` es el valor que el cliente envía — debe coincidir con
> `FARO_BOOTSTRAP_INGEST_TOKEN` (o con el token de un proyecto creado vía
> `POST /api/v1/projects`).

### Notificaciones de Telegram

Para que las reglas de alerta puedan notificar por Telegram:

1. Crear un bot con [@BotFather](https://t.me/BotFather), copiar el token
   (`123456:ABC-DEF...`).
2. Añadir el bot al chat / grupo / canal donde se recibirán las alertas
   y obtener el `chat_id` (los grupos son negativos, los canales empiezan
   por `-100`). Para chats privados, escribirle al bot y mirar
   `https://api.telegram.org/bot<TOKEN>/getUpdates`.
3. En el dashboard, ir a **Integraciones** y pegar el token. Persiste en
   ClickHouse (`faro.integrations`) y queda compartido entre réplicas del
   backend. Desde ahí mismo puedes mandar un mensaje de prueba.
4. En la UI de Alertas, añadir destinos con la forma:
   - `tg://-1001234567890` — usa el bot configurado en Integraciones.
   - `tg://@mi_canal` — canales públicos por nombre.
   - `tg://<chat_id>@<otro_token>` — token por destino, útil si quieres
     enviar a chats que pertenecen a otro bot sin tocar la configuración
     global.

Los destinos `https://...` siguen funcionando como webhooks JSON
genéricos (Slack/Discord/custom).

> Compatibilidad: la variable de entorno `TELEGRAM_BOT_TOKEN` sigue
> funcionando como fallback cuando no hay integración guardada — útil
> para self-host con configuración 100 % declarativa. El orden de
> resolución es **token inline → integración en BD → variable de
> entorno**.

## Persistencia

Volúmenes Docker (declarados en `docker-compose.prod.yml`):

| Volumen           | Contenido                                |
| ----------------- | ---------------------------------------- |
| `faro-clickhouse` | Datos de ClickHouse (logs, traces, etc.) |
| `faro-redis`      | Cola/cache (vacío por ahora)             |

Backups: responsabilidad operacional, no automatizada en el repo. Ver
faro-deploy en la memoria del operador para el procedimiento actual.

## Rollback

Cualquiera de estas tres opciones:

```bash
# 1. Revertir el commit y empujar (el deploy vuelve a correr)
git revert <bad-sha> && git push

# 2. Forzar checkout en el host y rebuild manual
ssh infra-iaportafolio
cd /opt/faro && git fetch && git checkout <good-sha>
docker compose -f docker-compose.prod.yml --env-file .env.prod up -d --build

# 3. Pausar el runner mientras investigas
sudo /opt/actions-runner/svc.sh stop
```

## Observabilidad del propio Faro

Para la operación en producción usamos **Prometheus exposition + Grafana
externos** (ver [ADR-0011](adr/0011-prometheus-self-monitoring.md)). El backend
expone `/metrics` en el listener de API (`:8080`); un Prometheus
externo lo scrapea y un Grafana externo lo grafica y alerta. La instancia
operada usa el Prometheus+Grafana que corre en el host `iaportafolio` para
monitorear el host `infra-iaportafolio` donde vive Faro — el monitor no
depende del sistema vigilado.

Configuración relevante: `FARO_METRICS_TOKEN` (proteger `/metrics` con
Bearer token) y la sección **Self-observability** de la
[referencia de variables](reference/environment.md) (`FARO_SELF_OBSERVE*`,
`OTEL_SERVICE_NAME`). Las series están documentadas en
`crate::observability::names` (`faro_ingest_records_total`,
`faro_clickhouse_insert_duration_seconds`, etc.) y las labels permitidas
están acotadas a cardinalidad baja por diseño.

### Self-observe vía OTLP (opcional, dev)

El backend tiene scaffolding de self-observability vía OTLP
(ver [ADR-0007](adr/0007-self-observability.md), superseded by
[ADR-0011](adr/0011-prometheus-self-monitoring.md) para producción). Sigue
disponible como **opt-in** cuando quieres ver trazas+logs+métricas
correlacionadas en la misma UI de Faro durante desarrollo: activá
`FARO_SELF_OBSERVE=true` (ver la sección Self-observability de la
[referencia de variables](reference/environment.md) para endpoint y
service name).

En producción se deja apagado: si la pipeline OTLP se cae, perdemos la
señal de que se cayó. Esa es justamente la razón de monitorear desde afuera.

Mientras no esté activado — o como diagnóstico paralelo — los logs de
contenedor siguen siendo la fuente de verdad:

```bash
ssh infra-iaportafolio
cd /opt/faro
docker compose -f docker-compose.prod.yml logs -f --tail=200 backend
docker compose -f docker-compose.prod.yml logs -f --tail=200 frontend
docker compose -f docker-compose.prod.yml logs -f --tail=200 clickhouse
```

## Endurecimiento mínimo recomendado

- **Auth nativa con bootstrap**: setear `FARO_BOOTSTRAP_ADMIN_EMAIL` y
  `FARO_BOOTSTRAP_ADMIN_PASSWORD` (o copiar el password random generado al
  primer boot) en `.env.prod`. Después exigir 2FA TOTP a los admins desde
  `/settings/security`. Ver [ADR-0009](adr/0009-security-hardening.md).
- **Proxy de red delante del puerto 3000** como defense-in-depth aun con
  auth nativa: TLS termination, WAF, IP allowlist. Cloudflare Access /
  Authelia / Tailscale serve siguen siendo opciones válidas; con auth
  nativa ya no son obligatorias.
- **Firewall**: bloquear `:4318` y `:4317` desde internet salvo para
  clientes autorizados; mantener `:8080` accesible solo si un cliente
  externo consume la API.
- **`/metrics`**: si el endpoint es alcanzable desde fuera de la red
  privada, definir `FARO_METRICS_TOKEN=<bearer>` y configurarlo en el
  scrape de Prometheus.
- **`FARO_ENABLE_HSTS=true`** en producción una vez que el dominio sirve
  HTTPS estable (ADR-0009).
- **Redaction + origin allowlist por proyecto**: activar desde
  `/settings/projects/:slug/redaction` y `.../origins` para reducir PII en
  disco y limitar el blast radius de un token RUM filtrado.
- **Rotar tokens de ingesta por proyecto** desde `/settings/projects/:slug`
  cuando un cliente se desautoriza (no requiere redespliegue).
- **No mezclar** datos sensibles con baja cardinalidad — incluso con
  redaction activa, lo que no matchee las reglas queda almacenado tal cual
  durante el período de retención.

## Atajos del Makefile

El [`Makefile`](../Makefile) del root expone targets para las tareas más
comunes. `make help` lista todos:

```bash
make up              # Levanta el stack completo (dev)
make down            # Detiene (preserva volúmenes)
make reset           # Detiene y BORRA datos (volúmenes incluidos)
make backend         # Corre backend en host contra CH de docker
make backend-test    # cargo nextest (requiere CH arriba)
make backend-check   # cargo fmt + clippy
make frontend        # Dev server en :5173
make frontend-check  # svelte-check + tsc
make ch              # Cliente interactivo de ClickHouse
make migrate         # Aplica migraciones de clickhouse/migrations/
make send-log        # Envía un log de prueba al ingest
make prod-deploy     # Rebuild + restart en prod (si auto-deploy falló)
make prod-logs       # Logs del backend en prod
make release-sdk SDK=node VER=0.3.0  # Tag + push de release de SDK
```

## Imágenes Docker (GHCR)

El workflow `deploy.yml` construye y publica imágenes multi-arch a GitHub
Container Registry bajo `ghcr.io/ia-portafolio/faro`:

- `ghcr.io/ia-portafolio/faro:latest` — tag móvil, último deploy del runner
- `ghcr.io/ia-portafolio/faro:<git-sha>` — tag inmutable por commit

El `docker-compose.prod.yml` usa `build:` (construye local) en lugar de
`image:` — el runner de prod hace el build in-situ. Si preferís pull de GHCR,
cambiá el servicio `backend` a:

```yaml
backend:
  image: ghcr.io/ia-portafolio/faro:latest
  # elimina el bloque `build:`
```

## Compose de test e integración de SDKs

### Backend integration (ClickHouse vivo)

```bash
docker compose -f docker-compose.test.yml -p faro-test up \
  --abort-on-container-exit --exit-code-from backend-test
```

Levanta ClickHouse + backend con `cargo test --tests` (incluye integration).
Ver [AGENTS.md §1.d](../AGENTS.md) para cuándo es obligatorio.

### SDK integration

```bash
docker compose -f docker-compose.sdk-integration.yml up \
  --abort-on-container-exit --exit-code-from sdk-test
```

Levanta backend + CH y corre los SDKs Node/Python/Go contra endpoints reales.
Útil para validar que un cambio en el wire format no rompa ningún SDK.
