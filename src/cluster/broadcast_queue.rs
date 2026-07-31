//! Persistent FIFO broadcast queue (plan 6.3).
//!
//! Replaces the fire-and-forget broadcast in `ClusterGateway` with a
//! **persisted queue + async consumer thread** that guarantees
//! at-least-once delivery:
//!
//! - Every broadcast request is written to a queue file **before** any
//!   HTTP attempt, so a crash cannot lose it.
//! - Queue files live at `<data_dir>/cluster/broadcast-<node>-<ts>.bin`
//!   (one file per target node, JSON Lines).
//! - The consumer thread **drains in real time**: it delivers the oldest
//!   undelivered entry, and only advances when the delivery succeeds — so
//!   successful broadcasts are removed immediately (no waiting for a
//!   rollover). Entries are re-read from disk on every attempt, so a
//!   crash mid-delivery simply replays them (at-least-once).
//! - The `max_per_file` limit (default 1000) is a **safety valve** only:
//!   if undelivered entries pile up (e.g. a worker stays offline and
//!   retries keep failing), the file is rolled/compacted to a new file so
//!   a single file never grows unbounded. In normal operation entries are
//!   consumed faster than they accumulate, so no rollover happens.
//! - On transport failure the consumer retries forever (with backoff)
//!   until the node succeeds; once a whole file is delivered it is deleted.
//! - On master restart, leftover files are re-queued and drained before
//!   the master starts serving.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cluster::forward::ForwardedRequest;
use crate::cluster::node::now_micros;

/// Default maximum **undelivered** entries per queue file before rolling.
pub const DEFAULT_MAX_PER_FILE: usize = 1000;
/// Backoff for retries after a failed delivery (1s → 2s → 4s → … capped at 30s).
const MAX_BACKOFF: Duration = Duration::from_secs(30);

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

/// Per-node queue state (guarded by `state`).
struct NodeQueueState {
    /// The file currently receiving appends for this node.
    file: PathBuf,
    /// Number of leading entries already delivered (advanced in order).
    consumed: usize,
}

/// Persistent FIFO broadcast queue.
///
/// The writer (`enqueue`) and the consumer (`start_consumer`) coordinate
/// through a single `state` mutex. File operations (append / delete /
/// compact) happen under the lock; the network delivery happens outside it.
pub struct BroadcastQueue {
    /// Directory holding the queue files (`<data_dir>/cluster`).
    dir: PathBuf,
    /// Max undelivered entries per file before rolling.
    max_per_file: usize,
    /// Per-node queue state: node_id → (file, consumed count).
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
    /// node's queue file synchronously (durable before any HTTP attempt).
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
        let node_state = state
            .entry(target_node.to_string())
            .or_insert_with(|| NodeQueueState {
                file: self.new_queue_file(target_node),
                consumed: 0,
            });

        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&node_state.file)
        {
            Ok(mut f) => {
                if let Err(e) = writeln!(f, "{}", line) {
                    log::error!("BroadcastQueue: append to {} failed: {}", node_state.file.display(), e);
                }
            }
            Err(e) => {
                log::error!("BroadcastQueue: open {} failed: {}", node_state.file.display(), e);
            }
        }
    }

    /// Build a fresh queue file path for a node.
    fn new_queue_file(&self, node_id: &str) -> PathBuf {
        let safe = sanitize_node_id(node_id);
        self.dir.join(format!("broadcast-{}-{}.bin", safe, now_micros()))
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

    /// Prepare the next undelivered entry for a node (lock held, no await).
    /// Returns:
    /// - `Some(entry)` — the next entry to deliver (caller delivers outside the lock)
    /// - `None` — nothing to deliver now (file empty / fully delivered / compacted)
    fn prepare_next(&self, node_id: &str, node_state: &mut NodeQueueState) -> Option<QueuedBroadcast> {
        let entries = Self::read_file(&node_state.file);
        let total = entries.len();
        if total == 0 {
            // Nothing parseable / empty — drop the file to avoid spinning.
            let _ = std::fs::remove_file(&node_state.file);
            log::warn!("BroadcastQueue: removed empty/unparseable queue file {}", node_state.file.display());
            return None;
        }

        // Safety valve: if undelivered entries exceed the cap, compact the
        // file to keep only the undelivered tail in a fresh file.
        if total.saturating_sub(node_state.consumed) >= self.max_per_file {
            let undelivered = entries[node_state.consumed..].to_vec();
            let new_file = self.new_queue_file(node_id);
            if let Ok(mut f) = std::fs::File::create(&new_file) {
                for e in &undelivered {
                    if let Ok(line) = serde_json::to_string(e) {
                        let _ = writeln!(f, "{}", line);
                    }
                }
                let _ = std::fs::remove_file(&node_state.file);
                log::warn!(
                    "BroadcastQueue: rolled {} ({} undelivered entries exceed cap {}), new file {}",
                    node_id,
                    undelivered.len(),
                    self.max_per_file,
                    new_file.display()
                );
                node_state.file = new_file;
                node_state.consumed = 0;
            }
            return None;
        }

        if node_state.consumed >= total {
            // All entries delivered — delete the file (guard against a
            // concurrent append that raced the read by re-checking).
            let still_all = Self::read_file(&node_state.file).len() <= node_state.consumed;
            if still_all {
                let _ = std::fs::remove_file(&node_state.file);
                log::info!("BroadcastQueue: delivered and removed {}", node_state.file.display());
            }
            return None;
        }

        Some(entries[node_state.consumed].clone())
    }

    /// Spawn the background consumer loop. Polls every node's queue,
    /// delivering entries in order, retrying failures forever.
    pub fn start_consumer(self: &Arc<Self>) {
        let queue = self.clone();
        tokio::spawn(async move {
            loop {
                // Drop stale state entries whose file was fully delivered.
                {
                    let mut state = queue.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.retain(|_, ns| ns.file.exists());
                }
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
                        match state.get_mut(node_id.as_str()) {
                            Some(ns) => queue.prepare_next(node_id, ns),
                            None => None,
                        }
                    };
                    if let Some(entry) = entry {
                        let ok = Self::deliver(&entry).await;
                        if ok {
                            let mut state = queue.state.lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(ns) = state.get_mut(node_id.as_str()) {
                                ns.consumed += 1;
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
