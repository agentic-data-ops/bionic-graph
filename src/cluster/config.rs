//! Cluster configuration — defines how nodes discover each other and how
//! replication works.

use serde::{Deserialize, Serialize};

/// Role of this node in the cluster.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    /// Single master — handles reads + writes.
    Master,
    /// Read replica — proxies writes to the master.
    Worker,
}

/// Top-level cluster configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Whether clustering is enabled at all.
    pub enabled: bool,
    /// This node's role.
    pub role: NodeRole,
    /// Address this node listens on for inter-node communication.
    /// Format: `host:port`. Default: `0.0.0.0:9090`.
    pub bind_addr: String,
    /// For workers: the master's address for replication + write forwarding.
    /// Format: `host:port`.
    pub master_addr: Option<String>,
    /// For masters: interval (seconds) between heartbeat checks.
    pub heartbeat_interval_secs: u64,
    /// Timeout (seconds) before a worker is considered dead.
    pub worker_timeout_secs: u64,
    /// Whether to forward write requests from workers to the master.
    pub forward_writes: bool,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            role: NodeRole::Master,
            bind_addr: "0.0.0.0:9090".to_string(),
            master_addr: None,
            heartbeat_interval_secs: 5,
            worker_timeout_secs: 30,
            forward_writes: true,
        }
    }
}

impl ClusterConfig {
    /// Create a config for a master node.
    pub fn master(bind_addr: &str) -> Self {
        Self {
            enabled: true,
            role: NodeRole::Master,
            bind_addr: bind_addr.to_string(),
            ..Self::default()
        }
    }

    /// Create a config for a worker node.
    pub fn worker(bind_addr: &str, master_addr: &str) -> Self {
        Self {
            enabled: true,
            role: NodeRole::Worker,
            bind_addr: bind_addr.to_string(),
            master_addr: Some(master_addr.to_string()),
            ..Self::default()
        }
    }

    /// Returns `true` if this node is the master.
    pub fn is_master(&self) -> bool {
        self.role == NodeRole::Master
    }

    /// Returns `true` if this node is a worker.
    pub fn is_worker(&self) -> bool {
        self.role == NodeRole::Worker
    }
}
