# Deployment

Cómo se despliega Faro en producción. La instancia operada está en
<https://faro.iaportafolio.com>. Para CI/CD del repo ver
[`infra/README.md`](../infra/README.md).

## Topología

```
Internet
   ↓ HTTPS (Cloudflare / proxy del host)
faro.iaportafolio.com
   ↓
docker compose (docker-compose.prod.yml)
   ├── faro-frontend     (SvelteKit Node adapter, :3000 interno)
   ├── faro-backend      (Rust, :8080 API + :4318 OTLP)
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
# Edita .env.prod y define CLICKHOUSE_PASSWORD y FARO_INGEST_TOKEN
nano .env.prod
docker compose -f docker-compose.prod.yml --env-file .env.prod up -d --build
```

Verifica:

```bash
curl https://faro.iaportafolio.com/healthz
```

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
5. Polling de `https://faro.iaportafolio.com/healthz` (hasta 40 reintentos).
6. `docker image prune -f`.

## Variables de entorno productivas

`.env.prod` (no se commitea, vive solo en el host). Ver
[`.env.prod.template`](../.env.prod.template) para el formato.

Valores mínimos a setear manualmente:

| Variable                  | Cómo generarlo |
| ------------------------- | -------------- |
| `CLICKHOUSE_PASSWORD`     | `openssl rand -base64 32` |
| `FARO_INGEST_TOKEN`       | `openssl rand -hex 32` |

Variables públicas (URL del dashboard, etc.) ya tienen valores razonables
en el template.

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

El backend tiene scaffolding de **self-observability** vía OTLP
(ver [ADR-0007](adr/0007-self-observability.md)). Está **opt-in** —
apagado por defecto para evitar back-pressure si el listener `:4318` no
responde en frío.

| Variable                       | Default                 | Para qué |
| ------------------------------ | ----------------------- | -------- |
| `FARO_SELF_OBSERVE`            | `false`                 | Activa la emisión OTel del propio backend |
| `FARO_SELF_OBSERVE_ENDPOINT`   | `http://localhost:4318` | Dónde mandar la telemetría (default: nosotros mismos) |
| `OTEL_SERVICE_NAME`            | `faro-backend`          | Nombre del servicio en la telemetría |

Activarlo apunta el exporter al propio listener OTLP local: los logs y
trazas del backend aparecen en el mismo dashboard. Para enviar a un
colector externo (otro Faro, Tempo, etc.) bastará con cambiar
`FARO_SELF_OBSERVE_ENDPOINT`.

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

- **Proxy de autenticación** delante del puerto 3000 si el dashboard se
  expone públicamente (Faro no implementa login propio — ver README →
  Limitaciones).
- **Firewall**: bloquear `:4318` desde internet salvo para clientes
  autorizados; mantener `:8080` accesible solo si un cliente externo
  consume la API.
- **Rotar `FARO_INGEST_TOKEN`** cuando un cliente se desautoriza (requiere
  redespliegue + reconfiguración de todos los emisores).
- **No mezclar** datos sensibles con baja cardinalidad — Faro no tiene
  redacción de PII; lo que envías queda almacenado tal cual durante el
  período de retención.
