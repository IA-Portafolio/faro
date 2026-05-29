"""OTLP tracing setup para el SDK de Python.

Inicializa `opentelemetry.sdk.trace.TracerProvider` con un `BatchSpanProcessor`
+ un exporter OTLP/HTTP/JSON propio (el oficial de OTel Python sólo viene en
protobuf y nuestro backend Faro acepta sólo JSON en `/v1/traces`). Después
intenta instrumentar las librerías comunes (requests, urllib3, httpx, psycopg,
pymongo, redis, sqlalchemy, aiohttp, celery, starlette) que estén instaladas
para que el Service Map y la pestaña Trazas se llenen sin instrumentación manual.

Diseño:
    - Singleton: una sola inicialización por proceso; las llamadas siguientes son no-op.
    - El TracerProvider se guarda a nivel de módulo para poder llamar a
      `force_flush()` desde `flush_tracing()` mid-lifetime — esto es necesario
      para `faro.flush()` y para los tests, que no pueden esperar al scheduled
      export del BatchSpanProcessor (5s por default).
    - `instrumentations_registered` evita re-instrumentar tras un re-init (no
      todas las instrumentaciones son idempotentes).
    - Exporter custom (`FaroJsonSpanExporter`) — convierte ReadableSpan a
      OTLP/JSON ExportTraceServiceRequest y postea con bearer auth.
"""

from __future__ import annotations

import sys
from typing import Any, Callable, Sequence

# Tracer público que devolvemos cuando OTel no está inicializado. Lo expone
# get_tracer(). El no-op de @opentelemetry/api hace que startSpan devuelva un
# Span no-op (trace_id=000...000) que no rompe el código del usuario.
_TRACER_NAME = "faro_sdk"

_provider: Any = None  # opentelemetry.sdk.trace.TracerProvider | None
_cached_tracer: Any = None  # opentelemetry.trace.Tracer | None
_instrumentations_registered = False


# Lista de (módulo_python, clase_instrumentor). El SDK intenta importar cada
# uno; si no está instalado, lo salta. Si el user instala uno nuevo después,
# vuelve a llamar a init_tracing() o reinicia para que sea descubierto.
_INSTRUMENTATIONS: tuple[tuple[str, str, str], ...] = (
    ("requests", "opentelemetry.instrumentation.requests", "RequestsInstrumentor"),
    ("urllib3", "opentelemetry.instrumentation.urllib3", "URLLib3Instrumentor"),
    ("httpx", "opentelemetry.instrumentation.httpx", "HTTPXClientInstrumentor"),
    ("psycopg2", "opentelemetry.instrumentation.psycopg2", "Psycopg2Instrumentor"),
    ("psycopg", "opentelemetry.instrumentation.psycopg", "PsycopgInstrumentor"),
    ("pymongo", "opentelemetry.instrumentation.pymongo", "PymongoInstrumentor"),
    ("redis", "opentelemetry.instrumentation.redis", "RedisInstrumentor"),
    ("sqlalchemy", "opentelemetry.instrumentation.sqlalchemy", "SQLAlchemyInstrumentor"),
    ("aiohttp_client", "opentelemetry.instrumentation.aiohttp_client", "AioHttpClientInstrumentor"),
    ("celery", "opentelemetry.instrumentation.celery", "CeleryInstrumentor"),
    ("starlette", "opentelemetry.instrumentation.starlette", "StarletteInstrumentor"),
    # FastAPI/Flask/Django requieren `app` o `settings` y por eso no se
    # auto-instrumentan acá — el usuario llama p.ej. `FastAPIInstrumentor.instrument_app(app)`.
)


def init_tracing(
    endpoint: str,
    token: str,
    service: str,
    *,
    traces_endpoint: str | None = None,
    environment: str | None = None,
    release: str | None = None,
    resource_attributes: dict[str, str] | None = None,
    disabled_instrumentations: tuple[str, ...] | list[str] | None = None,
    diag: Callable[[str, Exception | None], None] | None = None,
) -> bool:
    """Inicializa el TracerProvider de OTel apuntando a Faro. Idempotente."""
    global _provider, _instrumentations_registered
    if _provider is not None:
        return False
    if not endpoint or not token or not service:
        return False

    def _diag(msg: str, err: Exception | None = None) -> None:
        if diag is not None:
            diag(msg, err)
        else:
            extra = f": {err}" if err else ""
            sys.stderr.write(f"[faro/tracing] {msg}{extra}\n")

    try:
        from opentelemetry import trace
        from opentelemetry.sdk.resources import Resource
        from opentelemetry.sdk.trace import TracerProvider
        from opentelemetry.sdk.trace.export import BatchSpanProcessor
    except ImportError as e:
        _diag("OTel no está instalado — corré `pip install faro-sdk[tracing]`", e)
        return False

    base = endpoint.rstrip("/")
    url = traces_endpoint or f"{base}/v1/traces"

    attrs: dict[str, str] = {"service.name": service}
    if release:
        attrs["service.version"] = release
    if environment:
        # OTel 1.27+ usa deployment.environment.name; emitimos también el viejo
        # `deployment.environment` para compat con el backend Faro.
        attrs["deployment.environment.name"] = environment
        attrs["deployment.environment"] = environment
    if resource_attributes:
        for k, v in resource_attributes.items():
            attrs[k] = str(v)

    try:
        provider = TracerProvider(resource=Resource.create(attrs))
        exporter = FaroJsonSpanExporter(
            endpoint=url,
            headers={"Authorization": f"Bearer {token}"},
        )
        provider.add_span_processor(BatchSpanProcessor(exporter))
        trace.set_tracer_provider(provider)
    except Exception as e:
        _diag("init_tracing falló creando el provider", e)
        return False

    _provider = provider

    if not _instrumentations_registered:
        disabled = set(disabled_instrumentations or ())
        for short_name, module_name, class_name in _INSTRUMENTATIONS:
            if short_name in disabled:
                continue
            try:
                mod = __import__(module_name, fromlist=[class_name])
                instrumentor_cls = getattr(mod, class_name)
                instrumentor_cls().instrument()
            except ImportError:
                # paquete no instalado — saltar silencio
                continue
            except Exception as e:
                _diag(f"falló instrumentar {short_name}", e)
        _instrumentations_registered = True

    return True


def flush_tracing(timeout_ms: int = 5000) -> None:
    """Drena los spans pending del BatchSpanProcessor sin apagar el provider.
    Usado por `faro.flush()` y por los tests."""
    if _provider is None:
        return
    try:
        _provider.force_flush(timeout_ms)
    except Exception:
        # best-effort
        pass


def shutdown_tracing(timeout_ms: int = 5000) -> None:
    """Drena pending spans y apaga el provider.

    CRÍTICO: opentelemetry-api se NIEGA a re-registrar el global tracer provider
    (warning: "Overriding of current TracerProvider is not allowed") — es un
    Once() interno. Sin resetearlo a mano, una segunda llamada a init_tracing()
    con un endpoint distinto crea el provider pero `set_tracer_provider` es
    no-op y los spans siguen yendo al destino viejo. Esto rompe tests con
    init/close repetidos y un eventual hot-reload en dev. Tocamos privados
    porque OTel Python no expone una API pública para esto en 1.42."""
    global _provider, _cached_tracer
    current = _provider
    if current is None:
        return
    _provider = None
    _cached_tracer = None
    try:
        current.shutdown()
    except Exception:
        # best-effort
        pass
    # Reset del Once() interno + el provider global. Sin esto, `set_tracer_provider`
    # logueará "Overriding…" y dejará el provider viejo (ya cerrado) en su lugar.
    try:
        from opentelemetry import trace as _trace
        _trace._TRACER_PROVIDER = None  # type: ignore[attr-defined]
        once = getattr(_trace, "_TRACER_PROVIDER_SET_ONCE", None)
        if once is not None:
            # Once tiene un _done bool (o un Lock) — distintas versiones de OTel.
            for attr in ("_done", "done"):
                if hasattr(once, attr):
                    setattr(once, attr, False)
                    break
    except Exception:
        pass


def get_tracer() -> Any:
    """Devuelve un Tracer con el nombre del SDK. Si OTel no está inicializado,
    devuelve un tracer no-op del provider global."""
    global _cached_tracer
    if _cached_tracer is not None:
        return _cached_tracer
    try:
        from opentelemetry import trace
        _cached_tracer = trace.get_tracer(_TRACER_NAME)
    except ImportError:
        return _NoopTracer()
    return _cached_tracer


def get_current_otel_span() -> Any:
    """Devuelve el span activo de OTel o None. Lo usa el SDK para auto-correlación
    de logs con trace_id/span_id."""
    try:
        from opentelemetry import trace
        span = trace.get_current_span()
        if span is None:
            return None
        sc = span.get_span_context()
        if not getattr(sc, "is_valid", False):
            return None
        return span
    except Exception:
        return None


class _NoopSpan:
    """Span no-op para cuando OTel no está disponible. Mantiene la API del Span
    público pero no emite nada y devuelve trace_id/span_id todo ceros."""

    def get_span_context(self):
        class _SC:
            trace_id = 0
            span_id = 0
            is_valid = False
            trace_flags = 0
        return _SC()

    def set_attribute(self, *args, **kwargs):
        pass

    def set_attributes(self, *args, **kwargs):
        pass

    def add_event(self, *args, **kwargs):
        pass

    def set_status(self, *args, **kwargs):
        pass

    def record_exception(self, *args, **kwargs):
        pass

    def end(self, *args, **kwargs):
        pass

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False


class _NoopTracer:
    """Tracer no-op para cuando OTel no está instalado."""

    def start_span(self, *args, **kwargs):
        return _NoopSpan()

    def start_as_current_span(self, *args, **kwargs):
        return _NoopSpan()


def _reset_for_tests() -> None:
    """Solo para tests — limpia los singletons para que init_tracing pueda volver a correr."""
    global _provider, _cached_tracer, _instrumentations_registered
    _provider = None
    _cached_tracer = None
    # NO reseteamos _instrumentations_registered — las instrumentaciones ya
    # están patched globalmente y no se pueden desinstrumentar idempotentemente.


# ---------- Custom OTLP/HTTP/JSON Span Exporter ----------
#
# OTel Python solo provee `opentelemetry-exporter-otlp-proto-http` (protobuf).
# Nuestro backend Faro acepta OTLP/HTTP/JSON en /v1/traces. Este exporter
# convierte ReadableSpan a la wire format JSON definida en el protocol y la
# postea con bearer auth. ~120 líneas vs traer 50MB de deps de protobuf.

try:
    # Importes inline para no romper si OTel SDK no está disponible.
    from opentelemetry.sdk.trace.export import SpanExporter as _BaseSpanExporter
    from opentelemetry.sdk.trace.export import SpanExportResult as _SpanExportResult
    _SPAN_EXPORTER_BASE = _BaseSpanExporter
    _EXPORT_OK = _SpanExportResult.SUCCESS
    _EXPORT_FAIL = _SpanExportResult.FAILURE
except ImportError:  # pragma: no cover
    _SPAN_EXPORTER_BASE = object  # fallback para que el módulo importe sin SDK
    _EXPORT_OK = True
    _EXPORT_FAIL = False


class FaroJsonSpanExporter(_SPAN_EXPORTER_BASE):  # type: ignore[misc,valid-type]
    """Exporter OTLP/HTTP/JSON apuntado al backend de Faro.

    El BatchSpanProcessor llama a `export(spans)` con un batch de ReadableSpan
    cuando se llena el buffer o cuando se hace force_flush(). Devolvemos
    SUCCESS si el server respondió 2xx, FAILURE si 5xx/red caída → OTel
    reintenta automáticamente con backoff.
    """

    def __init__(
        self,
        endpoint: str,
        headers: dict[str, str] | None = None,
        timeout: float = 10.0,
    ) -> None:
        import requests  # import lazy para no afectar el import del paquete
        self._endpoint = endpoint
        self._headers = {**(headers or {}), "Content-Type": "application/json"}
        self._timeout = timeout
        self._session = requests.Session()
        self._closed = False

    def export(self, spans: Sequence[Any]) -> Any:
        if self._closed or not spans:
            return _EXPORT_OK
        try:
            payload = self._build_payload(spans)
        except Exception as e:
            sys.stderr.write(f"[faro/tracing] error serializando spans: {e}\n")
            return _EXPORT_FAIL
        try:
            r = self._session.post(
                self._endpoint, json=payload, headers=self._headers, timeout=self._timeout,
            )
            if r.status_code >= 500:
                sys.stderr.write(f"[faro/tracing] traces HTTP {r.status_code}\n")
                return _EXPORT_FAIL
            if r.status_code >= 400:
                # 4xx no se reintenta (auth inválida / batch malformado). Logueamos y descartamos.
                sys.stderr.write(f"[faro/tracing] traces HTTP {r.status_code}: {r.text[:200]}\n")
                return _EXPORT_OK
            return _EXPORT_OK
        except Exception as e:
            sys.stderr.write(f"[faro/tracing] export falló: {e}\n")
            return _EXPORT_FAIL

    def force_flush(self, timeout_millis: int = 30000) -> bool:
        # El exporter es sync; no hay nada que drenar dentro nuestro.
        return True

    def shutdown(self) -> None:
        if self._closed:
            return
        self._closed = True
        try:
            self._session.close()
        except Exception:
            pass

    # ---- Serialización OTLP/JSON ----

    def _build_payload(self, spans: Sequence[Any]) -> dict[str, Any]:
        """Agrupa spans por (resource, instrumentation_scope) según la spec OTLP."""
        # OTel garantiza que todos los spans de un batch típicamente comparten el
        # mismo Resource. Aún así, agrupamos por id(resource) para ser correctos.
        by_resource: dict[int, tuple[Any, dict[tuple[str, str], list[Any]]]] = {}
        for sp in spans:
            res = sp.resource
            rid = id(res)
            if rid not in by_resource:
                by_resource[rid] = (res, {})
            scope = sp.instrumentation_scope
            scope_key = (scope.name or "", scope.version or "")
            by_resource[rid][1].setdefault(scope_key, []).append(sp)

        resource_spans = []
        for res, scopes_map in by_resource.values():
            scope_spans = []
            for (scope_name, scope_version), spans_in_scope in scopes_map.items():
                scope_spans.append({
                    "scope": {"name": scope_name, **({"version": scope_version} if scope_version else {})},
                    "spans": [self._span_to_json(s) for s in spans_in_scope],
                })
            resource_spans.append({
                "resource": {"attributes": self._attrs_to_kvs(res.attributes)},
                "scopeSpans": scope_spans,
            })
        return {"resourceSpans": resource_spans}

    def _span_to_json(self, span: Any) -> dict[str, Any]:
        sc = span.get_span_context()
        # OTLP/JSON SpanKind: 1=INTERNAL, 2=SERVER, 3=CLIENT, 4=PRODUCER, 5=CONSUMER.
        # OTel Python's SpanKind enum: INTERNAL=0, SERVER=1, … → sumamos 1 para llevar
        # a la enumeración OTLP wire (donde 0 está reservado para UNSPECIFIED).
        out: dict[str, Any] = {
            "traceId": format(sc.trace_id, "032x"),
            "spanId": format(sc.span_id, "016x"),
            "name": span.name,
            "kind": int(span.kind.value) + 1,
            "startTimeUnixNano": str(span.start_time),
            "endTimeUnixNano": str(span.end_time),
            "attributes": self._attrs_to_kvs(span.attributes or {}),
        }
        parent = getattr(span, "parent", None)
        if parent is not None and parent.span_id:
            out["parentSpanId"] = format(parent.span_id, "016x")
        if span.events:
            out["events"] = [
                {
                    "timeUnixNano": str(e.timestamp),
                    "name": e.name,
                    "attributes": self._attrs_to_kvs(e.attributes or {}),
                }
                for e in span.events
            ]
        status = getattr(span, "status", None)
        if status is not None and status.status_code.value != 0:  # 0 = UNSET
            sout: dict[str, Any] = {"code": int(status.status_code.value)}
            if status.description:
                sout["message"] = status.description
            out["status"] = sout
        return out

    def _attrs_to_kvs(self, attrs: Any) -> list[dict[str, Any]]:
        out: list[dict[str, Any]] = []
        for k, v in (attrs or {}).items():
            value = self._wrap_value(v)
            if value is not None:
                out.append({"key": str(k), "value": value})
        return out

    def _wrap_value(self, v: Any) -> dict[str, Any] | None:
        if isinstance(v, bool):
            return {"boolValue": v}
        if isinstance(v, int):
            return {"intValue": str(v)}  # OTLP/JSON: int64 como string
        if isinstance(v, float):
            return {"doubleValue": v}
        if isinstance(v, str):
            return {"stringValue": v}
        if v is None:
            return {"stringValue": ""}
        if isinstance(v, (list, tuple)):
            return {"arrayValue": {"values": [self._wrap_value(x) for x in v if x is not None]}}
        return {"stringValue": str(v)}
