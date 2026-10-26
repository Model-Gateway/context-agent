//! Exact nearest-neighbour index over normalized vectors.

use ctx_storage::EmbeddingRecord;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct VectorMatch {
    pub id: String,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct VectorIndex {
    model_id: String,
    dimension: usize,
    ids: Vec<String>,
    hashes: Vec<String>,
    data: Vec<f32>,
    positions: HashMap<String, usize>,
    by_hash: HashMap<String, usize>,
}

impl VectorIndex {
    pub fn new(model_id: &str, dimension: usize) -> Self {
        Self { model_id: model_id.to_string(), dimension, ids: Vec::new(), hashes: Vec::new(), data: Vec::new(), positions: HashMap::new(), by_hash: HashMap::new() }
    }

    pub fn from_records(model_id: &str, dimension: usize, records: Vec<EmbeddingRecord>) -> Self {
        let mut idx = Self::new(model_id, dimension);
        for r in records {
            if r.model_id == model_id && r.vector.len() == dimension {
                idx.insert(&r.chunk_id, &r.content_hash, &r.vector);
            }
        }
        idx
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.positions.contains_key(id)
    }

    pub fn hash_of(&self, id: &str) -> Option<&str> {
        self.positions.get(id).map(|&p| self.hashes[p].as_str())
    }

    /// A vector previously computed for identical content, if any.
    pub fn vector_for_hash(&self, content_hash: &str) -> Option<Vec<f32>> {
        self.by_hash.get(content_hash).map(|&p| self.row(p).to_vec())
    }

    pub fn vector_of(&self, id: &str) -> Option<&[f32]> {
        self.positions.get(id).map(|&p| self.row(p))
    }

    fn row(&self, p: usize) -> &[f32] {
        &self.data[p * self.dimension..(p + 1) * self.dimension]
    }

    pub fn insert(&mut self, id: &str, content_hash: &str, vector: &[f32]) {
        assert_eq!(vector.len(), self.dimension, "vector dimension mismatch");
        let mut v = vector.to_vec();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 && (norm - 1.0).abs() > 1e-4 {
            v.iter_mut().for_each(|x| *x /= norm);
        }
        if let Some(&p) = self.positions.get(id) {
            let old_hash = std::mem::replace(&mut self.hashes[p], content_hash.to_string());
            if self.by_hash.get(&old_hash) == Some(&p) {
                self.by_hash.remove(&old_hash);
            }
            self.data[p * self.dimension..(p + 1) * self.dimension].copy_from_slice(&v);
            self.by_hash.insert(content_hash.to_string(), p);
            return;
        }
        let p = self.ids.len();
        self.ids.push(id.to_string());
        self.hashes.push(content_hash.to_string());
        self.data.extend_from_slice(&v);
        self.positions.insert(id.to_string(), p);
        self.by_hash.entry(content_hash.to_string()).or_insert(p);
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let Some(p) = self.positions.remove(id) else { return false };
        let last = self.ids.len() - 1;
        let removed_hash = self.hashes[p].clone();
        if self.by_hash.get(&removed_hash) == Some(&p) {
            self.by_hash.remove(&removed_hash);
        }
        if p != last {
            let moved_id = self.ids[last].clone();
            self.ids.swap(p, last);
            self.hashes.swap(p, last);
            let (head, tail) = self.data.split_at_mut(last * self.dimension);
            head[p * self.dimension..(p + 1) * self.dimension].copy_from_slice(&tail[..self.dimension]);
            self.positions.insert(moved_id, p);
            let moved_hash = self.hashes[p].clone();
            if self.by_hash.get(&moved_hash) == Some(&last) {
                self.by_hash.insert(moved_hash, p);
            }
        }
        self.ids.pop();
        self.hashes.pop();
        self.data.truncate(last * self.dimension);
        true
    }

    /// Exact cosine search; `query` is normalized internally.
    pub fn search(&self, query: &[f32], k: usize) -> Vec<VectorMatch> {
        if query.len() != self.dimension || self.is_empty() || k == 0 {
            return Vec::new();
        }
        let norm: f32 = query.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm == 0.0 {
            return Vec::new();
        }
        let q: Vec<f32> = query.iter().map(|x| x / norm).collect();
        let mut scored: Vec<(usize, f32)> = (0..self.ids.len())
            .map(|p| {
                let row = self.row(p);
                let dot: f32 = row.iter().zip(&q).map(|(a, b)| a * b).sum();
                (p, dot)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored.into_iter().map(|(p, score)| VectorMatch { id: self.ids[p].clone(), score }).collect()
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.ids.iter().map(|s| s.as_str())
    }

    pub fn to_records(&self) -> Vec<EmbeddingRecord> {
        (0..self.ids.len())
            .map(|p| EmbeddingRecord { chunk_id: self.ids[p].clone(), content_hash: self.hashes[p].clone(), model_id: self.model_id.clone(), vector: self.row(p).to_vec() })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_search_remove() {
        let mut idx = VectorIndex::new("m", 3);
        idx.insert("a", "h1", &[1.0, 0.0, 0.0]);
        idx.insert("b", "h2", &[0.0, 1.0, 0.0]);
        idx.insert("c", "h3", &[0.7, 0.7, 0.0]);
        let top = idx.search(&[1.0, 0.1, 0.0], 2);
        assert_eq!(top[0].id, "a");
        assert_eq!(top[1].id, "c");
        assert!(idx.remove("a"));
        assert!(!idx.contains("a"));
        assert_eq!(idx.len(), 2);
        // "c" moved into a's slot; lookups must still work.
        assert_eq!(idx.search(&[1.0, 1.0, 0.0], 1)[0].id, "c");
        assert_eq!(idx.vector_for_hash("h3").map(|v| v.len()), Some(3));
        assert!(idx.vector_for_hash("h1").is_none());
        idx.insert("b", "h2b", &[0.0, 0.0, 1.0]);
        assert_eq!(idx.hash_of("b"), Some("h2b"));
        assert!(idx.vector_for_hash("h2").is_none());
    }
}
