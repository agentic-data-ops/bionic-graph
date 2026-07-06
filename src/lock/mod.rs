//! Concurrency lock management for the graph engine.
//!
//! Provides striped `RwLock` pools for blocks, vertices, and edges with
//! strict lock ordering to prevent deadlocks.

pub mod lock_manager;
