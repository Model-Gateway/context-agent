//! Token estimation.
//!
//! Exact tokenizers are model-specific and not discoverable through
//! OpenAI-compatible endpoints. The runtime therefore uses a conservative
//! heuristic estimator by default and lets providers report actual usage.

/// Estimates the number of tokens a text will occupy.
pub trait TokenEstimator: Send + Sync {
    fn estimate(&self, text: &str) -> usize;

    fn name(&self) -> &str;
}

/// Heuristic: source code and English both average around 3.5-4 characters
/// per token for common BPE tokenizers. We bias slightly high so budgets err
/// on the side of leaving headroom.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeuristicEstimator;

impl HeuristicEstimator {
    pub fn estimate_str(text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }
        let chars = text.chars().count();
        let whitespace = text.chars().filter(|c| c.is_whitespace()).count();
        let punctuation = text.chars().filter(|c| c.is_ascii_punctuation()).count();
        // Each punctuation character tends to be its own token; words average
        // ~4.5 characters per token.
        let base = (chars - whitespace - punctuation) as f64 / 4.2;
        let est = base + punctuation as f64 * 0.8 + whitespace as f64 * 0.15;
        est.ceil().max(1.0) as usize
    }
}

impl TokenEstimator for HeuristicEstimator {
    fn estimate(&self, text: &str) -> usize {
        Self::estimate_str(text)
    }

    fn name(&self) -> &str {
        "heuristic"
    }
}

/// Convenience wrapper around the default estimator.
pub fn estimate_tokens(text: &str) -> usize {
    HeuristicEstimator::estimate_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_scale_with_length() {
        let short = estimate_tokens("fn main() {}");
        let long = estimate_tokens(&"fn main() {}\n".repeat(50));
        assert!(short > 0);
        assert!(long > short * 30);
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }
}
