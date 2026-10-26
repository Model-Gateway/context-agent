//! Benchmark framework (spec §15).
//!
//! Tasks are TOML files; variants A–E share one runtime and differ only in
//! retrieval and history policy; every run snapshots the fixture, runs the
//! agent, re-runs the task's validation commands independently, and
//! records metrics. Reports carry sample counts and Wilson confidence
//! intervals.

pub mod report;
pub mod retrieval_eval;
pub mod runner;
pub mod spec;
pub mod stats;
pub mod variant;

pub use report::{BenchmarkReport, TaskRunResult, VariantSummary};
pub use retrieval_eval::{evaluate_retrieval, RetrievalEvalReport};
pub use runner::{run_benchmark, BenchmarkOptions, ProviderFactory};
pub use spec::{load_tasks, BenchmarkTask, RetrievalQuery, Turn};
pub use stats::wilson_interval;
pub use variant::Variant;
