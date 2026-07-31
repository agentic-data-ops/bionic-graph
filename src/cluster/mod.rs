//! Cluster mode — distributed graph with 1 master + N workers.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────┐     ┌─────────┐     ┌─────────┐
//! │ Worker 1│     │ Master  │     │ Worker 2│
//! │ (read)  │◄────│(R+W)    │────►│ (read)  │
//! └────┬────┘     └─────────┘     └────┬────┘
//!      │               │               │
//!      └─── writes ────┘               │
//!           forwarded                  │
//!                                     │
//!         Redo log replication ────────┘
//! ```
//!
//! - **Master**: handles reads + writes, pushes redo log entries to workers
//! - **Worker**: handles reads only; forwards write requests to master via HTTP
//! - **Replication**: master pushes redo log entries to workers after each write
//! - **Heartbeat**: workers send periodic heartbeats to the master
//!
//! # Status
//!
//! This module is a functional stub. The core protocol types and forwarding
//! logic are defined, but the runtime integration (cluster-aware router,
//! automatic worker discovery, leader election) is not yet implemented.
//!
//! To use clustering, start a master:
//! ```ignore
//! cargo run -- --cluster-master 0.0.0.0:9090
//! ```
//!
//! Then start workers:
//! ```ignore
//! cargo run -- --cluster-worker 0.0.0.0:9091 --master 0.0.0.0:9090
//! ```

pub mod forward;
pub mod gateway;
pub mod node;
pub mod replication;
pub mod request;
pub mod server;
