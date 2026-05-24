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
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, Callable, Iterable

import requests

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
    "flush",
    "close",
    "FaroHandler",
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


def _otel_current_trace_context() -> dict[str, str] | None:
    try:
        from opentelemetry import trace  # type: ignore
    except Exception:
        return None
    try:
        span = trace.get_current_span()
        span_context = span.get_span_context()
        if not getattr(span_context, "is_valid", False):
            return None
        return {
            "trace_id": format(span_context.trace_id, "032x"),
            "span_id": format(span_context.span_id, "016x"),
        }
    except Exception:
        return None


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
        # Cola paralela para product events. Mismo modelo que `_queue` pero su
        # worker postea a `/ingest/events` en vez de `/ingest/logs`.
        self._events_queue: queue.Queue[dict[str, Any]] = queue.Queue(maxsize=opts.max_queue_size)
        # Estado de identidad para los product events. `_distinct_id` se setea
        # con `identify()`; mientras esté vacío, los eventos se atribuyen al
        # `_anonymous_id` generado en el boot del cliente.
        self._distinct_id: str = ""
        self._anonymous_id: str = f"anon_{os.urandom(8).hex()}"
        self._user_properties: dict[str, Any] = {}
        self._closed = threading.Event()
        self._session = requests.Session()
        self._worker = threading.Thread(target=self._run, daemon=True, name="faro-flush")
        self._worker.start()
        # Worker separado para events: misma cadencia de flush, mismo backoff,
        # pero contra `/ingest/events`. Mantenerlos separados evita que un batch
        # de logs grande retrase los events (y viceversa) en la misma sección
        # crítica.
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
        """Envía un evento custom de producto. Equivalente a `analytics.track()`."""
        self._enqueue_event(type_="track", name=event_name, properties=properties or {})

    def identify(self, user_id: str, traits: dict[str, Any] | None = None) -> None:
        """Setea el `distinct_id` para los eventos siguientes y emite `$identify`."""
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
        """Fusiona una sesión pre-login (`prev_id`) con un usuario post-login (`new_id`)."""
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
        trace = self._current_trace_context()
        if trace:
            event.update(trace)
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
        return _otel_current_trace_context()

    def flush(self, timeout: float = 5.0) -> None:
        # Despierta a ambos workers; si alguno ya no está corriendo, lo de él
        # se queda en la cola hasta el próximo init (rare en práctica). Damos
        # mitad del presupuesto a cada cola — log() suele ser el más volumoso.
        deadline = time.monotonic() + timeout
        while (not self._queue.empty() or not self._events_queue.empty()) and time.monotonic() < deadline:
            time.sleep(0.05)

    def close(self, timeout: float = 5.0) -> None:
        """Cierra el SDK, drenando la cola y esperando a que el worker termine.

        El parámetro `timeout` acota tanto el drenado de la cola como el join
        del thread worker — porque es daemon, sin join podría quedar truncado
        a mitad de HTTP request si el proceso muere justo después.
        """
        if self._closed.is_set():
            return
        self._closed.set()
        # Reparto: la mitad del presupuesto al drenado, la otra al join (en el peor caso
        # _send() está bloqueado en HTTP justo cuando llamamos close).
        half = max(0.5, timeout / 2)
        self.flush(timeout=half)
        # Espera explícita a que ambos workers drenen el batch en vuelo y terminen.
        # Sin join, los daemon threads quedarían truncados al salir el proceso
        # (en mitad de POST). Repartimos el presupuesto restante entre los dos.
        join_each = max(0.25, half / 2)
        self._worker.join(timeout=join_each)
        self._events_worker.join(timeout=join_each)

    # ---------- Worker (en segundo plano) ----------

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
                    # 5xx / red caída → reintentamos el batch en la próxima iteración.
                    # Reinsertamos delante para mantener orden aproximado.
                    for item in batch:
                        try:
                            self._queue.put_nowait(item)
                        except queue.Full:
                            # cola llena → caemos en la misma regla que log(): descartar.
                            sys.stderr.write("[faro] cola llena al reintentar, evento descartado\n")
                batch = []
                last_flush = time.monotonic()
            if self._closed.is_set() and self._queue.empty() and not batch:
                return

    def _run_events(self) -> None:
        """Worker análogo a `_run` pero contra la cola de events y `/ingest/events`."""
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
        """Devuelve True si el batch se aceptó (2xx/4xx — 4xx descartamos, no reintenta).
        False si hubo 5xx o error de red → el caller re-encola."""
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
                # 4xx → batch malformado / auth inválida; reintentar acumularía basura.
                # 5xx → caller reintenta.
                return r.status_code < 500
            return True
        except requests.RequestException as e:
            sys.stderr.write(f"[faro] falló el flush: {e}\n")
            return False

    # ---------- Auto-captura ----------

    def _install_handlers(self) -> None:
        # sys.excepthook se dispara ante excepciones no manejadas en el thread principal.
        prev_excepthook = sys.excepthook

        def hook(exc_type, exc, tb):
            try:
                self.capture_exception(exc, message=f"[unhandled] {exc_type.__name__}")
                self.flush(timeout=2.0)
            finally:
                prev_excepthook(exc_type, exc, tb)

        sys.excepthook = hook

        # threading.excepthook (3.8+) para crashes en threads worker.
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
) -> _Client:
    """Inicializa el SDK. Si no se pasan, endpoint y token caen en las env vars FARO_ENDPOINT / FARO_TOKEN."""
    global _client
    endpoint = endpoint or os.environ.get("FARO_ENDPOINT")
    token = token or os.environ.get("FARO_TOKEN")
    if not endpoint or not token:
        raise ValueError("faro.init: 'endpoint' y 'token' son obligatorios (o define FARO_ENDPOINT/FARO_TOKEN)")
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


# Alias para encajar con el nombre del módulo `logging` estándar (WARNING).
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


def flush(timeout: float = 5.0) -> None:
    if _client is not None:
        _client.flush(timeout=timeout)


def close(timeout: float = 5.0) -> None:
    global _client
    if _client is not None:
        _client.close(timeout=timeout)
        _client = None


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
        # Expone la info de excepción cuando se usa log.exception() / exc_info=True.
        if record.exc_info:
            etype, evalue, tb = record.exc_info
            attrs["exception.type"] = etype.__name__ if etype else "Exception"
            attrs["exception.message"] = str(evalue)
            attrs["exception.stacktrace"] = "".join(traceback.format_exception(etype, evalue, tb))
        _client.log(level=lvl, message=record.getMessage(), attributes=attrs)
