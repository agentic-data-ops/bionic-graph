//! Node registry — tracks live workers, manages heartbeats, detects
//! failed nodes, and persists known nodes for startup readiness checks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::settings::ClusterConfig;
use crate::graph::graph_registry::GraphMetadata;

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
    /// Last heartbeat timestamp in microseconds (for persistence).
    #[serde(default)]
    pub last_seen: u64,
    /// Node status ("alive" / "offline") for the persisted snapshot.
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(skip, default = "Instant::now")]
    last_heartbeat: Instant,
    #[serde(skip, default)]
    alive: bool,
}

fn default_status() -> String {
    "alive".to_string()
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
            last_seen: now_micros(),
            status: "alive".to_string(),
        }
    }

    /// Check if the worker has timed out.
    pub fn is_expired(&self, timeout: Duration) -> bool {
        self.last_heartbeat.elapsed() > timeout
    }
}

/// Snapshot of the whole cluster topology, persisted to
/// `<data_dir>/cluster/nodes.json` by the master on every heartbeat.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeSnapshot {
    /// The master node's own identity.
    #[serde(default)]
    pub master: Option<WorkerInfo>,
    /// All known workers (including currently offline ones).
    #[serde(default)]
    pub workers: Vec<WorkerInfo>,
    /// Snapshot format version.
    #[serde(default)]
    pub version: u32,
}

impl NodeSnapshot {
    pub fn new() -> Self {
        Self {
            master: None,
            workers: Vec::new(),
            version: 1,
        }
    }
}

impl Default for NodeSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// A graph as reported by a worker in its heartbeat, including its
/// full per-graph config so the master can detect inconsistencies.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerGraphSnapshot {
    /// Graph metadata (name, description, time_travel).
    pub meta: GraphMetadata,
    /// Full per-graph config (storage / lock / indices).
    pub config: crate::graph::graph::GraphConfig,
}

/// Messages exchanged between master and workers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClusterMessage {
    /// Worker → Master: registration / heartbeat.
    /// Carries the worker's local graph list so the master can detect
    /// missing/inconsistent graphs and issue sync commands.
    Heartbeat {
        node_id: String,
        api_addr: String,
        cluster_addr: String,
        last_acked_seq: u64,
        /// Worker's local graphs (metadata + config).
        #[serde(default)]
        graphs: Vec<WorkerGraphSnapshot>,
        /// Worker's default graph name.
        #[serde(default)]
        default_graph: String,
    },
    /// Master → Worker: heartbeat acknowledgment + graph sync commands.
    HeartbeatAck {
        master_time: u64,
        /// Graph sync commands computed by the master from the diff
        /// between the master's registry and the worker's reported graphs.
        #[serde(default)]
        sync_commands: Vec<GraphSyncCommand>,
    },
    /// Worker → Master: I am shutting down.
    Shutdown {
        node_id: String,
    },
}

/// A command the master sends to a worker to bring its local graphs
/// in sync with the master's registry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GraphSyncCommand {
    /// Create (open) a graph the worker is missing, with the master's
    /// metadata and full config.
    CreateGraph {
        name: String,
        description: String,
        time_travel: bool,
        config: crate::graph::graph::GraphConfig,
    },
    /// Update a graph's metadata (description / time_travel) to match.
    UpdateGraphMeta {
        name: String,
        description: String,
        time_travel: bool,
    },
    /// Update a graph's full config (storage / lock / indices) to match.
    UpdateGraphConfig {
        name: String,
        config: crate::graph::graph::GraphConfig,
    },
    /// Delete a graph the worker has but the master does not.
    DeleteGraph {
        name: String,
    },
    /// Set the default graph to match the master.
    SetDefaultGraph {
        name: String,
    },
}

/// The cluster node registry on the master.
pub struct NodeRegistry {
    #[allow(dead_code)]
    config: ClusterConfig,
    workers: RwLock<HashMap<String, WorkerInfo>>,
    /// Workers known from a previous run (loaded from nodes.json).
    /// Used for the master's startup readiness check.
    known_workers: RwLock<Vec<WorkerInfo>>,
    /// The master node's own identity (for nodes.json persistence).
    master_info: RwLock<Option<WorkerInfo>>,
    /// The heartbeat timeout duration (computed from config).
    timeout: Duration,
    /// Monotonically increasing cluster-wide operation sequence.
    next_seq: std::sync::atomic::AtomicU64,
    /// Data directory for persisting cluster/nodes.json.
    data_dir: Option<PathBuf>,
}

impl NodeRegistry {
    pub fn new(config: &ClusterConfig) -> Self {
        Self {
            config: config.clone(),
            workers: RwLock::new(HashMap::new()),
            known_workers: RwLock::new(Vec::new()),
            master_info: RwLock::new(None),
            timeout: Duration::from_secs(config.worker_timeout_secs),
            next_seq: std::sync::atomic::AtomicU64::new(1),
            data_dir: None,
        }
    }

    /// Register or heartbeat a worker.
    pub fn register(&self, info: WorkerInfo) {
        log::info!("register worker: {} (cluster={})", info.node_id, info.cluster_addr);
        {
            let mut workers = self.workers.write().unwrap_or_else(|e| e.into_inner());
            workers.insert(info.node_id.clone(), info);
        } // release write lock before persist (persist re-acquires read lock)
        self.persist();
    }

    /// Remove a worker (on shutdown or timeout).
    pub fn remove(&self, node_id: &str) {
        {
            let mut workers = self.workers.write().unwrap_or_else(|e| e.into_inner());
            workers.remove(node_id);
        } // release write lock before persist
        self.persist();
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
        {
            let mut workers = self.workers.write().unwrap_or_else(|e| e.into_inner());
            workers.retain(|id, w| {
                if w.is_expired(self.timeout) {
                    expired.push(id.clone());
                    false
                } else {
                    true
                }
            });
        } // release write lock before persist
        if !expired.is_empty() {
            self.persist();
        }
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

    // ── Node persistence (6.1) ───────────────────────────────────────────

    /// Set the data directory used for cluster/nodes.json persistence.
    pub fn set_data_dir(&mut self, dir: PathBuf) {
        self.data_dir = Some(dir);
    }

    /// Set the master node's own identity (node_id / api_addr / cluster_addr).
    /// Called on the master at startup so nodes.json includes master info.
    pub fn set_master_info(&self, info: WorkerInfo) {
        {
            let mut mi = self.master_info.write().unwrap_or_else(|e| e.into_inner());
            *mi = Some(info);
        } // release write lock before persist
        self.persist();
    }

    /// Persist the current cluster topology to `<data_dir>/cluster/nodes.json`.
    /// Called after every worker registration / removal.
    pub fn persist(&self) {
        let Some(dir) = self.data_dir.as_ref() else { return };
        let snapshot = NodeSnapshot {
            master: self.master_info.read().unwrap_or_else(|e| e.into_inner()).clone(),
            workers: self.list(),
            version: 1,
        };
        let cluster_dir = dir.join("cluster");
        if std::fs::create_dir_all(&cluster_dir).is_err() {
            return;
        }
        let path = cluster_dir.join("nodes.json");
        let json = serde_json::to_string_pretty(&snapshot);
        if let Ok(json) = json {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!("Failed to persist cluster/nodes.json: {}", e);
            }
        }
    }

    /// Load known workers from `<data_dir>/cluster/nodes.json`.
    /// Returns the list of worker node_ids known from a previous run.
    pub fn load_known(&self, dir: &Path) -> Vec<String> {
        let path = dir.join("cluster").join("nodes.json");
        if !path.exists() {
            return Vec::new();
        }
        let known = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<NodeSnapshot>(&s).ok());
        let Some(snapshot) = known else {
            return Vec::new();
        };
        let ids: Vec<String> = snapshot.workers.iter().map(|w| w.node_id.clone()).collect();
        let mut known_workers = self.known_workers.write().unwrap_or_else(|e| e.into_inner());
        *known_workers = snapshot.workers;
        ids
    }

    /// Number of known workers that have registered so far.
    pub fn known_registered_count(&self) -> usize {
        let known = self.known_workers.read().unwrap_or_else(|e| e.into_inner());
        if known.is_empty() {
            return 0;
        }
        let workers = self.workers.read().unwrap_or_else(|e| e.into_inner());
        known.iter().filter(|k| workers.contains_key(&k.node_id)).count()
    }

    /// Total number of known workers (from a previous run).
    pub fn known_total(&self) -> usize {
        let known = self.known_workers.read().unwrap_or_else(|e| e.into_inner());
        known.len()
    }
}

/// Current time in microseconds since the UNIX epoch.
pub fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}
