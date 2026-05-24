use axum::middleware::from_fn_with_state;
use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::auth;
use crate::openapi::ApiDoc;
use crate::state::SharedState;

pub mod alerts;
pub mod dashboard;
pub mod errors;
pub mod integrations;
pub mod logs;
pub mod metrics;
pub mod monitors;
pub mod projects;
pub mod services;
pub mod traces;
pub mod users;

pub fn router(state: SharedState) -> Router {
    // Un único router para que las rutas no choquen al anidarse. El propio middleware
    // decide si una petición necesita sesión autenticada según la ruta.
    Router::new()
        .route("/healthz", get(healthz))
        // OpenAPI + Swagger UI. `SwaggerUi::new(...).url(...)` ya monta
        // **tanto** el spec JSON en la URL pasada como el HTML en el path
        // principal, así que NO registramos `/api/v1/openapi.json` por
        // separado — duplicarlo hace panic en axum (Overlapping method route).
        .merge(SwaggerUi::new("/docs").url("/api/v1/openapi.json", ApiDoc::openapi()))
        .nest("/api/v1/ingest", crate::ingest::logs::router())
        .nest("/api/v1", auth::open_router().merge(auth::protected_router()).merge(v1_router()))
        .layer(from_fn_with_state(state.clone(), auth::require_session_mw))
        .with_state(state)
}

/// Liveness + información del protocolo wire. Los SDKs pueden hacer GET
/// aquí al init para descubrir el rango de protocolo que el backend
/// soporta y advertir al desarrollador si la versión del SDK está
/// desfasada. Ver `crate::versions` y ADR-0008.
async fn healthz() -> Json<crate::versions::HealthResponse> {
    Json(crate::versions::HealthResponse::current())
}

fn v1_router() -> Router<SharedState> {
    Router::new()
        .merge(logs::router())
        .merge(traces::router())
        .merge(metrics::router())
        .merge(errors::router())
        .merge(monitors::router())
        .merge(alerts::router())
        .merge(services::router())
        .merge(dashboard::router())
        .merge(projects::router())
        .merge(users::router())
        .merge(integrations::router())
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
    pub fn de_u32_0<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
        de_num_default(d, 0u32)
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
        #[serde(default, deserialize_with = "de_u32_0")]
        pub offset: u32,
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

        pub fn project_clause(&self, alias: &str) -> String {
            match &self.project {
                Some(s) if !s.is_empty() => {
                    let col = if alias.is_empty() {
                        "project_id".to_string()
                    } else {
                        format!("{alias}.project_id")
                    };
                    format!(" AND {col} = '{}'", escape_sql(s))
                }
                _ => String::new(),
            }
        }
    }

    pub fn escape_sql(s: &str) -> String {
        s.replace('\\', "\\\\").replace('\'', "\\'")
    }

    pub fn ch_dt(t: DateTime<Utc>) -> String {
        t.format("%Y-%m-%d %H:%M:%S%.9f").to_string()
    }
}
