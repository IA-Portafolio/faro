# faro-sdk (Python)

> **Perfil de defaults:** `server` — flush 750ms · batch 200 · queue 10 000. Ver [perfiles](../README.md#perfiles-de-defaults).

```bash
pip install faro-sdk
```

```python
import faro_sdk as faro

faro.init(
    endpoint="https://faro.iaportafolio.com",
    token="...",                       # visible en /projects → SDK
    service="ingesta-csv",
    environment="production",
    release="git-sha-abc123",
    attributes={"region": "eu-west-1"},
)

faro.info("arranque ok", port=8080)

try:
    procesar(archivo)
except Exception as exc:
    faro.capture_exception(exc, tags={"archivo": archivo.name})
    raise
```

## Integración con `logging`

```python
import logging
from faro_sdk import FaroHandler

logging.basicConfig(level=logging.INFO)
logging.getLogger().addHandler(FaroHandler())

logging.info("auto-enviado a Faro")
try:
    raise ValueError("boom")
except Exception:
    logging.exception("falló el job")   # incluye stack trace
```

## Captura automática

`init()` instala `sys.excepthook` y `threading.excepthook` (Python 3.8+). Cualquier excepción no manejada se envía a Faro antes de imprimirla en stderr.

Para desactivar: `faro.init(..., install_global_handlers=False)`.

## Flush / cierre

```python
faro.flush(timeout=3.0)
# o, al cerrar la app:
faro.close()
```

`atexit` ya registra un cierre limpio al terminar el proceso, pero para scripts cortos llama explícitamente para no perder eventos.

## Auto-correlación con traces

`track()` adjunta `trace_id`/`span_id` si OpenTelemetry está instalado y hay un span activo, o si pasas un provider explícito. El provider puede devolver un header W3C `traceparent` o un dict con `trace_id`/`span_id`:

```python
faro.init(
    endpoint="https://faro.iaportafolio.com",
    token="...",
    service="checkout",
    trace_context=lambda: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
)

faro.track("checkout_completed")  # incluye trace_id + span_id
```

## Tracing (OpenTelemetry)

El SDK incluye auto-instrumentación OTel: si tenés `opentelemetry-sdk` +
`opentelemetry-instrumentation-*` instalados, el SDK detecta automáticamente
`requests`, `urllib3`, `httpx`, `psycopg2`/`psycopg`, `pymongo`, `redis`,
`sqlalchemy`, `aiohttp`, `celery`, `starlette` y crea spans por vos. Los spans
se envían por OTLP/HTTP/JSON al endpoint de Faro.

```python
import faro_sdk as faro

faro.init(
    endpoint="https://faro.iaportafolio.com",
    token="...",
    service="checkout-api",
    environment="production",
    release="git-sha-abc123",
)

# Inicializa el TracerProvider de OTel (idempotente)
faro.init_tracing(
    endpoint="https://faro.iaportafolio.com",
    token="...",
    service="checkout-api",
)
```

Spans manuales:

```python
# Context manager (cierra solo)
with faro.use_span("db-query") as span:
    span.set_attribute("db.system", "postgresql")
    db.query("SELECT 1")

# Manual
span = faro.start_span("procesar-pago")
try:
    charge(order)
    span.set_status("OK")
except Exception as exc:
    span.set_status("ERROR", str(exc))
    raise
finally:
    span.end()
```

API disponible: `start_span(name, ...)`, `use_span(name, ...)`, `active_span()`,
`init_tracing(...)`, `shutdown_tracing()`, `flush_tracing()`, `get_tracer()`,
`Span`, `SpanKind`, `SpanStatus`.

## Middlewares WSGI / ASGI

El SDK incluye middlewares que abren un span SERVER por request para los
frameworks web comunes:

```python
# Flask / Django (WSGI)
from flask import Flask
from faro_sdk.middleware import FaroWsgiMiddleware

faro.init(endpoint=..., token=..., service="mi-app")
app = Flask(__name__)
app.wsgi_app = FaroWsgiMiddleware(app.wsgi_app)

# FastAPI / Starlette (ASGI)
from fastapi import FastAPI
from faro_sdk.middleware import FaroAsgiMiddleware

app = FastAPI()
app.add_middleware(FaroAsgiMiddleware)
```

Los middlewares respetan el `traceparent` entrante y propagan el del span actual
en la respuesta.

## Product analytics

```python
# Eventos de producto
faro.track("checkout_completed", amount=99.50, currency="USD")

# Identificar usuario
faro.identify("user_42", email="a@b.com", plan="pro")

# Fusionar sesión anónima con usuario post-login
faro.alias("anon_abc123", "user_42")
```

Ver [API uniforme](../README.md#api-uniforme-entre-sdks) para la semántica de
`anonymous_id`/`distinct_id`/`session_id`.

## Opciones de init

| Opción | Default | Descripción |
| ------ | ------- | ----------- |
| `enable_tracing` | `True` | Activa auto-instrumentación OTel al llamar `init()`. |
| `traces_endpoint` | `${endpoint}/v1/traces` | Override del path de traces. |
| `resource_attributes` | `{}` | Atributos extra del Resource OTel. |
| `disabled_instrumentations` | `[]` | Lista de instrumentores a desactivar (p. ej. `["requests", "urllib3"]`). |
| `flush_interval` | `2.0` | Cadencia de flush (segundos). |
| `max_queue_size` | `10000` | Cap de la cola. |
| `batch_size` | `100` | Eventos por POST. |

```python
faro.init(
    endpoint="https://faro.iaportafolio.com",
    token="...",
    service="api",
    enable_tracing=False,          # tracing off — sólo logs/events
    disabled_instrumentations=["urllib3"],
    resource_attributes={"region": "us-east-1"},
)
```

## Opciones cross-SDK

`warning()` (alias de `warn()`), `scrub_fields`/`scrub_headers`/`scrub_patterns` y el hook `before_send` están disponibles aquí con la misma semántica que en el resto de SDKs. Ver [API uniforme entre SDKs](../README.md#api-uniforme-entre-sdks).
