//! Capa de almacenamiento: cliente de ClickHouse y modelos de fila.
//!
//! Reexporta el `Client` (HTTP sobre ClickHouse) y los structs de fila (`LogRow`,
//! `SpanRow`, `MetricRow`, `ProductEventRow`, `AlertRuleRow`, …) que mapean las
//! tablas de `faro.*`.

pub mod client;
pub mod models;

pub use client::Client;
pub use models::de_dt;
pub use models::*;
