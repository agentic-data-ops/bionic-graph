//! Persistent FIFO broadcast queue (plan 6.3).
//!
//! Replaces the fire-and-forget broadcast in `ClusterGateway` with a
//! **persisted queue + async consumer thread** that guarantees
//! at-least-once delivery.
//!
//! # Design
//!
//! - **Write side (enqueue)**: entries are appended to the node's *current*
//!   queue file (`<data_dir>/cluster/broadcast-<node>-<ts>.bin`, JSON Lines).
//!   When a file reaches `max_per_file` (default 1000) entries, the next
//!   enqueue rolls to a fresh file — this is the safety valve that keeps
//!   any single file bounded. In normal operation entries are consumed
//!   faster than they accumulate, so no rollover happens.
//! - **Read side (consumer)**: polls every node's queue, delivering the
//!   oldest undelivered entry in order. A delivery only advances when it
//!   succeeds (retried forever on transport failure with backoff). When a
//!   whole file is delivered it is deleted and the next file (if any) is
//!   processed. Entries are re-read from disk on each attempt, so a crash
//!   mid-delivery simply replays them (at-least-once).
//! - **Restart recovery**: leftover files are scanned and re-queued on
//!   startup, so undelivered broadcasts survive a master restart.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cluster::forward::ForwardedRequest;
use crate::cluster::node::now_micros;

/// Default maximum entries per queue file before rolling to a new file.
pub const DEFAULT_MAX_PER_FILE: usize = 1000;

/// A single queued broadcast destined for one worker.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueuedBroadcast {
    /// The request to execute on the target worker.
    pub req: ForwardedRequest,
    /// Target worker node_id (e.g. "worker@127.0.0.1:9091").
    pub target_node: String,
    /// Target worker cluster address (e.g. "127.0.0.1:9091").
    pub target_addr: String,
    /// Enqueue timestamp in microseconds.
    pub created_at: u64,
}

/// A queue file plus how many of its leading entries have been delivered
/// and how many entries have been written to it.
#[derive(Clone)]
struct FileProgress {
    file: PathBuf,
    consumed: usize,
    written: usize,
}

/// Per-node queue state (guarded by `state`).
#[derive(Default)]
struct NodeQueueState {
    /// Queue files for this node, oldest first. The last entry is the
    /// current append target; earlier entries are being drained.
    files: Vec<FileProgress>,
}

/// Persistent FIFO broadcast queue.
///
/// The writer (`enqueue`) and the consumer (`start_consumer`) coordinate
/// through a single `state` mutex. File operations happen under the lock;
/// the network delivery happens outside it.
pub struct BroadcastQueue {
    /// Directory holding the queue files (`<data_dir>/cluster`).
    dir: PathBuf,
    /// Max entries per queue file before rolling (write-side).
    max_per_file: usize,
    /// Per-node queue state: node_id → queue files + progress.
    state: Mutex<HashMap<String, NodeQueueState>>,
}

impl BroadcastQueue {
    /// Create a queue rooted at `data_dir/cluster` (directory created lazily).
    pub fn new(data_dir: &Path, max_per_file: usize) -> Self {
        Self {
            dir: data_dir.join("cluster"),
            max_per_file: max_per_file.max(1),
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Enqueue a broadcast for a target worker. Appends the entry to the
    /// node's current queue file synchronously (durable before any HTTP
    /// attempt). Rolls to a fresh file when the current one is full.
    pub fn enqueue(&self, target_node: &str, target_addr: &str, req: &ForwardedRequest) {
        std::fs::create_dir_all(&self.dir).unwrap_or_else(|e| {
            log::error!("BroadcastQueue: failed to create {}: {}", self.dir.display(), e);
        });

        let entry = QueuedBroadcast {
            req: req.clone(),
            target_node: target_node.to_string(),
            target_addr: target_addr.to_string(),
            created_at: now_micros(),
        };
        let line = match serde_json::to_string(&entry) {
            Ok(l) => l,
            Err(e) => {
                log::error!("BroadcastQueue: failed to serialize queued broadcast: {}", e);
                return;
            }
        };

        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let ns = state.entry(target_node.to_string()).or_default();

        // Roll to a fresh file if the current one is full (or absent).
        let need_new = ns
            .files
            .last()
            .map_or(true, |fp| fp.written >= self.max_per_file);
        if need_new {
            ns.files.push(FileProgress {
                file: self.new_queue_file(target_node),
                consumed: 0,
                written: 0,
            });
        }

        let current = ns.files.last_mut().expect("file just created");
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&current.file)
        {
            Ok(mut f) => {
                if let Err(e) = writeln!(f, "{}", line) {
                    log::error!("BroadcastQueue: append to {} failed: {}", current.file.display(), e);
                } else {
                    current.written += 1;
                }
            }
            Err(e) => {
                log::error!("BroadcastQueue: open {} failed: {}", current.file.display(), e);
            }
        }
    }

    /// Build a fresh queue file path for a node.
    fn new_queue_file(&self, node_id: &str) -> PathBuf {
        let safe = sanitize_node_id(node_id);
        self.dir.join(format!("broadcast-{}-{}.bin", safe, now_micros()))
    }

    /// Number of complete lines currently in a queue file.
    fn file_line_count(path: &Path) -> usize {
        std::fs::read_to_string(path)
            .map(|c| c.lines().count())
            .unwrap_or(0)
    }

    /// Read all complete (newline-terminated) lines of a queue file, in order.
    /// A trailing partial line (being appended concurrently) is ignored.
    fn read_file(path: &Path) -> Vec<QueuedBroadcast> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::error!("BroadcastQueue: read {} failed: {}", path.display(), e);
                return Vec::new();
            }
        };
        content
            .lines()
            .filter_map(|l| serde_json::from_str::<QueuedBroadcast>(l).ok())
            .collect()
    }

    /// Deliver a single queued broadcast to its target worker.
    /// Returns `true` if the HTTP request reached the worker (any response);
    /// `false` only on transport-level failure (worker unreachable).
    async fn deliver(entry: &QueuedBroadcast) -> bool {
        let req = &entry.req;
        let client = reqwest::Client::new();

        // Tokenizer operations use /cluster/tokenizer-sync with a special body.
        let is_tokenizer = req.path == "/settings/tokenizer/words";
        let endpoint = if is_tokenizer { "/cluster/tokenizer-sync" } else { "/cluster/execute" };
        let url = format!("http://{}{}", entry.target_addr, endpoint);

        let request = if is_tokenizer {
            let op = match req.method.to_uppercase().as_str() {
                "POST" => "add",
                "DELETE" => "remove",
                _ => "unknown",
            };
            let words: Option<serde_json::Value> = req.body.as_ref().and_then(|b| {
                serde_json::from_str::<serde_json::Value>(b)
                    .ok()
                    .and_then(|v| v.get("words").cloned())
            });
            let sync_body = serde_json::json!({ "operation": op, "words": words });
            client.post(&url).json(&sync_body)
        } else {
            let payload_json = serde_json::to_string(req).unwrap_or_default();
            client.post(&url).header("Content-Type", "application/json").body(payload_json)
        };

        match request.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status < 500 {
                    log::debug!(
                        "BroadcastQueue: delivered {} {} -> {} (HTTP {})",
                        req.method,
                        req.path,
                        entry.target_node,
                        status
                    );
                    true
                } else {
                    log::warn!(
                        "BroadcastQueue: worker {} returned HTTP {} for {} {} — retrying",
                        entry.target_node,
                        status,
                        req.method,
                        req.path
                    );
                    false
                }
            }
            Err(e) => {
                log::warn!(
                    "BroadcastQueue: delivery to {} failed: {} (will retry)",
                    entry.target_node,
                    e
                );
                false
            }
        }
    }

    /// Rebuild in-memory state from leftover queue files after a restart.
    /// Files are grouped by target node (from their first entry) and sorted
    /// oldest-first per node.
    fn rebuild_state_from_files(&self) {
        let mut files_by_node: HashMap<String, Vec<PathBuf>> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.extension().map_or(false, |e| e == "bin")
                    || !path.file_name().map_or(false, |n| n.to_string_lossy().starts_with("broadcast-"))
                {
                    continue;
                }
                match Self::read_file(&path).into_iter().next() {
                    Some(first) => files_by_node.entry(first.target_node).or_default().push(path),
                    None => {
                        // Empty/unparseable leftover — drop it.
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }

        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        for (node, mut files) in files_by_node {
            files.sort();
            let ns = state.entry(node).or_default();
            for f in files {
                let written = Self::file_line_count(&f);
                ns.files.push(FileProgress { file: f, consumed: 0, written });
            }
        }
        let total: usize = state.values().map(|ns| ns.files.len()).sum();
        if total > 0 {
            log::info!(
                "BroadcastQueue: rebuilt state for {} leftover queue file(s) after restart",
                total
            );
        }
    }

    /// Spawn the background consumer loop. Polls every node's queue,
    /// delivering entries in order (oldest file first), retrying failures
    /// forever, deleting each file once fully delivered.
    pub fn start_consumer(self: &Arc<Self>) {
        let queue = self.clone();
        tokio::spawn(async move {
            // Rebuild state from leftover files (survived a restart).
            queue.rebuild_state_from_files();
            loop {
                let node_ids: Vec<String> = {
                    let state = queue.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.keys().cloned().collect()
                };
                if node_ids.is_empty() {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
                for node_id in &node_ids {
                    // Prepare under the lock (no await); deliver outside it.
                    let entry = {
                        let mut state = queue.state.lock().unwrap_or_else(|e| e.into_inner());
                        let Some(ns) = state.get_mut(node_id.as_str()) else {
                            continue;
                        };
                        queue.prepare_next(node_id, ns)
                    };
                    if let Some(entry) = entry {
                        let ok = Self::deliver(&entry).await;
                        if ok {
                            let mut state = queue.state.lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(ns) = state.get_mut(node_id.as_str()) {
                                if let Some(fp) = ns.files.first_mut() {
                                    fp.consumed += 1;
                                }
                            }
                        } else {
                            // Delivery failed — back off before retrying.
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                    }
                }
            }
        });
        log::info!("BroadcastQueue: consumer thread started (dir={})", self.dir.display());
    }

    /// Prepare the next undelivered entry for a node (lock held, no await).
    /// Cleans up fully-delivered/empty files. Returns:
    /// - `Some(entry)` — the next entry to deliver (caller delivers outside the lock)
    /// - `None` — nothing to deliver now
    fn prepare_next(&self, _node_id: &str, ns: &mut NodeQueueState) -> Option<QueuedBroadcast> {
        // Drop fully-delivered or empty leading files.
        while let Some(fp) = ns.files.first() {
            let total = Self::file_line_count(&fp.file);
            if total == 0 || fp.consumed >= total {
                let path = fp.file.clone();
                let _ = std::fs::remove_file(&path);
                log::info!("BroadcastQueue: delivered and removed {}", path.display());
                ns.files.remove(0);
            } else {
                break;
            }
        }

        let fp = ns.files.first_mut()?;
        let entries = Self::read_file(&fp.file);
        let total = entries.len();
        if fp.consumed >= total {
            return None;
        }
        Some(entries[fp.consumed].clone())
    }
}

/// Sanitize a node_id for use in a file name (`@` and `:` are replaced).
fn sanitize_node_id(node_id: &str) -> String {
    node_id
        .chars()
        .map(|c| match c {
            '@' | ':' | '/' | '\\' | ' ' => '_',
            c => c,
        })
        .collect()
}
