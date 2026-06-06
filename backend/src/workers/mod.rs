//! Workers en segundo plano: tareas tokio que corren fuera del ciclo request/response.
//!
//! Cada `start_*` lanza un bucle independiente: escritura por lotes a ClickHouse
//! (`ingest_writer`), indexado de errores (`error_indexer`), evaluación de alertas
//! (`alert_evaluator`), ejecución de monitores (`monitor_runner`), detección de
//! anomalías y de rollback de feature flags, compactación de fingerprints,
//! agregación de sesiones, detección de servicios "stale" y unificación de usuarios.

pub mod alert_evaluator;
pub mod anomaly_detector;
pub mod error_indexer;
pub mod feature_rollback_detector;
pub mod fingerprint_compactor;
pub mod ingest_writer;
pub mod monitor_runner;
pub mod session_aggregator;
pub mod stale_detector;
pub mod user_unifier;

pub use alert_evaluator::start_alert_evaluator;
pub use anomaly_detector::start_anomaly_detector;
pub use error_indexer::start_error_indexer;
pub use feature_rollback_detector::start_feature_rollback_detector;
pub use fingerprint_compactor::start_fingerprint_compactor;
pub use ingest_writer::start_ingest_writers;
pub use monitor_runner::start_monitor_runner;
pub use session_aggregator::start_session_aggregator;
pub use stale_detector::start_stale_detector;
pub use user_unifier::start_user_unifier;
