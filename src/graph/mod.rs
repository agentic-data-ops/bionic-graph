//! Query engine for the new block-based graph storage.
//!
//! This module replaces the old in-memory `Graph` and `DiskGraph` with a
//! direct block-oriented implementation. The central type is [`Graph`]
//! which combines:
//!
//! - A block-based storage engine (data file, bitmap, cache, WAL)
//! - An on-disk + in-memory index (B-tree for IDs, HashMap for tokens)
//! - A Gremlin-compatible step pipeline for traversal and search

#[cfg(test)]
pub mod tests;

pub mod crud;
pub mod graph;
pub mod graph_registry;
pub mod gremlin;
pub mod locked;
pub mod rank_decay;
pub mod serialize;
pub mod tokenizer;

pub use graph::{Graph, GraphConfig};
pub use gremlin::{GremlinQuery, GremlinResult, GremlinStep, execute_gremlin};
pub use serialize::{serialize_vertex, deserialize_vertex, serialize_edge, deserialize_edge};
