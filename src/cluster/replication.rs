//! Redo log replication protocol — master pushes entries to workers.
//!
//! # Protocol
//!
//! After a write operation on the master, the master appends the redo log
//! entry and then pushes it to all connected workers via a simple HTTP POST.
//! Workers receive the entry, write it to their local redo log, and apply
//! it to their local graph state.
//!
//! # Why push over pull
//!
//! Push is simpler than pull for our use case:
//! - No polling overhead on workers
//! - Lower replication latency (entries arrive immediately)
//! - Easier to reason about ordering (master controls the sequence)

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::storage::redo_log::RedoLogEntry;

/// A redo log entry wrapped with cluster metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplicatedEntry {
    /// Cluster-wide sequence number (monotonically increasing on master).
    pub cluster_seq: u64,
    /// The actual redo log entry.
    pub entry: RedoLogEntry,
    /// Name of the graph this entry belongs to.
    pub graph_name: String,
    /// Timestamp (microseconds) when the entry was created on the master.
    pub master_timestamp: u64,
}

/// Response from a worker after receiving a replicated entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplicationAck {
    pub worker_id: String,
    pub acked_seq: u64,
    pub success: bool,
    pub error: Option<String>,
}

/// Errors that can occur during replication.
#[derive(Error, Debug)]
pub enum ReplicationError {
    #[error("Worker {0} returned error: {1}")]
    WorkerError(String, String),

    #[error("Worker {0} unreachable: {1}")]
    WorkerUnreachable(String, String),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Push a redo log entry to a single worker.
///
/// In production, this would use an HTTP client (reqwest) to POST the
/// entry to the worker's `/cluster/replicate` endpoint.
pub async fn push_entry_to_worker(
    worker_cluster_addr: &str,
    entry: &ReplicatedEntry,
) -> Result<ReplicationAck, ReplicationError> {
    // Replace 0.0.0.0 with 127.0.0.1 for outgoing connections.
    let addr = worker_cluster_addr.replace("0.0.0.0", "127.0.0.1");
    let url = format!("http://{}/cluster/replicate", addr);
    let body = serde_json::to_string(entry)?;

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await?;

    let ack: ReplicationAck = resp.json().await?;
    Ok(ack)
}

/// Push a redo log entry to all workers concurrently.
///
/// Returns a list of (worker_id, result) for all workers.
pub async fn broadcast_entry(
    workers: &[crate::cluster::node::WorkerInfo],
    entry: &ReplicatedEntry,
) -> Vec<(String, Result<ReplicationAck, ReplicationError>)> {
    let mut handles = Vec::new();
    let entry = entry.clone();

    for worker in workers {
        let addr = worker.cluster_addr.clone();
        let entry = entry.clone();
        let node_id = worker.node_id.clone();
        handles.push(tokio::spawn(async move {
            let result = push_entry_to_worker(&addr, &entry).await;
            (node_id, result)
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        match handle.await {
            Ok((id, result)) => results.push((id, result)),
            Err(e) => results.push(("unknown".to_string(), Err(ReplicationError::WorkerUnreachable("spawn".into(), e.to_string())))),
        }
    }
    results
}

/// Handle an incoming replicated entry on a worker.
///
/// The worker writes the entry to its own redo log and applies it to the
/// local graph state. Returns the sequence number that was acked.
pub fn handle_replicated_entry(
    entry: &ReplicatedEntry,
    current_seq: u64,
    graph: &crate::graph::graph::Graph,
) -> ReplicationAck {
    let success = (|| -> Result<(), String> {
        graph.redo_log
            .append(entry.entry.op_type, entry.entry.op_id, &entry.entry.data)
            .map_err(|e| format!("append to redo_log: {}", e))?;
        crate::graph::crud::replay_entry(graph, &entry.entry)
            .map_err(|e| format!("replay_entry: {}", e))?;
        Ok(())
    })();

    ReplicationAck {
        worker_id: "local".to_string(),
        acked_seq: if success.is_ok() { current_seq } else { current_seq },
        success: success.is_ok(),
        error: success.err(),
    }
}
