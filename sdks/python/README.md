# faro-sdk (Python)

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
