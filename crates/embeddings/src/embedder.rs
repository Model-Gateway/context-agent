//! Embedding models.

use ctx_core::{CtxError, EmbeddingBackend, EmbeddingConfig, Result};
use std::sync::Arc;

pub trait Embedder: Send + Sync {
    /// Identifier persisted with vectors so model changes trigger re-embedding.
    fn model_id(&self) -> &str;

    fn dimension(&self) -> usize;

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;

    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = self.embed(&[text])?;
        v.pop().ok_or_else(|| CtxError::other("embedder returned no vector"))
    }
}

/// Deterministic feature-hashing embedder.
///
/// Features: normalized identifier parts, their bigrams, and character
/// trigrams of each part, hashed into `dimension` buckets with signed
/// weights and L2-normalized. Documents and queries share the same feature
/// space, so similar identifiers and phrasing land near each other.
#[derive(Debug, Clone)]
pub struct HashingEmbedder {
    dimension: usize,
    model_id: String,
}

impl HashingEmbedder {
    pub fn new(dimension: usize) -> Self {
        let dimension = dimension.clamp(64, 4096);
        Self { dimension, model_id: format!("hashing-v1-d{dimension}") }
    }

    fn fnv1a(bytes: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    fn add_feature(&self, vec: &mut [f32], feature: &str, weight: f32) {
        let h = Self::fnv1a(feature.as_bytes());
        let bucket = (h % self.dimension as u64) as usize;
        let sign = if (h >> 63) & 1 == 1 { -1.0 } else { 1.0 };
        vec[bucket] += sign * weight;
    }

    /// Split text into lowercase word parts (camelCase/snake_case aware).
    pub fn tokenize(text: &str) -> Vec<String> {
        let mut parts = Vec::new();
        for raw in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if raw.len() < 2 {
                continue;
            }
            let chars: Vec<char> = raw.chars().collect();
            let mut current = String::new();
            for (i, &c) in chars.iter().enumerate() {
                if c == '_' {
                    if !current.is_empty() {
                        parts.push(std::mem::take(&mut current));
                    }
                    continue;
                }
                if !current.is_empty() && c.is_uppercase() {
                    let prev = chars[i - 1];
                    let next_lower = chars.get(i + 1).map(|n| n.is_lowercase()).unwrap_or(false);
                    if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                        parts.push(std::mem::take(&mut current));
                    }
                }
                current.push(c);
            }
            if !current.is_empty() {
                parts.push(current);
            }
        }
        parts.into_iter().map(|p| p.to_lowercase()).filter(|p| p.len() >= 2 && !STOPWORDS.contains(&p.as_str())).collect()
    }

    fn embed_text(&self, text: &str) -> Vec<f32> {
        let mut vec = vec![0.0f32; self.dimension];
        let tokens = Self::tokenize(text);
        if tokens.is_empty() {
            return vec;
        }
        // Term frequencies with sublinear scaling.
        let mut counts: std::collections::HashMap<&str, f32> = std::collections::HashMap::new();
        for t in &tokens {
            *counts.entry(t.as_str()).or_insert(0.0) += 1.0;
        }
        for (tok, count) in &counts {
            let w = 1.0 + count.ln();
            self.add_feature(&mut vec, &format!("w:{tok}"), w);
            // Character trigrams give robustness to small spelling variants.
            let chars: Vec<char> = tok.chars().collect();
            if chars.len() >= 4 {
                for win in chars.windows(3) {
                    let tri: String = win.iter().collect();
                    self.add_feature(&mut vec, &format!("c:{tri}"), 0.25 * w);
                }
            }
        }
        for pair in tokens.windows(2) {
            self.add_feature(&mut vec, &format!("b:{}|{}", pair[0], pair[1]), 0.6);
        }
        normalize(&mut vec);
        vec
    }
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "into", "are", "was", "were", "has", "have", "not", "but", "its",
    "use", "let", "var", "const", "fn", "def", "end", "return", "pub", "self", "new", "null", "none", "true", "false",
    "import", "export", "class", "function", "void", "int", "string", "struct", "impl", "mod", "type",
];

fn normalize(vec: &mut [f32]) {
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in vec.iter_mut() {
            *x /= norm;
        }
    }
}

impl Embedder for HashingEmbedder {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.embed_text(t)).collect())
    }
}

/// Construct the configured embedder. Returns `None` when semantic
/// retrieval is disabled.
pub fn build_embedder(config: &EmbeddingConfig) -> Result<Option<Arc<dyn Embedder>>> {
    match config.backend {
        EmbeddingBackend::None => Ok(None),
        EmbeddingBackend::Hashing => Ok(Some(Arc::new(HashingEmbedder::new(config.dimension)))),
        EmbeddingBackend::Onnx => Err(CtxError::Config(
            "the onnx embedding backend is not compiled into this build; use backend = \"hashing\" or \"none\"".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn similar_identifiers_are_close() {
        let e = HashingEmbedder::new(256);
        let login = e.embed_one("fn handleLoginClick(user) { showSpinner(); authenticate(user) }").unwrap();
        let login_q = e.embed_one("login button spinner").unwrap();
        let footer = e.embed_one("fn renderFooter(year) { copyright(year) }").unwrap();
        assert!(cosine(&login, &login_q) > cosine(&footer, &login_q));
        assert!((cosine(&login, &login) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn deterministic_and_normalized() {
        let e = HashingEmbedder::new(128);
        let a = e.embed_one("parse JSON value").unwrap();
        let b = e.embed_one("parse JSON value").unwrap();
        assert_eq!(a, b);
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
        assert!(e.embed_one("").unwrap().iter().all(|x| *x == 0.0));
    }

    #[test]
    fn tokenizer_splits_identifiers() {
        assert_eq!(HashingEmbedder::tokenize("getUserName HTTP_SERVER"), vec!["get", "user", "name", "http", "server"]);
    }
}
