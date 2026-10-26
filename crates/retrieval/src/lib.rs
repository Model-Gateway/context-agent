//! Retrieval engine (spec §7).
//!
//! A hybrid pipeline: deterministic query analysis, parallel path / lexical
//! / symbol / semantic / memory strategies, reciprocal-rank fusion with
//! code-specific signals, bounded relationship expansion, freshness
//! validation against the working tree, budgeted selection, and a
//! calibrated confidence assessment. Every stage records diagnostics so an
//! engineer can answer "why did the agent receive this file?".

pub mod engine;
pub mod expansion;
pub mod fusion;
pub mod query;
pub mod strategies;
pub mod types;

pub use engine::{RetrievalEngine, RetrievalRequest};
pub use fusion::{RankContext, Ranker, RrfRanker};
pub use query::{analyze_query, QueryAnalysis};
pub use types::*;
