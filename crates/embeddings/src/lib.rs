//! Semantic retrieval support (spec §7.6).
//!
//! * [`Embedder`] abstracts the embedding model. The default
//!   [`HashingEmbedder`] is a deterministic feature-hashing model that needs
//!   no download; it captures lexical/identifier similarity rather than deep
//!   semantics. The `onnx` backend is an explicit extension point.
//! * [`VectorIndex`] is an exact cosine-similarity index kept in memory and
//!   persisted through the index store. HNSW can replace it behind the same
//!   interface once benchmarks justify it.
//! * [`EmbeddingService`] keeps the index in sync with chunks, reusing
//!   vectors when content and model are unchanged.

pub mod embedder;
pub mod index;
pub mod service;

pub use embedder::{build_embedder, Embedder, HashingEmbedder};
pub use index::{VectorIndex, VectorMatch};
pub use service::{EmbeddingService, RefreshStats};
