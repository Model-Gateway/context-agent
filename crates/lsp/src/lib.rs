//! Semantic code intelligence via the Language Server Protocol (spec §6.1,
//! "Semantic" level).
//!
//! This crate is deliberately independent from the parser layer: every
//! language works without a server, and server-backed operations degrade
//! gracefully to `Unavailable`. [`SemanticService`] is the interface the
//! tools use; [`ServerCatalog`] knows which servers exist for which
//! languages and whether they are installed; [`client::LspClient`] is a
//! minimal JSON-RPC stdio client for definition/references lookups.

pub mod catalog;
pub mod client;
pub mod service;

pub use catalog::{ServerCatalog, ServerSpec, ServerStatus};
pub use client::{LspClient, NotificationHandler};
pub use service::{Location, NullSemanticService, SemanticOutcome, SemanticService};
