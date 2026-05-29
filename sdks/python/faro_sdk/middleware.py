"""Middlewares HTTP del SDK de Faro para Python.

Dos puntos de integración por separación de tipos (WSGI vs ASGI):

* ``FaroWsgiMiddleware`` — Flask, Django, cualquier app WSGI.
* ``FaroAsgiMiddleware`` — FastAPI, Starlette, cualquier app ASGI.

Ambos crean un span ``SERVER`` por request, lo activan en el ContextManager de
OTel para que logs/spans hijos auto-correlacionen, respetan el W3C ``traceparent``
entrante, y propagan el del span actual en la respuesta saliente.

v0.2.0: estos middlewares siguen sirviendo pero son redundantes si tenés
``opentelemetry-instrumentation-flask``/``-django``/``-fastapi``/``-starlette``
instalado y activado — esa auto-instrumentación crea spans equivalentes. La
diferencia principal: estos middlewares Faro propagan ``traceparent`` también
en la RESPUESTA (no-standard pero útil para correlación cliente-servidor),
mientras que OTel sólo propaga en requests salientes.

Uso (Flask)::

    from flask import Flask
    import faro_sdk as faro
    from faro_sdk.middleware import FaroWsgiMiddleware

    faro.init(endpoint=..., token=..., service="mi-app")
    app = Flask(__name__)
    app.wsgi_app = FaroWsgiMiddleware(app.wsgi_app)

Uso (FastAPI)::

    from fastapi import FastAPI
    import faro_sdk as faro
    from faro_sdk.middleware import FaroAsgiMiddleware

    faro.init(endpoint=..., token=..., service="mi-app")
    app = FastAPI()
    app.add_middleware(FaroAsgiMiddleware)
"""

from __future__ import annotations

from typing import Any, Callable, Iterable

from . import Span, _parse_traceparent

__all__ = ["FaroWsgiMiddleware", "FaroAsgiMiddleware"]


def _current_client():
    """Lee el cliente actual de faro_sdk. Se hace lazy porque init() asigna
    el singleton a nivel de módulo después de que este archivo ya está importado."""
    import faro_sdk  # noqa: PLC0415
    return faro_sdk._client


def _span_name(method: str, path: str) -> str:
    return f"{method} {path}".strip()


def _activate_span(span: Span) -> Any:
    """Hace al span activo en el ContextManager de OTel y devuelve el token de detach."""
    try:
        from opentelemetry import context as otel_context
        from opentelemetry import trace as otel_trace
        ctx = otel_trace.set_span_in_context(span._otel)
        return otel_context.attach(ctx)
    except Exception:
        return None


def _deactivate_span(token: Any) -> None:
    if token is None:
        return
    try:
        from opentelemetry import context as otel_context
        otel_context.detach(token)
    except Exception:
        pass


class FaroWsgiMiddleware:
    """Middleware WSGI: crea un span SERVER por request.

    `app` es la aplicación WSGI a envolver. Llama a ``faro.init(...)`` antes de
    construirla — si Faro no está inicializado, el middleware pasa el request
    a `app` sin instrumentar (no rompe el flujo)."""

    def __init__(self, app: Callable[..., Any]) -> None:
        self.app = app

    def __call__(self, environ: dict[str, Any], start_response: Callable) -> Iterable[bytes]:
        client = _current_client()
        if client is None:
            return self.app(environ, start_response)

        method = environ.get("REQUEST_METHOD", "GET")
        path = environ.get("PATH_INFO", "/")
        traceparent = environ.get("HTTP_TRACEPARENT")
        parent = _parse_traceparent(traceparent) if traceparent else None

        span = client.start_span(
            name=_span_name(method, path),
            kind="SERVER",
            attributes={
                "http.method": method,
                "http.target": path + (("?" + environ["QUERY_STRING"]) if environ.get("QUERY_STRING") else ""),
                "http.route": path,
                "net.peer.ip": environ.get("REMOTE_ADDR", ""),
            },
            parent=parent if parent else ...,
        )
        token = _activate_span(span)

        status_holder: dict[str, Any] = {"code": 0}

        def wrapped_start_response(status: str, headers: list[tuple[str, str]], exc_info: Any = None) -> Any:
            try:
                status_holder["code"] = int(status.split(" ", 1)[0])
            except (ValueError, IndexError):
                status_holder["code"] = 0
            # Propaga traceparent del span actual a quien sea downstream.
            headers.append(("traceparent", span.traceparent()))
            return start_response(status, headers, exc_info)

        try:
            result = self.app(environ, wrapped_start_response)
            # Si la app es un generador, consumimos lazy y cerramos al final.
            return _wrap_iterable(result, span, status_holder, token)
        except BaseException as e:
            span.record_exception(e)
            span.end()
            _deactivate_span(token)
            raise


def _wrap_iterable(
    iterable: Iterable[bytes], span: Span, status_holder: dict[str, Any], token: Any
) -> Iterable[bytes]:
    """Wrap un iterable WSGI para cerrar el span después de que se consuma."""
    try:
        for chunk in iterable:
            yield chunk
    finally:
        code = status_holder["code"]
        if code:
            span.set_attribute("http.status_code", code)
            if code >= 500:
                span.set_status("ERROR", f"HTTP {code}")
            else:
                span.set_status("OK")
        if not span.ended:
            span.end()
        _deactivate_span(token)
        # close() en el iterable original si lo expone (Flask suele hacerlo).
        closer = getattr(iterable, "close", None)
        if callable(closer):
            try:
                closer()
            except Exception:
                pass


class FaroAsgiMiddleware:
    """Middleware ASGI: crea un span SERVER por request HTTP.

    Compatible con FastAPI/Starlette. Pasa lifespan/websocket/etc al inner app
    sin instrumentarlos — el span solo se crea para scope.type == "http"."""

    def __init__(self, app: Callable[..., Any]) -> None:
        self.app = app

    async def __call__(self, scope: dict[str, Any], receive: Callable, send: Callable) -> None:
        client = _current_client()
        if scope.get("type") != "http" or client is None:
            await self.app(scope, receive, send)
            return

        method = scope.get("method", "GET")
        path = scope.get("path", "/")
        headers = {k.decode("latin-1").lower(): v.decode("latin-1") for k, v in scope.get("headers", [])}
        traceparent = headers.get("traceparent")
        parent = _parse_traceparent(traceparent) if traceparent else None

        span = client.start_span(
            name=_span_name(method, path),
            kind="SERVER",
            attributes={
                "http.method": method,
                "http.target": path + (("?" + scope["query_string"].decode("latin-1")) if scope.get("query_string") else ""),
                "http.route": path,
                "net.peer.ip": (scope.get("client") or ["", 0])[0] or "",
            },
            parent=parent if parent else ...,
        )
        token = _activate_span(span)
        status_code = 0

        async def wrapped_send(message: dict[str, Any]) -> None:
            nonlocal status_code
            if message.get("type") == "http.response.start":
                status_code = message.get("status", 0)
                # Inyecta traceparent en los headers de respuesta.
                headers_list = list(message.get("headers", []))
                headers_list.append((b"traceparent", span.traceparent().encode("latin-1")))
                message = {**message, "headers": headers_list}
            await send(message)

        try:
            await self.app(scope, receive, wrapped_send)
        except BaseException as e:
            span.record_exception(e)
            raise
        finally:
            if status_code:
                span.set_attribute("http.status_code", status_code)
                if status_code >= 500:
                    span.set_status("ERROR", f"HTTP {status_code}")
                else:
                    span.set_status("OK")
            if not span.ended:
                span.end()
            _deactivate_span(token)
