//! The semantic service interface used by tools.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub path: String,
    pub line: usize,
    pub column: usize,
}

/// Result of a semantic query: either answers or an explicit reason why the
/// semantic level is unavailable, so callers can fall back to structural or
/// lexical retrieval without guessing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticOutcome {
    Found(Vec<Location>),
    Unavailable { reason: String },
}

#[async_trait]
pub trait SemanticService: Send + Sync {
    async fn definition(&self, path: &str, line: usize, column: usize) -> SemanticOutcome;
    async fn references(&self, path: &str, line: usize, column: usize) -> SemanticOutcome;
    fn describe(&self) -> String;
}

/// Default service when no language server is running.
pub struct NullSemanticService;

#[async_trait]
impl SemanticService for NullSemanticService {
    async fn definition(&self, _path: &str, _line: usize, _column: usize) -> SemanticOutcome {
        SemanticOutcome::Unavailable { reason: "no language server configured".into() }
    }

    async fn references(&self, _path: &str, _line: usize, _column: usize) -> SemanticOutcome {
        SemanticOutcome::Unavailable { reason: "no language server configured".into() }
    }

    fn describe(&self) -> String {
        "semantic level: unavailable (parser and lexical levels active)".into()
    }
}
