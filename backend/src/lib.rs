//! Faro backend library entrypoint.
//!
//! Existe para que `tests/*.rs` (integration tests) puedan importar los módulos
//! del backend con `use faro::*`. El binario `faro` (en `src/main.rs`) consume
//! exactamente los mismos módulos vía este crate.

pub mod alert_query;
pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod feature_flags;
pub mod fingerprint;
pub mod ingest;
pub mod integrations;
pub mod minhash;
pub mod monitor_url;
pub mod notification_channels;
pub mod notify;
pub mod observability;
pub mod openapi;
pub mod origin_check;
pub mod projects;
pub mod redaction;
pub mod state;
pub mod storage;
pub mod stream;
pub mod telemetry;
pub mod totp;
pub mod versions;
pub mod workers;
