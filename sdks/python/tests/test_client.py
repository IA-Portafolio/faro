"""Tests unitarios del SDK Python — 4 invariantes mínimas:

  1. queue cap descarta cuando se llena
  2. retry on 5xx
  3. before_send filtra (None → descartar) y transforma
  4. scrubbing aplica scrub_fields + scrub_patterns

No requiere red real: levantamos un http.server local en un thread.
"""

from __future__ import annotations

import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any

import pytest

import faro_sdk as faro


# -------- fixture: server local de captura --------


class _CaptureHandler(BaseHTTPRequestHandler):
    received: list[dict[str, Any]] = []
    next_status: int = 200

    def do_POST(self) -> None:  # noqa: N802 (firma estándar de BaseHTTPRequestHandler)
        length = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(length).decode("utf-8")
        try:
            _CaptureHandler.received.append(json.loads(body))
        except Exception:
            _CaptureHandler.received.append({"_raw": body})
        self.send_response(_CaptureHandler.next_status)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"ok":true}')

    def log_message(self, *_a: Any, **_k: Any) -> None:  # silencia el log por request
        pass


@pytest.fixture
def server():
    _CaptureHandler.received = []
    _CaptureHandler.next_status = 200
    srv = HTTPServer(("127.0.0.1", 0), _CaptureHandler)
    t = threading.Thread(target=srv.serve_forever, daemon=True)
    t.start()
    yield srv, _CaptureHandler
    srv.shutdown()


@pytest.fixture(autouse=True)
def _reset_singleton():
    """Asegura que cada test arranca sin singleton previo."""
    try:
        faro.close(timeout=0.5)
    except Exception:
        pass
    yield
    try:
        faro.close(timeout=0.5)
    except Exception:
        pass


# -------- 1. queue cap --------


def test_queue_cap_descarta_cuando_se_llena(capsys):
    # El cap se aplica en el PUT (queue.Full → log a stderr). Contamos los warnings
    # en lugar de la qsize, porque el worker drena en paralelo (consume al instante).
    faro.init(
        endpoint="http://127.0.0.1:1",  # inalcanzable: el flush nunca quita carga
        token="tk",
        service="queue-cap-test",
        install_global_handlers=False,
        flush_interval_s=1000,
        max_queue_size=5,
    )
    # Lanza muchos eventos en ráfaga antes de que el worker pueda drenar.
    for i in range(200):
        faro.info(f"evento {i}")
    captured = capsys.readouterr()
    n_descartados = captured.err.count("cola llena, evento descartado")
    assert n_descartados > 0, "con max_queue_size=5 y 200 eventos debe haber descartes"


# -------- 2. retry sobre 5xx --------


def test_retry_on_5xx(server):
    srv, h = server
    port = srv.server_address[1]

    h.next_status = 503
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="retry-test",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    faro.info("se reintenta")

    # Espera un poco para que el worker mande, falle y reintente.
    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and len(h.received) < 1:
        time.sleep(0.05)
    assert len(h.received) >= 1, "debe haber al menos un intento POST"
    first_calls = len(h.received)

    # Ahora respondemos OK. El worker debe reintentar el batch fallido.
    h.next_status = 200
    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and len(h.received) <= first_calls:
        time.sleep(0.05)
    assert len(h.received) > first_calls, "tras 5xx → 200 el batch se reintenta"


# -------- 3. before_send --------


def test_before_send_descarta_con_none(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="bs-discard",
        install_global_handlers=False,
        flush_interval_s=0.05,
        before_send=lambda e: None if "descarta-me" in e["message"] else e,
    )
    faro.info("guarda-me")
    faro.info("descarta-me")
    faro.info("también guarda-me")
    faro.flush(timeout=2.0)

    # Espera al worker
    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and not h.received:
        time.sleep(0.05)
    assert h.received, "el server tiene que haber recibido algo"
    msgs = [log["message"] for batch in h.received for log in batch["logs"]]
    assert msgs == ["guarda-me", "también guarda-me"]


def test_before_send_puede_transformar(server):
    srv, h = server
    port = srv.server_address[1]

    def add_tag(e: dict[str, Any]) -> dict[str, Any]:
        e["attributes"]["injected"] = "yes"
        return e

    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="bs-mutate",
        install_global_handlers=False,
        flush_interval_s=0.05,
        before_send=add_tag,
    )
    faro.info("hola")
    faro.flush(timeout=2.0)

    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and not h.received:
        time.sleep(0.05)
    assert h.received[0]["logs"][0]["attributes"]["injected"] == "yes"


# -------- 4. scrubbing --------


def test_scrub_fields_redacta_por_clave(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="scrub-fields",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    faro.log(
        "INFO",
        "login",
        **{
            "user.password": "p4ssw0rd",
            "http.request.header.authorization": "Bearer abc",
            "safe.field": "visible",
        },
    )
    faro.flush(timeout=2.0)

    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and not h.received:
        time.sleep(0.05)
    attrs = h.received[0]["logs"][0]["attributes"]
    assert attrs["user.password"] == "[REDACTED]"
    assert attrs["http.request.header.authorization"] == "[REDACTED]"
    assert attrs["safe.field"] == "visible"


# -------- 5. init con opts inválidas --------


def test_init_sin_endpoint_lanza_valueerror(monkeypatch):
    # Aseguramos que tampoco haya una FARO_ENDPOINT que la "rescate".
    monkeypatch.delenv("FARO_ENDPOINT", raising=False)
    monkeypatch.delenv("FARO_TOKEN", raising=False)
    with pytest.raises(ValueError, match=r"endpoint.*token.*obligatorios"):
        faro.init(token="tk", service="s")


def test_init_sin_token_lanza_valueerror(monkeypatch):
    monkeypatch.delenv("FARO_ENDPOINT", raising=False)
    monkeypatch.delenv("FARO_TOKEN", raising=False)
    with pytest.raises(ValueError, match=r"endpoint.*token.*obligatorios"):
        faro.init(endpoint="http://x", service="s")


def test_init_acepta_env_vars(monkeypatch, server):
    """Hueco común: olvidar que init() también lee FARO_ENDPOINT/FARO_TOKEN
    del entorno. Si esa rama se rompiera, init() lanzaría ValueError aquí."""
    srv, _ = server
    port = srv.server_address[1]
    monkeypatch.setenv("FARO_ENDPOINT", f"http://127.0.0.1:{port}")
    monkeypatch.setenv("FARO_TOKEN", "tk-from-env")
    c = faro.init(service="env-test", install_global_handlers=False)
    assert c is not None  # no lanza


# -------- 6. log + flush + assert payload (shape del wire) --------


def test_payload_shape_del_wire(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="mi-token",
        service="payload-test",
        environment="prod",
        release="v1.2.3",
        attributes={"region": "eu-west-1"},
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    faro.log(
        "WARN",
        "algo raro",
        **{"http.status_code": 500, "user.id": "u42"},
    )
    faro.flush(timeout=2.0)

    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and not h.received:
        time.sleep(0.05)
    assert h.received, "el server debió recibir al menos un POST"

    body = h.received[0]
    assert body["service"] == "payload-test"
    assert isinstance(body["logs"], list) and len(body["logs"]) == 1

    entry = body["logs"][0]
    assert entry["level"] == "WARN"
    assert entry["message"] == "algo raro"
    # Timestamp ISO 8601 con TZ.
    assert "T" in entry["timestamp"]

    attrs = entry["attributes"]
    assert attrs["region"] == "eu-west-1"
    assert attrs["deployment.environment"] == "prod"
    assert attrs["service.version"] == "v1.2.3"
    # Los no-strings se serializan a JSON (números pasan tal cual).
    assert attrs["http.status_code"] == "500"
    assert attrs["user.id"] == "u42"


# -------- 7. auto-captura de excepciones (sys.excepthook) --------


def test_auto_captura_via_excepthook(server):
    """Verifica que installing global handlers registra sys.excepthook y que
    invocarlo (como haría el runtime ante una excepción no manejada) emite
    un evento ERROR con exception.type / message / stacktrace."""
    srv, h = server
    port = srv.server_address[1]

    prev_hook = sys.excepthook  # restauramos al final para no romper pytest
    try:
        faro.init(
            endpoint=f"http://127.0.0.1:{port}",
            token="tk",
            service="auto-capture-test",
            install_global_handlers=True,  # <- core del test
            flush_interval_s=0.05,
        )
        # El SDK debe haber pisado sys.excepthook.
        assert sys.excepthook is not prev_hook, "excepthook debe haber sido reemplazado"

        # Construimos una excepción real (con traceback) y la pasamos al hook.
        try:
            raise RuntimeError("¡boom sintético!")
        except RuntimeError as e:
            sys.excepthook(type(e), e, e.__traceback__)

        # Damos tiempo al worker y forzamos flush para drenar.
        faro.flush(timeout=2.0)
        deadline = time.monotonic() + 2.0
        while time.monotonic() < deadline and not h.received:
            time.sleep(0.05)

        assert h.received, "el server debió recibir el evento de auto-captura"
        entry = h.received[0]["logs"][0]
        assert entry["level"] == "ERROR"
        assert "RuntimeError" in entry["message"]
        attrs = entry["attributes"]
        assert attrs["exception.type"] == "RuntimeError"
        assert attrs["exception.message"] == "¡boom sintético!"
        assert "Traceback" in attrs["exception.stacktrace"]
    finally:
        # Importante: restaurar y dejar que el _reset_singleton del autouse haga el close.
        # Antes había un hook custom que delega al previo — devolverlo a su sitio.
        sys.excepthook = prev_hook


# -------- 8. close() graceful: no pierde eventos en cola --------


def test_close_drena_la_cola(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="close-test",
        install_global_handlers=False,
        # Intervalo grande: si no fuera por close(), estos eventos NO llegarían.
        flush_interval_s=100.0,
    )
    for i in range(7):
        faro.info(f"evento-{i}")
    # close() debe drenar el buffer y joinear al worker antes de devolver.
    faro.close(timeout=3.0)

    msgs = [log["message"] for batch in h.received for log in batch["logs"]]
    assert sorted(msgs) == [f"evento-{i}" for i in range(7)], \
        f"close() debe drenar los 7 eventos en cola; got {msgs}"


def test_track_envia_evento_a_endpoint_events(server):
    """track() escribe a /api/v1/ingest/events con la shape correcta."""
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="track-test",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    faro.track("checkout_completed", {"amount": 99.5, "currency": "USD"})
    faro.flush(timeout=2.0)

    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and not any("events" in r for r in h.received):
        time.sleep(0.05)
    batches = [r for r in h.received if "events" in r]
    assert batches, f"esperaba al menos un batch con 'events'; recibido: {h.received}"
    event = batches[0]["events"][0]
    assert event["type"] == "track"
    assert event["name"] == "checkout_completed"
    assert event["properties"] == {"amount": 99.5, "currency": "USD"}
    assert event["distinct_id"].startswith("anon_"), "pre-identify, distinct_id == anonymous_id"
    assert event["anonymous_id"] == event["distinct_id"]
    assert event["source"] == "backend"


def test_track_adjunta_trace_context(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="trace-context-test",
        install_global_handlers=False,
        flush_interval_s=0.05,
        trace_context=lambda: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
    )
    faro.track("checkout_completed")
    faro.flush(timeout=2.0)

    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and not any("events" in r for r in h.received):
        time.sleep(0.05)
    events = [e for r in h.received if "events" in r for e in r["events"]]
    assert events[0]["trace_id"] == "4bf92f3577b34da6a3ce929d0e0e4736"
    assert events[0]["span_id"] == "00f067aa0ba902b7"


def test_identify_setea_distinct_id_para_siguientes_eventos(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="identify-test",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    faro.identify("user_42", {"email": "a@b.com", "plan": "pro"})
    faro.track("after_login")
    faro.close(timeout=2.0)

    events = [e for r in h.received if "events" in r for e in r["events"]]
    identify_events = [e for e in events if e["type"] == "identify"]
    track_events = [e for e in events if e["type"] == "track"]
    assert identify_events, "debe haber un $identify"
    assert identify_events[0]["distinct_id"] == "user_42"
    assert identify_events[0]["user_properties"] == {"email": "a@b.com", "plan": "pro"}
    assert track_events, "el track tras identify debe llegar también"
    assert track_events[0]["distinct_id"] == "user_42", \
        "tras identify, distinct_id debe ser user_42 para los eventos siguientes"


def test_alias_fusiona_sesion_pre_y_post_login(server):
    """alias() lleva el anonymous_id previo y pisa distinct_id al nuevo."""
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="alias-test",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    faro.alias("anon_old", "user_99")
    faro.track("post_alias")
    faro.close(timeout=2.0)

    events = [e for r in h.received if "events" in r for e in r["events"]]
    alias_events = [e for e in events if e["type"] == "alias"]
    track_events = [e for e in events if e["type"] == "track"]
    assert alias_events
    assert alias_events[0]["anonymous_id"] == "anon_old", "alias lleva el PREV id como anonymous_id"
    assert alias_events[0]["distinct_id"] == "user_99"
    assert track_events[0]["distinct_id"] == "user_99", "tras alias, los eventos usan el nuevo id"


def test_scrub_patterns_redacta_jwt_y_apikey(server):
    srv, h = server
    port = srv.server_address[1]
    faro.init(
        endpoint=f"http://127.0.0.1:{port}",
        token="tk",
        service="scrub-patterns",
        install_global_handlers=False,
        flush_interval_s=0.05,
    )
    faro.log(
        "INFO",
        "auth con eyJabc.def.ghi y key sk-abcdefghijklmnop",
        embedded="ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    faro.flush(timeout=2.0)

    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline and not h.received:
        time.sleep(0.05)
    log = h.received[0]["logs"][0]
    assert "eyJabc" not in log["message"], "JWT redactado en message"
    assert "sk-abcdef" not in log["message"], "sk-* redactado en message"
    assert log["attributes"]["embedded"] == "[REDACTED]", "ghp_* redactado en attribute"
