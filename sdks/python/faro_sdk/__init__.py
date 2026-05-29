"""SDK de Faro para Python.

Uso:

    import faro_sdk as faro
    faro.init(endpoint="https://faro.iaportafolio.com", token="...", service="mi-app")
    faro.info("servidor arrancado", port=8080)

    try:
        do_work()
    except Exception as exc:
        faro.capture_exception(exc, tags={"job": "nightly"})
        raise

v0.2.0: el tracing pasa a estar respaldado por OpenTelemetry. La API pública
(`start_span` / `use_span` / `active_span` / `Span`) se conserva, pero por
dentro envuelve `opentelemetry.trace.Span`. Esto desbloquea auto-instrumentación
de requests/urllib3/httpx/psycopg/pymongo/redis/sqlalchemy/aiohttp/celery/starlette
si esos paquetes están instalados — Service Map y la pestaña Trazas en el
dashboard se llenan sin instrumentar manualmente.
"""

from __future__ import annotations

import atexit
import json
import logging
import os
import queue
import re
import sys
import threading
import time
import traceback
from contextlib import contextmanager
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, Callable, Iterable, Iterator

import requests

from ._tracing import (
    flush_tracing as _flush_tracing,
    get_current_otel_span as _get_current_otel_span,
    get_tracer as _get_tracer,
    init_tracing as _init_tracing,
    shutdown_tracing as _shutdown_tracing,
)

__all__ = [
    "init",
    "log",
    "info",
    "warn",
    "warning",
    "error",
    "capture_exception",
    "track",
    "identify",
    "alias",
    "start_span",
    "use_span",
    "active_span",
    "Span",
    "SpanKind",
    "SpanStatus",
    "flush",
    "close",
    "FaroHandler",
    # OTel-passthrough
    "init_tracing",
    "shutdown_tracing",
    "flush_tracing",
    "get_tracer",
]

_SEVERITIES = {"TRACE", "DEBUG", "INFO", "WARN", "ERROR", "FATAL"}

# --- Scrubbing (defensa en profundidad antes de enviar) ---
_DEFAULT_SCRUB_FIELDS = (
    "password", "token", "secret", "authorization", "cookie", "set-cookie", "api_key", "apikey",
)
_HEADER_SCRUB_FIELDS = ("authorization", "cookie", "set-cookie")
_REDACTED = "[REDACTED]"

_SCRUB_REGEXES: dict[str, re.Pattern[str]] = {
    "email": re.compile(r"[\w.+-]+@[\w-]+(?:\.[\w-]+)+"),
    "jwt": re.compile(r"\beyJ[\w-]+\.[\w-]+\.[\w-]+\b"),
    # Sin Luhn; puede tener falsos positivos en IDs largos. Opt-in.
    "credit-card": re.compile(r"\b(?:\d[ -]?){13,19}\b"),
    "api-key": re.compile(
        r"\b(?:sk-|ghp_|ghs_|gho_|github_pat_|xoxb-|xoxp-|xoxs-|AKIA|ASIA|AIza)[\w-]{12,}\b"
    ),
}

_TRACEPARENT_RE = re.compile(
    r"^[\da-fA-F]{2}-([\da-fA-F]{32})-([\da-fA-F]{16})-[\da-fA-F]{2}(?:-.+)?$"
)

TraceContextProvider = Callable[[], dict[str, Any] | str | None]

# --- Tracing primitives (mapeo Faro→OTel) ---

# OTLP SpanKind por nombre.
SpanKind = str  # "INTERNAL" | "SERVER" | "CLIENT" | "PRODUCER" | "CONSUMER"

# OTLP StatusCode por nombre.
SpanStatus = str  # "UNSET" | "OK" | "ERROR"

# OTel mappings — los importamos lazy para no romper si OTel SDK no se instaló.
def _otel_span_kind(name: SpanKind) -> Any:
    try:
        from opentelemetry.trace import SpanKind as OtelSpanKind
        return {
            "INTERNAL": OtelSpanKind.INTERNAL,
            "SERVER": OtelSpanKind.SERVER,
            "CLIENT": OtelSpanKind.CLIENT,
            "PRODUCER": OtelSpanKind.PRODUCER,
            "CONSUMER": OtelSpanKind.CONSUMER,
        }.get(name, OtelSpanKind.INTERNAL)
    except ImportError:
        return None


def _otel_status_code(name: SpanStatus) -> Any:
    try:
        from opentelemetry.trace import StatusCode
        return {"UNSET": StatusCode.UNSET, "OK": StatusCode.OK, "ERROR": StatusCode.ERROR}.get(
            name, StatusCode.UNSET
        )
    except ImportError:
        return None


def _stringify_attr(v: Any) -> str:
    if isinstance(v, str):
        return v
    if v is None:
        return ""
    if isinstance(v, (int, float, bool)):
        return str(v)
    try:
        return json.dumps(v, default=str)
    except Exception:
        return str(v)


class Span:
    """Span público respaldado por `opentelemetry.trace.Span`.

    La API es la misma que en v0.1.x (set_attribute / add_event / set_status /
    record_exception / end / traceparent / span_context) — el cambio interno
    es que ahora la exportación, batching, y propagación de contexto las maneja
    OTel, lo que habilita correlación con spans auto-instrumentados.
    """

    def __init__(self, otel_span: Any, *, ctx_token: Any = None) -> None:
        self._otel = otel_span
        self._ctx_token = ctx_token  # token de attach() para detach en end()
        self._ended = False

    @property
    def trace_id(self) -> str:
        sc = self._otel.get_span_context()
        return format(sc.trace_id, "032x")

    @property
    def span_id(self) -> str:
        sc = self._otel.get_span_context()
        return format(sc.span_id, "016x")

    @property
    def ended(self) -> bool:
        return self._ended

    def span_context(self) -> dict[str, str]:
        return {"trace_id": self.trace_id, "span_id": self.span_id}

    def traceparent(self) -> str:
        sc = self._otel.get_span_context()
        flags = "01" if (sc.trace_flags & 1) else "00"
        return f"00-{format(sc.trace_id, '032x')}-{format(sc.span_id, '016x')}-{flags}"

    def set_attribute(self, key: str, value: Any) -> None:
        if self._ended:
            return
        try:
            self._otel.set_attribute(key, _stringify_attr(value))
        except Exception:
            pass

    def set_attributes(self, attrs: dict[str, Any]) -> None:
        if self._ended:
            return
        for k, v in attrs.items():
            self.set_attribute(k, v)

    def add_event(
        self,
        name: str,
        attributes: dict[str, Any] | None = None,
        timestamp: datetime | None = None,
    ) -> None:
        if self._ended:
            return
        attrs: dict[str, str] = {}
        if attributes:
            for k, v in attributes.items():
                attrs[k] = _stringify_attr(v)
        try:
            ts = int(timestamp.timestamp() * 1e9) if timestamp else None
            self._otel.add_event(name, attributes=attrs, timestamp=ts)
        except Exception:
            pass

    def set_status(self, code: SpanStatus, message: str = "") -> None:
        if self._ended:
            return
        try:
            from opentelemetry.trace import Status
            otel_code = _otel_status_code(code)
            if otel_code is None:
                return
            self._otel.set_status(Status(otel_code, description=message or None))
        except Exception:
            pass

    def record_exception(self, exc: BaseException) -> None:
        # Set status manually first to match cross-SDK behavior, then add exception attrs.
        self.set_status("ERROR", str(exc))
        self.set_attribute("exception.type", type(exc).__name__)
        self.set_attribute("exception.message", str(exc))
        self.set_attribute(
            "exception.stacktrace",
            "".join(traceback.format_exception(type(exc), exc, exc.__traceback__)),
        )
        # OTel también tiene record_exception(); lo llamamos por las dudas, pero
        # los attrs custom de arriba son los que aseguran la wire shape.
        try:
            self._otel.record_exception(exc)
        except Exception:
            pass

    def end(self, end_time: datetime | None = None) -> None:
        if self._ended:
            return
        self._ended = True
        try:
            ts = int(end_time.timestamp() * 1e9) if end_time else None
            self._otel.end(end_time=ts)
        except Exception:
            pass
        # Si el span fue activado vía use_span(), liberamos el token del contextvar.
        if self._ctx_token is not None:
            try:
                from opentelemetry import context
                context.detach(self._ctx_token)
            except Exception:
                pass
            self._ctx_token = None


def _scrub_string(s: str, regexes: list[re.Pattern[str]]) -> str:
    for rx in regexes:
        s = rx.sub(_REDACTED, s)
    return s


def _scrub_entry(entry: dict[str, Any], needles: list[str], regexes: list[re.Pattern[str]]) -> None:
    attrs = entry.get("attributes") or {}
    for k in list(attrs.keys()):
        k_lower = k.lower()
        if any(n in k_lower for n in needles):
            attrs[k] = _REDACTED
        elif regexes and isinstance(attrs[k], str):
            attrs[k] = _scrub_string(attrs[k], regexes)
    if regexes and isinstance(entry.get("message"), str):
        entry["message"] = _scrub_string(entry["message"], regexes)


def _parse_traceparent(traceparent: str) -> dict[str, str] | None:
    match = _TRACEPARENT_RE.match(traceparent.strip())
    if not match:
        return None
    trace_id = match.group(1).lower()
    span_id = match.group(2).lower()
    if set(trace_id) == {"0"} or set(span_id) == {"0"}:
        return None
    return {"trace_id": trace_id, "span_id": span_id}


def _normalize_hex(value: Any, length: int) -> str | None:
    if not isinstance(value, str):
        return None
    value = value.strip().lower()
    if len(value) != length or not re.fullmatch(r"[\da-f]+", value) or set(value) == {"0"}:
        return None
    return value


def _normalize_trace_context(value: dict[str, Any] | str | None) -> dict[str, str] | None:
    if value is None:
        return None
    if isinstance(value, str):
        return _parse_traceparent(value)
    if not isinstance(value, dict):
        return None
    traceparent = value.get("traceparent")
    if isinstance(traceparent, str):
        parsed = _parse_traceparent(traceparent)
        if parsed:
            return parsed
    trace_id = _normalize_hex(value.get("trace_id"), 32)
    span_id = _normalize_hex(value.get("span_id"), 16)
    if not trace_id:
        return None
    out = {"trace_id": trace_id}
    if span_id:
        out["span_id"] = span_id
    return out


def _otel_trace_context_from_active_span() -> dict[str, str] | None:
    span = _get_current_otel_span()
    if span is None:
        return None
    sc = span.get_span_context()
    return {
        "trace_id": format(sc.trace_id, "032x"),
        "span_id": format(sc.span_id, "016x"),
    }


def _make_parent_context(parent: Any) -> Any:
    """Resuelve el `parent=` de start_span/use_span a un Context OTel.

    - `parent is ...` (default): None → hereda el current active context.
    - `parent is None`: fuerza root (Context vacío).
    - `parent` Span/dict/str: lo desenvolvemos al SpanContext + lo ponemos en Context.
    """
    if parent is ...:
        return None  # OTel toma el current active si no pasamos context
    try:
        from opentelemetry import context, trace
        from opentelemetry.trace import NonRecordingSpan, SpanContext, TraceFlags
    except ImportError:
        return None
    if parent is None:
        # Context vacío → root span
        return context.Context()
    if isinstance(parent, Span):
        sc = parent._otel.get_span_context()
        return trace.set_span_in_context(NonRecordingSpan(sc), context.Context())
    if isinstance(parent, str):
        tc = _parse_traceparent(parent)
    elif isinstance(parent, dict):
        tc = _normalize_trace_context(parent)
    else:
        tc = None
    if not tc or not tc.get("trace_id") or not tc.get("span_id"):
        return context.Context()
    sc = SpanContext(
        trace_id=int(tc["trace_id"], 16),
        span_id=int(tc["span_id"], 16),
        is_remote=True,
        trace_flags=TraceFlags(TraceFlags.SAMPLED),
    )
    return trace.set_span_in_context(NonRecordingSpan(sc), context.Context())


@dataclass
class _Options:
    endpoint: str
    token: str
    service: str
    environment: str | None = None
    release: str | None = None
    attributes: dict[str, Any] = field(default_factory=dict)
    # Perfil de defaults: "server" (sdks/README.md → Perfiles de defaults).
    flush_interval_s: float = 0.75
    max_batch_size: int = 200
    max_queue_size: int = 10_000
    install_global_handlers: bool = True
    timeout: float = 5.0
    # Scrubbing + beforeSend (ver sdks/README.md → Privacidad / hooks).
    scrub_fields: tuple[str, ...] = _DEFAULT_SCRUB_FIELDS
    scrub_headers: bool = True
    scrub_patterns: tuple[str, ...] = ("jwt", "api-key")
    before_send: Callable[[dict[str, Any]], dict[str, Any] | None] | None = None
    trace_context: TraceContextProvider | None = None
    # OTel tracing (v0.2.0+).
    enable_tracing: bool = True
    traces_endpoint: str | None = None
    resource_attributes: dict[str, str] | None = None
    disabled_instrumentations: tuple[str, ...] = ()


class _Client:
    def __init__(self, opts: _Options) -> None:
        self.opts = opts
        self.opts.endpoint = self.opts.endpoint.rstrip("/")
        needles = {f.lower() for f in opts.scrub_fields}
        if opts.scrub_headers:
            needles.update(_HEADER_SCRUB_FIELDS)
        self._scrub_needles: list[str] = sorted(needles)
        self._scrub_regexes: list[re.Pattern[str]] = [
            _SCRUB_REGEXES[p] for p in opts.scrub_patterns if p in _SCRUB_REGEXES
        ]
        self._queue: queue.Queue[dict[str, Any]] = queue.Queue(maxsize=opts.max_queue_size)
        self._events_queue: queue.Queue[dict[str, Any]] = queue.Queue(maxsize=opts.max_queue_size)
        # Estado de identidad para los product events.
        self._distinct_id: str = ""
        self._anonymous_id: str = f"anon_{os.urandom(8).hex()}"
        self._user_properties: dict[str, Any] = {}
        self._closed = threading.Event()
        self._session = requests.Session()

        # OTel tracing bootstrap. Es idempotente: si fue inicializado vía
        # `opentelemetry-instrument` o vía init_tracing() previo, esto es no-op.
        if opts.enable_tracing:
            _init_tracing(
                endpoint=opts.endpoint,
                token=opts.token,
                service=opts.service,
                traces_endpoint=opts.traces_endpoint,
                environment=opts.environment,
                release=opts.release,
                resource_attributes=opts.resource_attributes,
                disabled_instrumentations=opts.disabled_instrumentations,
            )

        self._worker = threading.Thread(target=self._run, daemon=True, name="faro-flush")
        self._worker.start()
        self._events_worker = threading.Thread(
            target=self._run_events, daemon=True, name="faro-flush-events"
        )
        self._events_worker.start()
        if opts.install_global_handlers:
            self._install_handlers()
        atexit.register(self.close)

    # ---------- API pública ----------

    def log(
        self,
        level: str = "INFO",
        message: str = "",
        attributes: dict[str, Any] | None = None,
        trace_id: str | None = None,
        span_id: str | None = None,
    ) -> None:
        if self._closed.is_set():
            return
        lvl = level.upper()
        if lvl not in _SEVERITIES:
            lvl = "INFO"
        attrs: dict[str, str] = {}
        for k, v in (self.opts.attributes or {}).items():
            attrs[k] = str(v)
        if self.opts.environment:
            attrs["deployment.environment"] = self.opts.environment
        if self.opts.release:
            attrs["service.version"] = self.opts.release
        if attributes:
            for k, v in attributes.items():
                attrs[k] = v if isinstance(v, str) else json.dumps(v, default=str)
        entry: dict[str, Any] = {
            "level": lvl,
            "message": message,
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "attributes": attrs,
        }
        # Auto-correlación: si el caller no pasó trace_id, leemos el span activo
        # del context manager de OTel. Esto cubre tanto spans Faro (vía use_span)
        # como cualquier span auto-instrumentado (requests, psycopg, fastapi, …).
        if not trace_id:
            tc = self._current_trace_context()
            if tc:
                trace_id = tc.get("trace_id")
                span_id = tc.get("span_id")
        if trace_id:
            entry["trace_id"] = trace_id
        if span_id:
            entry["span_id"] = span_id
        _scrub_entry(entry, self._scrub_needles, self._scrub_regexes)
        if self.opts.before_send is not None:
            entry = self.opts.before_send(entry)  # type: ignore[assignment]
            if entry is None:
                return
        try:
            self._queue.put_nowait(entry)
        except queue.Full:
            sys.stderr.write("[faro] cola llena, evento descartado\n")

    def capture_exception(
        self,
        exc: BaseException | None = None,
        tags: dict[str, str] | None = None,
        message: str | None = None,
    ) -> None:
        if exc is None:
            exc = sys.exc_info()[1]
        if exc is None:
            return
        attrs: dict[str, Any] = {
            "exception.type": type(exc).__name__,
            "exception.message": str(exc),
            "exception.stacktrace": "".join(
                traceback.format_exception(type(exc), exc, exc.__traceback__)
            ),
        }
        if tags:
            attrs.update(tags)
        self.log(
            level="ERROR",
            message=message or f"{type(exc).__name__}: {exc}",
            attributes=attrs,
        )

    # ---------- Product events API (Segment/PostHog-like) ----------

    def track(self, event_name: str, properties: dict[str, Any] | None = None) -> None:
        self._enqueue_event(type_="track", name=event_name, properties=properties or {})

    def identify(self, user_id: str, traits: dict[str, Any] | None = None) -> None:
        if not user_id:
            return
        self._distinct_id = user_id
        if traits:
            self._user_properties.update(traits)
        self._enqueue_event(
            type_="identify",
            name="$identify",
            properties={},
            user_properties_override=traits or {},
        )

    def alias(self, prev_id: str, new_id: str) -> None:
        if not prev_id or not new_id:
            return
        self._distinct_id = new_id
        self._enqueue_event(
            type_="alias",
            name="$alias",
            properties={},
            anonymous_id_override=prev_id,
        )

    def _enqueue_event(
        self,
        type_: str,
        name: str,
        properties: dict[str, Any],
        user_properties_override: dict[str, Any] | None = None,
        anonymous_id_override: str | None = None,
    ) -> None:
        if self._closed.is_set():
            return
        ctx: dict[str, Any] = {}
        if self.opts.environment:
            ctx["environment"] = self.opts.environment
        if self.opts.release:
            ctx["release"] = self.opts.release
        if self.opts.attributes:
            ctx.update(self.opts.attributes)
        event = {
            "type": type_,
            "name": name,
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "distinct_id": self._distinct_id or self._anonymous_id,
            "anonymous_id": anonymous_id_override or self._anonymous_id,
            "session_id": "",
            "properties": properties,
            "user_properties": user_properties_override
            if user_properties_override is not None
            else self._user_properties,
            "context": ctx,
            "source": "backend",
        }
        tc = self._current_trace_context()
        if tc:
            event.update(tc)
        try:
            self._events_queue.put_nowait(event)
        except queue.Full:
            sys.stderr.write("[faro] cola de events llena, evento descartado\n")

    def _current_trace_context(self) -> dict[str, str] | None:
        if self.opts.trace_context is not None:
            try:
                explicit = _normalize_trace_context(self.opts.trace_context())
            except Exception:
                explicit = None
            if explicit:
                return explicit
        return _otel_trace_context_from_active_span()

    # ---------- Tracing API (respaldada por OTel) ----------

    def start_span(
        self,
        name: str,
        kind: SpanKind = "INTERNAL",
        attributes: dict[str, Any] | None = None,
        parent: Any = ...,
        start_time: datetime | None = None,
    ) -> Span:
        """Crea un span (sin activarlo). Para auto-activar, usá use_span."""
        tracer = _get_tracer()
        attrs: dict[str, str] = {}
        if attributes:
            for k, v in attributes.items():
                attrs[k] = _stringify_attr(v)
        ctx = _make_parent_context(parent)
        otel_kind = _otel_span_kind(kind)
        kwargs: dict[str, Any] = {"attributes": attrs}
        if otel_kind is not None:
            kwargs["kind"] = otel_kind
        if start_time is not None:
            kwargs["start_time"] = int(start_time.timestamp() * 1e9)
        if ctx is not None:
            kwargs["context"] = ctx
        otel_span = tracer.start_span(name, **kwargs)
        return Span(otel_span)

    @contextmanager
    def use_span(
        self,
        name: str,
        kind: SpanKind = "INTERNAL",
        attributes: dict[str, Any] | None = None,
        parent: Any = ...,
    ) -> Iterator[Span]:
        """Context manager: crea, activa (via OTel context), cierra. Si el bloque
        lanza, marca status=ERROR + record_exception. Re-lanza siempre."""
        from opentelemetry import context as otel_context
        from opentelemetry import trace as otel_trace

        span = self.start_span(name=name, kind=kind, attributes=attributes, parent=parent)
        # Activar el span en el context — esto hace que subsiguientes start_span
        # (sin parent= explícito) lo tomen como padre.
        ctx = otel_trace.set_span_in_context(span._otel)
        token = otel_context.attach(ctx)
        span._ctx_token = token
        try:
            yield span
        except BaseException as e:
            span.record_exception(e)
            raise
        finally:
            if not span.ended:
                span.end()

    def active_span(self) -> Span | None:
        otel_span = _get_current_otel_span()
        if otel_span is None:
            return None
        return Span(otel_span)

    def flush(self, timeout: float = 5.0) -> None:
        # Despierta a los workers de logs/events; espera a que se vacíen.
        deadline = time.monotonic() + timeout
        while (
            not self._queue.empty() or not self._events_queue.empty()
        ) and time.monotonic() < deadline:
            time.sleep(0.05)
        # Drena el batch processor de OTel.
        remaining = max(0.0, deadline - time.monotonic())
        _flush_tracing(timeout_ms=int(remaining * 1000) or 1)

    def close(self, timeout: float = 5.0) -> None:
        """Cierra el SDK drenando colas + apagando OTel."""
        if self._closed.is_set():
            return
        self._closed.set()
        # Repartimos el presupuesto: drenado + join + tracing shutdown.
        third = max(0.5, timeout / 3)
        self.flush(timeout=third)
        join_each = max(0.25, third / 2)
        self._worker.join(timeout=join_each)
        self._events_worker.join(timeout=join_each)
        _shutdown_tracing(timeout_ms=int(third * 1000))

    # ---------- Workers (en segundo plano) ----------

    def _run(self) -> None:
        batch: list[dict[str, Any]] = []
        last_flush = time.monotonic()
        while True:
            timeout = max(0.0, self.opts.flush_interval_s - (time.monotonic() - last_flush))
            try:
                entry = self._queue.get(timeout=timeout)
                batch.append(entry)
            except queue.Empty:
                pass
            ready = (
                len(batch) >= self.opts.max_batch_size
                or (batch and (time.monotonic() - last_flush) >= self.opts.flush_interval_s)
            )
            if ready or (self._closed.is_set() and batch):
                ok = self._send(batch)
                if not ok:
                    for item in batch:
                        try:
                            self._queue.put_nowait(item)
                        except queue.Full:
                            sys.stderr.write("[faro] cola llena al reintentar, evento descartado\n")
                batch = []
                last_flush = time.monotonic()
            if self._closed.is_set() and self._queue.empty() and not batch:
                return

    def _run_events(self) -> None:
        batch: list[dict[str, Any]] = []
        last_flush = time.monotonic()
        while True:
            timeout = max(0.0, self.opts.flush_interval_s - (time.monotonic() - last_flush))
            try:
                event = self._events_queue.get(timeout=timeout)
                batch.append(event)
            except queue.Empty:
                pass
            ready = (
                len(batch) >= self.opts.max_batch_size
                or (batch and (time.monotonic() - last_flush) >= self.opts.flush_interval_s)
            )
            if ready or (self._closed.is_set() and batch):
                ok = self._send_events(batch)
                if not ok:
                    for item in batch:
                        try:
                            self._events_queue.put_nowait(item)
                        except queue.Full:
                            sys.stderr.write("[faro] cola de events llena al reintentar, evento descartado\n")
                batch = []
                last_flush = time.monotonic()
            if self._closed.is_set() and self._events_queue.empty() and not batch:
                return

    def _send_events(self, batch: Iterable[dict[str, Any]]) -> bool:
        payload = {"service": self.opts.service, "events": list(batch)}
        try:
            r = self._session.post(
                f"{self.opts.endpoint}/api/v1/ingest/events",
                json=payload,
                headers={"Authorization": f"Bearer {self.opts.token}"},
                timeout=self.opts.timeout,
            )
            if r.status_code >= 400:
                sys.stderr.write(f"[faro] ingest events HTTP {r.status_code}: {r.text[:200]}\n")
                return r.status_code < 500
            return True
        except requests.RequestException as e:
            sys.stderr.write(f"[faro] falló el flush de events: {e}\n")
            return False

    def _send(self, batch: Iterable[dict[str, Any]]) -> bool:
        payload = {"service": self.opts.service, "logs": list(batch)}
        try:
            r = self._session.post(
                f"{self.opts.endpoint}/api/v1/ingest/logs",
                json=payload,
                headers={"Authorization": f"Bearer {self.opts.token}"},
                timeout=self.opts.timeout,
            )
            if r.status_code >= 400:
                sys.stderr.write(f"[faro] ingest HTTP {r.status_code}: {r.text[:200]}\n")
                return r.status_code < 500
            return True
        except requests.RequestException as e:
            sys.stderr.write(f"[faro] falló el flush: {e}\n")
            return False

    # ---------- Auto-captura ----------

    def _install_handlers(self) -> None:
        prev_excepthook = sys.excepthook

        def hook(exc_type, exc, tb):
            try:
                self.capture_exception(exc, message=f"[unhandled] {exc_type.__name__}")
                self.flush(timeout=2.0)
            finally:
                prev_excepthook(exc_type, exc, tb)

        sys.excepthook = hook

        if hasattr(threading, "excepthook"):
            prev_thread = threading.excepthook

            def thook(args: threading.ExceptHookArgs) -> None:
                try:
                    self.capture_exception(
                        args.exc_value,
                        message=f"[thread] {args.thread.name}: {args.exc_type.__name__}",
                    )
                finally:
                    prev_thread(args)

            threading.excepthook = thook


# ---------- Singleton a nivel de módulo ----------

_client: _Client | None = None


def init(
    endpoint: str | None = None,
    token: str | None = None,
    service: str = "unknown",
    environment: str | None = None,
    release: str | None = None,
    attributes: dict[str, Any] | None = None,
    flush_interval_s: float = 0.75,
    max_batch_size: int = 200,
    max_queue_size: int = 10_000,
    install_global_handlers: bool = True,
    timeout: float = 5.0,
    scrub_fields: tuple[str, ...] | list[str] = _DEFAULT_SCRUB_FIELDS,
    scrub_headers: bool = True,
    scrub_patterns: tuple[str, ...] | list[str] = ("jwt", "api-key"),
    before_send: Callable[[dict[str, Any]], dict[str, Any] | None] | None = None,
    trace_context: TraceContextProvider | None = None,
    enable_tracing: bool = True,
    traces_endpoint: str | None = None,
    resource_attributes: dict[str, str] | None = None,
    disabled_instrumentations: tuple[str, ...] | list[str] = (),
) -> _Client:
    """Inicializa el SDK. Si no se pasan, endpoint y token caen en las env vars FARO_ENDPOINT / FARO_TOKEN."""
    global _client
    endpoint = endpoint or os.environ.get("FARO_ENDPOINT")
    token = token or os.environ.get("FARO_TOKEN") or os.environ.get("FARO_INGEST_TOKEN")
    if not endpoint or not token:
        raise ValueError(
            "faro.init: 'endpoint' y 'token' son obligatorios (o define FARO_ENDPOINT/FARO_TOKEN)"
        )
    if _client is not None:
        _client.close()
    _client = _Client(
        _Options(
            endpoint=endpoint,
            token=token,
            service=service,
            environment=environment,
            release=release,
            attributes=attributes or {},
            flush_interval_s=flush_interval_s,
            max_batch_size=max_batch_size,
            max_queue_size=max_queue_size,
            install_global_handlers=install_global_handlers,
            timeout=timeout,
            scrub_fields=tuple(scrub_fields),
            scrub_headers=scrub_headers,
            scrub_patterns=tuple(scrub_patterns),
            before_send=before_send,
            trace_context=trace_context,
            enable_tracing=enable_tracing,
            traces_endpoint=traces_endpoint,
            resource_attributes=resource_attributes,
            disabled_instrumentations=tuple(disabled_instrumentations),
        )
    )
    return _client


def _need() -> _Client:
    if _client is None:
        raise RuntimeError("faro_sdk: llama a faro.init() antes de loguear")
    return _client


def log(level: str, message: str, **attrs: Any) -> None:
    _need().log(level=level, message=message, attributes=attrs)


def info(message: str, **attrs: Any) -> None:
    _need().log(level="INFO", message=message, attributes=attrs)


def warn(message: str, **attrs: Any) -> None:
    _need().log(level="WARN", message=message, attributes=attrs)


def warning(message: str, **attrs: Any) -> None:
    _need().log(level="WARN", message=message, attributes=attrs)


def error(message: str, **attrs: Any) -> None:
    _need().log(level="ERROR", message=message, attributes=attrs)


def capture_exception(
    exc: BaseException | None = None,
    tags: dict[str, str] | None = None,
    message: str | None = None,
) -> None:
    _need().capture_exception(exc=exc, tags=tags, message=message)


def track(event_name: str, properties: dict[str, Any] | None = None) -> None:
    _need().track(event_name, properties)


def identify(user_id: str, traits: dict[str, Any] | None = None) -> None:
    _need().identify(user_id, traits)


def alias(prev_id: str, new_id: str) -> None:
    _need().alias(prev_id, new_id)


def start_span(
    name: str,
    kind: SpanKind = "INTERNAL",
    attributes: dict[str, Any] | None = None,
    parent: Any = ...,
    start_time: datetime | None = None,
) -> Span:
    return _need().start_span(
        name=name, kind=kind, attributes=attributes, parent=parent, start_time=start_time
    )


def use_span(
    name: str,
    kind: SpanKind = "INTERNAL",
    attributes: dict[str, Any] | None = None,
    parent: Any = ...,
):
    return _need().use_span(name=name, kind=kind, attributes=attributes, parent=parent)


def active_span() -> Span | None:
    if _client is None:
        return None
    return _client.active_span()


def flush(timeout: float = 5.0) -> None:
    if _client is not None:
        _client.flush(timeout=timeout)


def close(timeout: float = 5.0) -> None:
    global _client
    if _client is not None:
        _client.close(timeout=timeout)
        _client = None


# Re-exports OTel passthrough — para users que quieran control fino del tracing
# sin pasar por la inicialización del cliente Faro.

def init_tracing(*args: Any, **kwargs: Any) -> bool:
    return _init_tracing(*args, **kwargs)


def shutdown_tracing(timeout_ms: int = 5000) -> None:
    _shutdown_tracing(timeout_ms=timeout_ms)


def flush_tracing(timeout_ms: int = 5000) -> None:
    _flush_tracing(timeout_ms=timeout_ms)


def get_tracer() -> Any:
    return _get_tracer()


class FaroHandler(logging.Handler):
    """logging.Handler de uso directo. Reenvía los registros del logging estándar a Faro."""

    def emit(self, record: logging.LogRecord) -> None:
        if _client is None:
            return
        lvl = record.levelname.upper()
        if lvl == "WARNING":
            lvl = "WARN"
        attrs: dict[str, Any] = {
            "logger": record.name,
            "module": record.module,
            "lineno": record.lineno,
        }
        if record.exc_info:
            etype, evalue, tb = record.exc_info
            attrs["exception.type"] = etype.__name__ if etype else "Exception"
            attrs["exception.message"] = str(evalue)
            attrs["exception.stacktrace"] = "".join(traceback.format_exception(etype, evalue, tb))
        _client.log(level=lvl, message=record.getMessage(), attributes=attrs)
