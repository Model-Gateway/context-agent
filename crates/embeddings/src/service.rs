//! Keeps embeddings in sync with chunks and answers semantic queries.

use crate::embedder::Embedder;
use crate::index::{VectorIndex, VectorMatch};
use ctx_core::Result;
use ctx_storage::{EmbeddingRecord, IndexStore};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshStats {
    pub embedded: usize,
    pub reused: usize,
    pub unchanged: usize,
    pub removed: usize,
}

pub struct EmbeddingService {
    store: Arc<IndexStore>,
    embedder: Arc<dyn Embedder>,
    index: RwLock<VectorIndex>,
}

impl EmbeddingService {
    /// Load persisted vectors for the configured model; vectors from other
    /// models are discarded (they are regenerated on the next refresh).
    pub fn load(store: Arc<IndexStore>, embedder: Arc<dyn Embedder>) -> Result<Self> {
        let model_id = embedder.model_id().to_string();
        let removed = store.delete_embeddings_except_model(&model_id)?;
        if removed > 0 {
            tracing::info!(removed, model = %model_id, "dropped embeddings from a different model");
        }
        let records = store.load_embeddings(&model_id)?;
        let index = VectorIndex::from_records(&model_id, embedder.dimension(), records);
        store.set_meta("embedding_model", &model_id)?;
        Ok(Self { store, embedder, index: RwLock::new(index) })
    }

    pub fn model_id(&self) -> &str {
        self.embedder.model_id()
    }

    pub fn len(&self) -> usize {
        self.index.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Ensure every `(chunk_id, content_hash, text)` has a current vector.
    /// Unchanged chunks are skipped; identical content under a new id reuses
    /// the existing vector; everything else is embedded in batches.
    pub fn refresh(&self, chunks: &[(String, String, String)]) -> Result<RefreshStats> {
        let mut stats = RefreshStats::default();
        let mut to_embed: Vec<(String, String, String)> = Vec::new();
        let mut reused: Vec<EmbeddingRecord> = Vec::new();
        {
            let index = self.index.read();
            for (id, hash, text) in chunks {
                if index.hash_of(id) == Some(hash.as_str()) {
                    stats.unchanged += 1;
                    continue;
                }
                if let Some(vec) = index.vector_for_hash(hash) {
                    reused.push(EmbeddingRecord { chunk_id: id.clone(), content_hash: hash.clone(), model_id: self.model_id().to_string(), vector: vec });
                    continue;
                }
                to_embed.push((id.clone(), hash.clone(), text.clone()));
            }
        }
        stats.reused = reused.len();
        let mut new_records = reused;
        for batch in to_embed.chunks(64) {
            let texts: Vec<&str> = batch.iter().map(|(_, _, t)| t.as_str()).collect();
            let vectors = self.embedder.embed(&texts)?;
            for ((id, hash, _), vector) in batch.iter().zip(vectors) {
                new_records.push(EmbeddingRecord { chunk_id: id.clone(), content_hash: hash.clone(), model_id: self.model_id().to_string(), vector });
                stats.embedded += 1;
            }
        }
        if !new_records.is_empty() {
            self.store.upsert_embeddings(&new_records)?;
            let mut index = self.index.write();
            for r in &new_records {
                index.insert(&r.chunk_id, &r.content_hash, &r.vector);
            }
        }
        Ok(stats)
    }

    /// Drop vectors for chunks that no longer exist.
    pub fn retain(&self, live_ids: &HashSet<String>) -> Result<usize> {
        let stale: Vec<String> = self.index.read().ids().filter(|id| !live_ids.contains(*id)).map(|s| s.to_string()).collect();
        if stale.is_empty() {
            return Ok(0);
        }
        {
            let mut index = self.index.write();
            for id in &stale {
                index.remove(id);
            }
        }
        self.store.delete_embeddings(&stale)?;
        Ok(stale.len())
    }

    /// Whether a vector exists for this chunk id.
    pub fn search_cache_contains(&self, chunk_id: &str) -> bool {
        self.index.read().contains(chunk_id)
    }

    pub fn search(&self, query: &str, k: usize) -> Result<Vec<VectorMatch>> {
        let q = self.embedder.embed_one(query)?;
        Ok(self.index.read().search(&q, k))
    }

    /// Discard everything (index rebuild).
    pub fn clear(&self) -> Result<()> {
        *self.index.write() = VectorIndex::new(self.model_id(), self.embedder.dimension());
        self.store.delete_embeddings_except_model("__none__")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedder::HashingEmbedder;

    #[test]
    fn refresh_reuses_and_prunes() {
        let store = Arc::new(IndexStore::in_memory().unwrap());
        let svc = EmbeddingService::load(store.clone(), Arc::new(HashingEmbedder::new(64))).unwrap();
        let chunks = vec![
            ("c1".to_string(), "h1".to_string(), "fn login_button()".to_string()),
            ("c2".to_string(), "h2".to_string(), "fn footer()".to_string()),
        ];
        let s = svc.refresh(&chunks).unwrap();
        assert_eq!(s.embedded, 2);
        // Same content, new id → reused; unchanged → skipped.
        let chunks2 = vec![
            ("c1".to_string(), "h1".to_string(), "fn login_button()".to_string()),
            ("c3".to_string(), "h2".to_string(), "fn footer()".to_string()),
        ];
        let s = svc.refresh(&chunks2).unwrap();
        assert_eq!((s.unchanged, s.reused, s.embedded), (1, 1, 0));
        assert_eq!(svc.len(), 3);
        let live: HashSet<String> = ["c1", "c3"].iter().map(|s| s.to_string()).collect();
        assert_eq!(svc.retain(&live).unwrap(), 1);
        assert_eq!(svc.len(), 2);
        let hits = svc.search("login button", 1).unwrap();
        assert_eq!(hits[0].id, "c1");
        // Persisted and reloadable.
        let svc2 = EmbeddingService::load(store, Arc::new(HashingEmbedder::new(64))).unwrap();
        assert_eq!(svc2.len(), 2);
    }
}
