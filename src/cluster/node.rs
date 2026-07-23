//! Node registry — tracks live workers, manages heartbeats, and detects
//! failed nodes.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::settings::ClusterConfig;

/// Identity and status of a single worker node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerInfo {
    /// Unique node ID (auto-assigned or configured).
    pub node_id: String,
    /// The worker's API endpoint (for proxying or health checks).
    pub api_addr: String,
    /// The worker's cluster communication address.
    pub cluster_addr: String,
    /// The last redo log sequence the worker has acknowledged.
    pub last_acked_seq: u64,
    #[serde(skip, default = "Instant::now")]
    last_heartbeat: Instant,
    #[serde(skip, default)]
    alive: bool,
}

impl WorkerInfo {
    pub fn new(node_id: &str, api_addr: &str, cluster_addr: &str) -> Self {
        Self {
            node_id: node_id.to_string(),
            api_addr: api_addr.to_string(),
            cluster_addr: cluster_addr.to_string(),
            last_heartbeat: Instant::now(),
            alive: true,
            last_acked_seq: 0,
        }
    }

    /// Check if the worker has timed out.
    pub fn is_expired(&self, timeout: Duration) -> bool {
        self.last_heartbeat.elapsed() > timeout
    }
}

/// Messages exchanged between master and workers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClusterMessage {
    /// Worker → Master: registration / heartbeat.
    Heartbeat {
        node_id: String,
        api_addr: String,
        cluster_addr: String,
        last_acked_seq: u64,
    },
    /// Master → Worker: heartbeat acknowledgment.
    HeartbeatAck {
        master_time: u64,
    },
    /// Worker → Master: I am shutting down.
    Shutdown {
        node_id: String,
    },
}

/// The cluster node registry on the master.
pub struct NodeRegistry {
    #[allow(dead_code)]
    config: ClusterConfig,
    workers: RwLock<HashMap<String, WorkerInfo>>,
    /// The heartbeat timeout duration (computed from config).
    timeout: Duration,
    /// Monotonically increasing cluster-wide operation sequence.
    next_seq: std::sync::atomic::AtomicU64,
}

impl NodeRegistry {
    pub fn new(config: &ClusterConfig) -> Self {
        Self {
            config: config.clone(),
            workers: RwLock::new(HashMap::new()),
            timeout: Duration::from_secs(config.worker_timeout_secs),
            next_seq: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Register or heartbeat a worker.
    pub fn register(&self, info: WorkerInfo) {
        let mut workers = self.workers.write().unwrap_or_else(|e| e.into_inner());
        workers.insert(info.node_id.clone(), info);
    }

    /// Remove a worker (on shutdown or timeout).
    pub fn remove(&self, node_id: &str) {
        let mut workers = self.workers.write().unwrap_or_else(|e| e.into_inner());
        workers.remove(node_id);
    }

    /// Get a worker by ID.
    pub fn get(&self, node_id: &str) -> Option<WorkerInfo> {
        let workers = self.workers.read().unwrap_or_else(|e| e.into_inner());
        workers.get(node_id).cloned()
    }

    /// List all workers.
    pub fn list(&self) -> Vec<WorkerInfo> {
        let workers = self.workers.read().unwrap_or_else(|e| e.into_inner());
        workers.values().cloned().collect()
    }

    /// List alive workers.
    pub fn alive_workers(&self) -> Vec<WorkerInfo> {
        let workers = self.workers.read().unwrap_or_else(|e| e.into_inner());
        workers
            .values()
            .filter(|w| w.alive && !w.is_expired(self.timeout))
            .cloned()
            .collect()
    }

    /// Purge workers that have timed out.
    pub fn purge_expired(&self) -> Vec<String> {
        let mut expired = Vec::new();
        let mut workers = self.workers.write().unwrap_or_else(|e| e.into_inner());
        workers.retain(|id, w| {
            if w.is_expired(self.timeout) {
                expired.push(id.clone());
                false
            } else {
                true
            }
        });
        expired
    }

    /// Allocate a new cluster-wide sequence number.
    pub fn next_seq(&self) -> u64 {
        self.next_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Mark all workers as alive (called by heartbeat handler).
    pub fn mark_all_alive(&self) {
        let workers = self.workers.read().unwrap_or_else(|e| e.into_inner());
        for w in workers.values() {
            let _ = w; // alive status tracked via is_expired at query time
        }
    }
}
