"""Tests de feature flags del SDK Python.

Cubren:
  (a) los 5 vectores dorados del hash sticky (_sticky_bucket / FNV-1a 32-bit),
  (b) rollout_percentage=100 → is_feature_enabled True + se encola un evento
      $feature_exposure con variant "B",
  (c) conditions.properties no satisfechas → False y SIN exposición encolada.

Como el resto de la suite, no requiere red real: levantamos un http.server local
que sirve GET /api/v1/ingest/feature-flags y captura los POST a
/api/v1/ingest/events.
"""

from __future__ import annotations

import json
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any

import pytest

import faro_sdk as faro
from faro_sdk import _sticky_bucket


# -------- (a) vectores dorados del hash --------


def test_sticky_bucket_golden_vectors():
    cases = {
        "proj:new-checkout:user_42": 9,
        "acme:flag-a:anon_x": 54,
        "myproj:dark-mode:user_1": 75,
        "p:k:abcdefghij": 49,
        "demo:exp1:user_42": 34,
    }
    for s, expected in cases.items():
        assert _sticky_bucket(s) == expected, f"{s!r} → {_sticky_bucket(s)} (esperado {expected})"


# -------- fixture: server local (GET feature-flags + POST events) --------


class _FlagsHandler(BaseHTTPRequestHandler):
    # Configurables por test antes de inicializar el cliente.
    flags_payload: dict[str, Any] = {"project": "proj", "flags": []}
    flags_status: int = 200
    received_events: list[dict[str, Any]] = []

    def do_GET(self) -> None:  # noqa: N802 (firma estándar de BaseHTTPRequestHandler)
        if self.path == "/api/v1/ingest/feature-flags":
            body = json.dumps(_FlagsHandler.flags_payload).encode("utf-8")
            self.send_response(_FlagsHandler.flags_status)
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(body)
            return
        self.send_response(404)
        self.end_headers()

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("content-length", "0"))
        raw = self.rfile.read(length).decode("utf-8")
        try:
            parsed = json.loads(raw)
        except Exception:
            parsed = {"_raw": raw}
        if self.path == "/api/v1/ingest/events":
            _FlagsHandler.received_events.append(parsed)
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"ok":true}')

    def log_message(self, *_a: Any, **_k: Any) -> None:  # silencia el log por request
        pass


@pytest.fixture
def server():
    _FlagsHandler.received_events = []
    _FlagsHandler.flags_payload = {"project": "proj", "flags": []}
    _FlagsHandler.flags_status = 200
    srv = HTTPServer(("127.0.0.1", 0), _FlagsHandler)
    t = threading.Thread(target=srv.serve_forever, daemon=True)
    t.start()
    yield srv, _FlagsHandler
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


def _collect_events(handler: type[_FlagsHandler]) -> list[dict[str, Any]]:
    return [e for batch in handler.received_events if "events" in batch for e in batch["events"]]


# -------- (b) rollout 100 → True + $feature_exposure variant B --------


def test_rollout_100_enabled_y_encola_exposure_b(server):
    srv, h = server
    port = srv.server_address[1]
    h.flags_payload = {
        "project": "proj",
        "flags": [{"key": "new-checkout", "rollout_percentage": 100, "conditions": {}}],
    }

    c = faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="ff-test",
        install_global_handlers=False,
        flush_interval_s=0.05,
        # No esperamos al timer periódico: forzamos el primer fetch a mano,
        # igual que node (que tampoco hace fetch inicial inmediato).
        feature_flag_refresh_interval=3600.0,
    )
    c.refresh_feature_flags()

    assert faro.is_feature_enabled("new-checkout", distinct_id="user_42") is True

    faro.flush(timeout=2.0)
    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and not _collect_events(h):
        time.sleep(0.05)

    exposures = [e for e in _collect_events(h) if e["name"] == "$feature_exposure"]
    assert exposures, f"esperaba un $feature_exposure; recibido: {h.received_events}"
    ev = exposures[0]
    assert ev["type"] == "track"
    assert ev["distinct_id"] == "user_42", "el distinct_id del evento es el override, no el del SDK"
    assert ev["properties"] == {"flag_key": "new-checkout", "variant": "B", "enabled": True}

    # Dedupe: una segunda evaluación con el mismo (flag, id, variant) no re-encola.
    assert faro.is_feature_enabled("new-checkout", distinct_id="user_42") is True
    faro.flush(timeout=1.0)
    exposures_again = [e for e in _collect_events(h) if e["name"] == "$feature_exposure"]
    assert len(exposures_again) == 1, "el exposure debe deduplicarse por (project,flag,id,variant)"


# -------- (c) conditions no satisfechas → False sin exposición --------


def test_conditions_no_satisfechas_false_sin_exposure(server):
    srv, h = server
    port = srv.server_address[1]
    h.flags_payload = {
        "project": "proj",
        "flags": [
            {
                "key": "beta-feature",
                "rollout_percentage": 100,
                "conditions": {"properties": {"plan": "enterprise"}},
            }
        ],
    }

    c = faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="ff-cond-test",
        install_global_handlers=False,
        flush_interval_s=0.05,
        feature_flag_refresh_interval=3600.0,
    )
    c.refresh_feature_flags()

    # plan != enterprise → no matchea conditions → False y SIN exposición.
    assert (
        faro.is_feature_enabled(
            "beta-feature", distinct_id="user_1", properties={"plan": "free"}
        )
        is False
    )

    faro.flush(timeout=1.0)
    time.sleep(0.2)
    exposures = [e for e in _collect_events(h) if e["name"] == "$feature_exposure"]
    assert exposures == [], "condiciones no satisfechas no deben emitir exposición"

    # Mismo flag, ahora con la propiedad requerida → True (sí matchea + se evalúa).
    assert (
        faro.is_feature_enabled(
            "beta-feature", distinct_id="user_1", properties={"plan": "enterprise"}
        )
        is True
    )


# -------- extra: flag inexistente → False, sin exposición --------


def test_flag_inexistente_false_sin_exposure(server):
    srv, h = server
    port = srv.server_address[1]
    c = faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="ff-missing-test",
        install_global_handlers=False,
        flush_interval_s=0.05,
        feature_flag_refresh_interval=3600.0,
    )
    c.refresh_feature_flags()

    assert faro.is_feature_enabled("does-not-exist", distinct_id="user_1") is False
    faro.flush(timeout=1.0)
    time.sleep(0.2)
    assert _collect_events(h) == [], "un flag inexistente no debe encolar nada"
