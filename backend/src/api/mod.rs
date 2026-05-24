use axum::middleware::from_fn_with_state;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use bytes::Bytes;
use tower_http::compression::CompressionLayer;
use utoipa::OpenApi;

use crate::auth;
use crate::openapi::ApiDoc;
use crate::state::SharedState;

/// HTML estático que monta [Scalar](https://github.com/scalar/scalar) leyendo
/// el spec en `/api/v1/openapi.json`. Reemplaza a `utoipa-swagger-ui`: misma
/// data, UI moderna estilo Stripe/Vercel/Resend (three-pane con sidebar +
/// try-it-out), sin un `build.rs` que descargue el bundle de Swagger por curl.
/// El CDN de jsdelivr está cubierto por `SCALAR_CSP` en `security.rs`.
const SCALAR_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Faro API · Reference</title>
</head>
<body>
  <script id="api-reference" data-url="/api/v1/openapi.json"></script>
  <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference@1"></script>
</body>
</html>
"#;

pub mod account;
pub mod alerts;
pub mod cohorts;
pub mod dashboard;
pub mod errors;
pub mod events;
pub mod experiments;
pub mod feature_flags;
pub mod funnels;
pub mod health;
pub mod insights;
pub mod integrations;
pub mod logs;
pub mod metrics;
pub mod monitors;
pub mod preferences;
pub mod product_users;
pub mod projects;
pub mod replays;
pub mod retention;
pub mod security;
pub mod services;
pub mod traces;
pub mod users;

pub fn router(state: SharedState) -> Router {
    let enable_hsts = state.cfg.enable_hsts;

    // Dashboard: healthz/readyz + JSON API. Pasa por el middleware de auth (que
    // exime /healthz, /readyz, /metrics y /api/v1/auth/login según `is_public_path`).
    // Los security headers se aplican POR FUERA de auth — así las respuestas 401
    // de sesiones inválidas también las llevan (un browser que cae en
    // /api/v1/something sin cookie recibe CSP/X-Frame-Options aunque nunca llegue
    // al handler).
    let dashboard: Router<SharedState> = Router::new()
        .merge(health::router())
        .nest(
            "/api/v1",
            auth::open_router()
                .merge(auth::protected_router())
                .merge(v1_router()),
        )
        .layer(from_fn_with_state(state.clone(), auth::require_session_mw));
    let dashboard = security::apply_dashboard_headers(dashboard, enable_hsts);

    // OpenAPI + Scalar (referencia pública). Pre-serializamos el spec una vez
    // al boot y lo servimos como bytes — el JSON no cambia entre requests y
    // así evitamos re-serializar en cada hit. El bundle de Scalar lo carga el
    // browser desde jsdelivr; ver `SCALAR_HTML` y `SCALAR_CSP` para detalles.
    let openapi_json: Bytes = Bytes::from(
        ApiDoc::openapi()
            .to_json()
            .expect("OpenAPI spec debe serializar (programming error si falla)"),
    );
    let docs: Router<SharedState> = Router::new()
        .route("/docs", get(|| async { Html(SCALAR_HTML) }))
        .route(
            "/api/v1/openapi.json",
            get(move || {
                let body = openapi_json.clone();
                async move {
                    (
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        body,
                    )
                }
            }),
        );
    let docs = security::apply_docs_headers(docs, enable_hsts);

    // Ingest: SDKs no-browser. Sin auth de sesión (tienen su propio bearer token
    // por proyecto), sin security headers (bytes desperdiciados en un endpoint
    // que recibe miles de POSTs/segundo y nunca devuelve HTML a un browser).
    let ingest: Router<SharedState> = Router::new().nest(
        "/api/v1/ingest",
        crate::ingest::logs::router()
            .merge(crate::ingest::events::router())
            .merge(crate::ingest::replay::router())
            .merge(feature_flags::router()),
    );

    Router::new()
        .merge(dashboard)
        .merge(docs)
        .merge(ingest)
        // Compresión negociada por Accept-Encoding (br > gzip). El predicado por
        // defecto excluye `text/event-stream` y respuestas <32B, así que el SSE
        // de logs en vivo sigue fluyendo sin buffering. Reduce ~70-80% el tráfico
        // de las queries de logs/traces, que rutinariamente devuelven 500KB+.
        .layer(CompressionLayer::new().gzip(true).br(true))
        .with_state(state)
}

fn v1_router() -> Router<SharedState> {
    Router::new()
        .merge(logs::router())
        .merge(traces::router())
        .merge(metrics::router())
        .merge(errors::router())
        .merge(events::router())
        .merge(experiments::router())
        .merge(funnels::router())
        .merge(insights::router())
        .merge(cohorts::router())
        .merge(monitors::router())
        .merge(alerts::router())
        .merge(services::router())
        .merge(dashboard::router())
        .merge(projects::router())
        .merge(users::router())
        .merge(integrations::router())
        .merge(preferences::router())
        .merge(product_users::router())
        .merge(retention::router())
        .merge(replays::router())
        .merge(account::router())
}

/// Parsea los parámetros comunes de rango de tiempo / paginación usados en los endpoints de consulta.
pub mod params {
    use std::fmt::Display;
    use std::str::FromStr;

    use chrono::{DateTime, Duration, Utc};
    use serde::de::{Deserializer, Error};
    use serde::Deserialize;

    /// Acepta tanto un número como un número codificado en string para los query params.
    pub fn de_opt_num<'de, T, D>(d: D) -> Result<Option<T>, D::Error>
    where
        T: FromStr + Deserialize<'de>,
        T::Err: Display,
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum NumOrStr<T> {
            Num(T),
            Str(String),
        }

        match Option::<NumOrStr<T>>::deserialize(d)? {
            None => Ok(None),
            Some(NumOrStr::Num(n)) => Ok(Some(n)),
            Some(NumOrStr::Str(s)) if s.is_empty() => Ok(None),
            Some(NumOrStr::Str(s)) => s.parse().map(Some).map_err(Error::custom),
        }
    }

    pub fn de_num_default<'de, T, D>(d: D, default: T) -> Result<T, D::Error>
    where
        T: FromStr + Deserialize<'de>,
        T::Err: Display,
        D: Deserializer<'de>,
    {
        Ok(de_opt_num(d)?.unwrap_or(default))
    }

    pub fn de_u32_200<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
        de_num_default(d, 200u32)
    }

    #[derive(Debug, Deserialize, Default)]
    pub struct Range {
        #[serde(default)]
        pub from: Option<DateTime<Utc>>,
        #[serde(default)]
        pub to: Option<DateTime<Utc>>,
        #[serde(default, deserialize_with = "de_opt_num")]
        pub last_minutes: Option<i64>,
        #[serde(default = "default_limit", deserialize_with = "de_u32_200")]
        pub limit: u32,
        /// Cursor para paginación keyset. Es el timestamp del último item de la
        /// página anterior; el backend filtra `WHERE <column> < cursor` antes del
        /// LIMIT. Reemplaza al viejo `offset` que escaneaba N+offset filas en
        /// ClickHouse (O(n)); con cursor es O(log n) por el índice de timestamp.
        #[serde(default)]
        pub cursor: Option<DateTime<Utc>>,
        #[serde(default)]
        pub project: Option<String>,
    }

    fn default_limit() -> u32 {
        200
    }

    impl Range {
        pub fn resolve(&self) -> (DateTime<Utc>, DateTime<Utc>) {
            let to = self.to.unwrap_or_else(Utc::now);
            let from = if let Some(f) = self.from {
                f
            } else if let Some(m) = self.last_minutes {
                to - Duration::minutes(m)
            } else {
                to - Duration::hours(1)
            };
            (from, to)
        }

        pub fn limit(&self) -> u32 {
            self.limit.min(10_000).max(1)
        }

        /// Devuelve la cláusula `AND project_id = {project:String}` lista para concatenar al WHERE
        /// y el valor a registrar como parámetro `project` (vacío si no hay filtro). El binding
        /// del valor lo hace ClickHouse del lado servidor, sin interpolación de strings.
        ///
        /// Si el caller necesita registrar el parámetro `project` con otro nombre (porque ya hay
        /// colisión en el mismo query), usar `project_clause_named` en su lugar.
        pub fn project_clause(&self, alias: &str) -> (String, Option<&str>) {
            self.project_clause_named(alias, "project")
        }

        pub fn project_clause_named<'a>(
            &'a self,
            alias: &str,
            param_name: &str,
        ) -> (String, Option<&'a str>) {
            match &self.project {
                Some(s) if !s.is_empty() => {
                    let col = if alias.is_empty() {
                        "project_id".to_string()
                    } else {
                        format!("{alias}.project_id")
                    };
                    (
                        format!(" AND {col} = {{{param_name}:String}}"),
                        Some(s.as_str()),
                    )
                }
                _ => (String::new(), None),
            }
        }

        /// Devuelve la cláusula `AND <column> < {cursor:DateTime64(9)}` y el valor a
        /// registrar como parámetro `cursor` ya formateado para ClickHouse. Si no hay
        /// cursor en el request, ambas posiciones son vacías. Pensado para endpoints
        /// con `ORDER BY <column> DESC LIMIT N` que quieren paginación keyset.
        ///
        /// `column` se interpola directo en el SQL — siempre debe ser un literal
        /// del código, NUNCA input del usuario, o se vuelve un vector de SQL injection.
        ///
        /// Limitación conocida (timestamps duplicados): si dos filas comparten
        /// exactamente el mismo `DateTime64(9)` y caen en el borde de página
        /// (unas en la página N, otras en la N+1), las de la N+1 se pierden
        /// porque usamos `<` estricto. En la práctica las colisiones a
        /// nanosegundos son raras (el ingest usa `now64(9)` y los SDKs llegan
        /// con su propio reloj cliente), pero pasan en bursts. Si se vuelve un
        /// problema, el fix es un cursor compuesto `(timestamp, id)` y comparar
        /// como tupla — más complejo y no lo necesitamos hoy.
        pub fn cursor_clause(&self, column: &str) -> (String, Option<String>) {
            match self.cursor {
                Some(c) => (
                    format!(" AND {column} < {{cursor:DateTime64(9)}}"),
                    Some(ch_dt(c)),
                ),
                None => (String::new(), None),
            }
        }
    }

    pub fn ch_dt(t: DateTime<Utc>) -> String {
        t.format("%Y-%m-%d %H:%M:%S%.9f").to_string()
    }
}
