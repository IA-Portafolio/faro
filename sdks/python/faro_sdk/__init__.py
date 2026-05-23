"""Faro SDK for Python.

Usage:

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
import sys
import threading
import time
import traceback
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, Iterable

import requests

__all__ = [
    "init",
    "log",
    "info",
    "warn",
    "error",
    "capture_exception",
    "flush",
    "close",
    "FaroHandler",
]

_SEVERITIES = {"TRACE", "DEBUG", "INFO", "WARN", "ERROR", "FATAL"}


@dataclass
class _Options:
    endpoint: str
    token: str
    service: str
    environment: str | None = None
    release: str | None = None
    attributes: dict[str, Any] = field(default_factory=dict)
    flush_interval_s: float = 0.75
    max_batch_size: int = 200
    max_queue_size: int = 10_000
    install_global_handlers: bool = True
    timeout: float = 5.0


class _Client:
    def __init__(self, opts: _Options) -> None:
        self.opts = opts
        self.opts.endpoint = self.opts.endpoint.rstrip("/")
        self._queue: queue.Queue[dict[str, Any]] = queue.Queue(maxsize=opts.max_queue_size)
        self._closed = threading.Event()
        self._session = requests.Session()
        self._worker = threading.Thread(target=self._run, daemon=True, name="faro-flush")
        self._worker.start()
        if opts.install_global_handlers:
            self._install_handlers()
        atexit.register(self.close)

    # ---------- Public API ----------

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
        try:
            self._queue.put_nowait(entry)
        except queue.Full:
            sys.stderr.write("[faro] queue full, dropping event\n")

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

    def flush(self, timeout: float = 5.0) -> None:
        # Wake the worker; if it's not running anymore, send synchronously.
        deadline = time.monotonic() + timeout
        while not self._queue.empty() and time.monotonic() < deadline:
            time.sleep(0.05)

    def close(self) -> None:
        if self._closed.is_set():
            return
        self._closed.set()
        self.flush(timeout=3.0)
        # Worker exits on next loop iteration once _closed is set + queue empty.

    # ---------- Worker ----------

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
                self._send(batch)
                batch = []
                last_flush = time.monotonic()
            if self._closed.is_set() and self._queue.empty() and not batch:
                return

    def _send(self, batch: Iterable[dict[str, Any]]) -> None:
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
        except requests.RequestException as e:
            sys.stderr.write(f"[faro] flush failed: {e}\n")

    # ---------- Auto-capture ----------

    def _install_handlers(self) -> None:
        # sys.excepthook fires for unhandled exceptions in the main thread.
        prev_excepthook = sys.excepthook

        def hook(exc_type, exc, tb):
            try:
                self.capture_exception(exc, message=f"[unhandled] {exc_type.__name__}")
                self.flush(timeout=2.0)
            finally:
                prev_excepthook(exc_type, exc, tb)

        sys.excepthook = hook

        # threading.excepthook (3.8+) for crashes in worker threads.
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


# ---------- Module-level singleton ----------

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
) -> _Client:
    """Initialise the SDK. Endpoint and token fall back to FARO_ENDPOINT / FARO_TOKEN env vars."""
    global _client
    endpoint = endpoint or os.environ.get("FARO_ENDPOINT")
    token = token or os.environ.get("FARO_TOKEN")
    if not endpoint or not token:
        raise ValueError("faro.init: 'endpoint' and 'token' are required (or set FARO_ENDPOINT/FARO_TOKEN)")
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
        )
    )
    return _client


def _need() -> _Client:
    if _client is None:
        raise RuntimeError("faro_sdk: call faro.init() before logging")
    return _client


def log(level: str, message: str, **attrs: Any) -> None:
    _need().log(level=level, message=message, attributes=attrs)


def info(message: str, **attrs: Any) -> None:
    _need().log(level="INFO", message=message, attributes=attrs)


def warn(message: str, **attrs: Any) -> None:
    _need().log(level="WARN", message=message, attributes=attrs)


def error(message: str, **attrs: Any) -> None:
    _need().log(level="ERROR", message=message, attributes=attrs)


def capture_exception(
    exc: BaseException | None = None,
    tags: dict[str, str] | None = None,
    message: str | None = None,
) -> None:
    _need().capture_exception(exc=exc, tags=tags, message=message)


def flush(timeout: float = 5.0) -> None:
    if _client is not None:
        _client.flush(timeout=timeout)


def close() -> None:
    global _client
    if _client is not None:
        _client.close()
        _client = None


class FaroHandler(logging.Handler):
    """Drop-in logging.Handler. Forwards stdlib logging records to Faro."""

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
        # Surface exception info when log.exception() / exc_info=True is used.
        if record.exc_info:
            etype, evalue, tb = record.exc_info
            attrs["exception.type"] = etype.__name__ if etype else "Exception"
            attrs["exception.message"] = str(evalue)
            attrs["exception.stacktrace"] = "".join(traceback.format_exception(etype, evalue, tb))
        _client.log(level=lvl, message=record.getMessage(), attributes=attrs)
