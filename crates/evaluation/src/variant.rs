//! Experimental variants (spec §15.2).

use ctx_agent::RunOptions;
use ctx_core::{Config, EmbeddingBackend};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Variant {
    /// Model-driven tools, conventional retained history.
    A,
    /// Model-driven tools, task-aware external memory and compaction.
    B,
    /// Proactive lexical + symbol retrieval.
    C,
    /// Full hybrid (lexical + semantic + structural).
    D,
    /// Hybrid + progressive investigation and adaptive context.
    E,
}

impl Variant {
    pub fn all() -> [Variant; 5] {
        [Variant::A, Variant::B, Variant::C, Variant::D, Variant::E]
    }

    pub fn parse(s: &str) -> Option<Variant> {
        match s.trim().to_ascii_uppercase().as_str() {
            "A" | "BASELINE" => Some(Variant::A),
            "B" | "COMPACTION" => Some(Variant::B),
            "C" | "PROACTIVE" => Some(Variant::C),
            "D" | "HYBRID" => Some(Variant::D),
            "E" | "ADAPTIVE" => Some(Variant::E),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Variant::A => "A-baseline",
            Variant::B => "B-compaction",
            Variant::C => "C-proactive",
            Variant::D => "D-hybrid",
            Variant::E => "E-adaptive",
        }
    }

    pub fn options(&self) -> RunOptions {
        match self {
            Variant::A => RunOptions { proactive_retrieval: false, compaction: false, memory_context: false, adaptive: false, max_steps: None, stream: false, ..RunOptions::default() },
            Variant::B => RunOptions { proactive_retrieval: false, compaction: true, memory_context: true, adaptive: false, max_steps: None, stream: false, ..RunOptions::default() },
            Variant::C => RunOptions { proactive_retrieval: true, compaction: true, memory_context: true, adaptive: false, max_steps: None, stream: false, ..RunOptions::default() },
            Variant::D => RunOptions { proactive_retrieval: true, compaction: true, memory_context: true, adaptive: false, max_steps: None, stream: false, ..RunOptions::default() },
            Variant::E => RunOptions { proactive_retrieval: true, compaction: true, memory_context: true, adaptive: true, max_steps: None, stream: false, ..RunOptions::default() },
        }
    }

    /// Configuration adjustments: semantic retrieval only for D and E.
    pub fn config(&self, base: &Config) -> Config {
        let mut cfg = base.clone();
        cfg.embeddings.backend = match self {
            Variant::D | Variant::E => if base.embeddings.backend == EmbeddingBackend::None { EmbeddingBackend::Hashing } else { base.embeddings.backend },
            _ => EmbeddingBackend::None,
        };
        if matches!(self, Variant::A) {
            // Conventional retained history: no compaction pressure.
            cfg.context.compaction_threshold = 0.98;
            cfg.context.keep_recent_turns = 1_000;
        }
        cfg
    }
}
