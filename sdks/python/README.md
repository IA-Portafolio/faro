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

## Opciones cross-SDK

`warning()` (alias de `warn()`), `scrub_fields`/`scrub_headers`/`scrub_patterns` y el hook `before_send` están disponibles aquí con la misma semántica que en el resto de SDKs. Ver [API uniforme entre SDKs](../README.md#api-uniforme-entre-sdks).
