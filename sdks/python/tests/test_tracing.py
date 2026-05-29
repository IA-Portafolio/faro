"""Tests del API de tracing del SDK Python — invariantes:

  1. start_span / end → POST OTLP/JSON a /v1/traces con shape correcta
  2. parent explícito: hijo hereda trace_id y setea parent_span_id
  3. use_span: cierra solo y propaga ERROR si el bloque lanza
  4. contextvar: spans anidados sin pasar parent
  5. logs dentro de use_span auto-heredan trace_id/span_id
  6. traceparent(): formato W3C válido
  7. record_exception: status=ERROR + exception.* attrs
  8. queue cap descarta spans
"""

from __future__ import annotations

import json
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any

import pytest

import faro_sdk as faro


class _TraceCaptureHandler(BaseHTTPRequestHandler):
    """Server local que separa los POST a /v1/traces de los de /api/v1/ingest/logs."""

    received_traces: list[dict[str, Any]] = []
    received_logs: list[dict[str, Any]] = []
    next_status_traces: int = 200
    next_status_logs: int = 200

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(length).decode("utf-8")
        try:
            data = json.loads(body)
        except Exception:
            data = {"_raw": body}
        if self.path == "/v1/traces":
            _TraceCaptureHandler.received_traces.append(data)
            status = _TraceCaptureHandler.next_status_traces
        elif self.path == "/api/v1/ingest/logs":
            _TraceCaptureHandler.received_logs.append(data)
            status = _TraceCaptureHandler.next_status_logs
        else:
            status = 200
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"ok":true}')

    def log_message(self, *_a: Any, **_k: Any) -> None:
        pass


@pytest.fixture
def server():
    _TraceCaptureHandler.received_traces = []
    _TraceCaptureHandler.received_logs = []
    _TraceCaptureHandler.next_status_traces = 200
    _TraceCaptureHandler.next_status_logs = 200
    srv = HTTPServer(("127.0.0.1", 0), _TraceCaptureHandler)
    t = threading.Thread(target=srv.serve_forever, daemon=True)
    t.start()
    yield srv, _TraceCaptureHandler
    srv.shutdown()


@pytest.fixture(autouse=True)
def _reset_singleton():
    try:
        faro.close(timeout=0.5)
    except Exception:
        pass
    yield
    try:
        faro.close(timeout=0.5)
    except Exception:
        pass


def _all_spans(traces: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out = []
    for req in traces:
        for rs in req.get("resourceSpans", []):
            for ss in rs.get("scopeSpans", []):
                out.extend(ss.get("spans", []))
    return out


def _get_attr(attrs: list[dict[str, Any]], key: str) -> str | None:
    for a in attrs:
        if a.get("key") == key:
            return a.get("value", {}).get("stringValue")
    return None


def _wait(predicate, timeout: float = 3.0) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(0.05)
    return False


def test_start_span_emite_otlp_json(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="trace-test",
        environment="prod",
        release="1.2.3",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    span = faro.start_span("checkout", kind="SERVER", attributes={"http.method": "POST"})
    span.set_attribute("user.id", 42)
    span.add_event("cache.miss", {"key": "abc"})
    span.set_status("OK")
    span.end()
    faro.flush(timeout=2.0)
    assert _wait(lambda: len(h.received_traces) >= 1)

    rs = h.received_traces[0]["resourceSpans"][0]
    res_attrs = rs["resource"]["attributes"]
    assert _get_attr(res_attrs, "service.name") == "trace-test"
    assert _get_attr(res_attrs, "deployment.environment") == "prod"
    assert _get_attr(res_attrs, "service.version") == "1.2.3"

    sp = _all_spans(h.received_traces)[0]
    assert sp["name"] == "checkout"
    assert sp["kind"] == 2  # SERVER
    assert len(sp["traceId"]) == 32
    assert len(sp["spanId"]) == 16
    assert "parentSpanId" not in sp or not sp["parentSpanId"]
    assert sp["status"]["code"] == 1  # OK
    assert _get_attr(sp["attributes"], "user.id") == "42"
    assert sp["events"][0]["name"] == "cache.miss"


def test_parent_explicito_hereda_trace_id(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="t",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    parent = faro.start_span("parent")
    child = faro.start_span("child", parent={"trace_id": parent.trace_id, "span_id": parent.span_id})
    child.end()
    parent.end()
    faro.flush(timeout=2.0)
    assert _wait(lambda: len(_all_spans(h.received_traces)) >= 2)

    spans = _all_spans(h.received_traces)
    p = next(s for s in spans if s["name"] == "parent")
    c = next(s for s in spans if s["name"] == "child")
    assert c["traceId"] == p["traceId"]
    assert c["parentSpanId"] == p["spanId"]


def test_use_span_propaga_error(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="t",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    with pytest.raises(ValueError, match="kaboom"):
        with faro.use_span("boom"):
            raise ValueError("kaboom")
    faro.flush(timeout=2.0)
    assert _wait(lambda: len(_all_spans(h.received_traces)) >= 1)

    sp = _all_spans(h.received_traces)[0]
    assert sp["status"]["code"] == 2  # ERROR
    assert _get_attr(sp["attributes"], "exception.type") == "ValueError"
    assert _get_attr(sp["attributes"], "exception.message") == "kaboom"


def test_contextvar_spans_anidados(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="t",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    with faro.use_span("outer"):
        with faro.use_span("inner"):
            assert faro.active_span() is not None
    faro.flush(timeout=2.0)
    assert _wait(lambda: len(_all_spans(h.received_traces)) >= 2)

    spans = _all_spans(h.received_traces)
    outer = next(s for s in spans if s["name"] == "outer")
    inner = next(s for s in spans if s["name"] == "inner")
    assert inner["traceId"] == outer["traceId"]
    assert inner["parentSpanId"] == outer["spanId"]


def test_logs_dentro_use_span_auto_heredan(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="t",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    captured_ctx: dict[str, str] = {}
    with faro.use_span("handler") as span:
        captured_ctx["trace_id"] = span.trace_id
        captured_ctx["span_id"] = span.span_id
        faro.info("procesando", foo="bar")
    faro.flush(timeout=2.0)
    assert _wait(lambda: len(h.received_logs) >= 1)

    log = h.received_logs[0]["logs"][0]
    assert log["trace_id"] == captured_ctx["trace_id"]
    assert log["span_id"] == captured_ctx["span_id"]


def test_traceparent_formato_w3c():
    faro.init(
        endpoint="http://127.0.0.1:1",
        token="tk",
        service="t",
        install_global_handlers=False,
        flush_interval_s=1000,
    )
    span = faro.start_span("x")
    tp = span.traceparent()
    assert tp.startswith("00-")
    assert tp.endswith("-01")
    parts = tp.split("-")
    assert len(parts) == 4
    assert len(parts[1]) == 32
    assert len(parts[2]) == 16


def test_record_exception_setea_attrs(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="t",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    span = faro.start_span("op")
    span.record_exception(TypeError("bad input"))
    span.end()
    faro.flush(timeout=2.0)
    assert _wait(lambda: len(_all_spans(h.received_traces)) >= 1)

    sp = _all_spans(h.received_traces)[0]
    assert sp["status"]["code"] == 2  # ERROR
    assert _get_attr(sp["attributes"], "exception.type") == "TypeError"
    assert _get_attr(sp["attributes"], "exception.message") == "bad input"


def test_disable_tracing_omite_otel():
    """`enable_tracing=False` debe permitir que `start_span` siga funcionando como
    no-op (sin red, sin OTel SDK init), para no romper código que lo llame."""
    faro.init(
        endpoint="http://127.0.0.1:1",  # inalcanzable
        token="tk",
        service="t",
        install_global_handlers=False,
        flush_interval_s=1000,
        enable_tracing=False,
    )
    s = faro.start_span("noop")
    # Sin OTel inicializado, el trace_id es válido pero no se exporta.
    assert len(s.trace_id) == 32
    s.end()


def test_parent_none_fuerza_root(server):
    """parent=None debe ignorar el span activo y crear root."""
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="t",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    with faro.use_span("outer"):
        # parent=None debería NO heredar el outer
        root = faro.start_span("forced-root", parent=None)
        root.end()
    faro.flush(timeout=2.0)
    assert _wait(lambda: len(_all_spans(h.received_traces)) >= 2)
    spans = _all_spans(h.received_traces)
    outer = next(s for s in spans if s["name"] == "outer")
    forced = next(s for s in spans if s["name"] == "forced-root")
    assert forced["traceId"] != outer["traceId"], "parent=None debe romper la herencia"
    assert "parentSpanId" not in forced or not forced["parentSpanId"]
