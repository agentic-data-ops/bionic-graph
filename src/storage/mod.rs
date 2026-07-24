//! Block-based storage engine for the Bionic-Graph.
//!
//! This module replaces the old subgraph-based persistence with a flat
//! block-oriented design. Every vertex, edge, and token is stored as a
//! serialized payload inside 64-byte chunks within 16 KB blocks.
//!
//! # Sub-modules
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`types`]     | Fundamental type definitions, constants, binary layouts |
//! | [`block_allocator`] | Chunk-level allocator within a single block |
//! | [`data_file`] | Raw 16 KB block I/O |
//! | [`bitmap_file`] | Block-level space management |
//! | [`block_cache`] | LRU cache with dirty-page tracking |
//! | [`redo_log`]  | Write-Ahead Log with rotation + replay + checkpoint |

pub mod block_allocator;
pub mod block_cache;
pub mod bitmap_file;
pub mod data_file;
pub mod memory_index;
pub mod memory_index_builder;
pub mod redo_log;
pub mod types;
