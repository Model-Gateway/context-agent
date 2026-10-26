//! Observability (spec §14): the persistent event log, task metrics, and
//! tracing initialization.

pub mod events;
pub mod metrics;
pub mod tracing_setup;

pub use events::EventLog;
pub use metrics::{MetricsCollector, TaskMetrics};
pub use tracing_setup::{init_tracing, TracingTarget};
