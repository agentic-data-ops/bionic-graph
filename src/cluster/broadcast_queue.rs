//! Persistent FIFO broadcast queue (plan 6.3).
//!
//! Replaces the fire-and-forget broadcast in `ClusterGateway` with a
//! **persisted queue + async consumer thread** that guarantees
//! at-least-once delivery:
//!
//! - Every broadcast request is written to a queue file **before** any
//!   HTTP attempt, so a crash cannot lose it.
//! - Queue files live at `<data_dir>/cluster/broadcast-<node>-<ts>.bin`,
//!   one rolling file per target node, max 1000 entries each.
//! - A consumer thread drains completed files in order, POSTing to
//!   `/cluster/execute` (or `/cluster/tokenizer-sync` for tokenizer ops).
//! - On network failure the consumer **retries forever** (with backoff)
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

/// Default maximum entries per queue file before rolling to a new file.
pub const DEFAULT_MAX_PER_FILE: usize = 1000;
/// Max age of an active file before it is rolled (so low-traffic queues
/// still get drained promptly). 1 second.
const ROLLOVER_AGE: u64 = 1_000_000; // microseconds
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

/// Persistent FIFO broadcast queue.
///
/// Thread-safety: the writer (`enqueue`) and the consumer (`start_consumer`)
/// run concurrently. The writer appends to a per-node "active" file; the
/// consumer only drains files that are no longer active (rolled files and
/// leftover files from a previous run).
pub struct BroadcastQueue {
    /// Directory holding the queue files (`<data_dir>/cluster`).
    dir: PathBuf,
    /// Max entries per queue file.
    max_per_file: usize,
    /// Active (still-being-written) file per target node:
    /// node_id → (file path, entries written so far, file created_at).
    active: Mutex<HashMap<String, (PathBuf, usize, u64)>>,
}

impl BroadcastQueue {
    /// Create a queue rooted at `data_dir/cluster` (directory created lazily).
    pub fn new(data_dir: &Path, max_per_file: usize) -> Self {
        Self {
            dir: data_dir.join("cluster"),
            max_per_file: max_per_file.max(1),
            active: Mutex::new(HashMap::new()),
        }
    }

    /// Enqueue a broadcast for a target worker. Writes the entry to the
    /// node's active queue file synchronously (durable before any HTTP).
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

        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        let full = {
            // Roll if the current file is too old (low-traffic drain) or full.
            if let Some((_, _, created_at)) = active.get(target_node) {
                if now_micros().saturating_sub(*created_at) > ROLLOVER_AGE {
                    active.remove(target_node);
                    log::debug!("BroadcastQueue: rolling queue for {} (age rollover)", target_node);
                }
            }

            let now = now_micros();
            let (file_path, count, _) = active
                .entry(target_node.to_string())
                .or_insert_with(|| (self.new_queue_file(target_node), 0, now));

            let append_ok = match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&*file_path)
            {
                Ok(mut f) => match writeln!(f, "{}", line) {
                    Ok(()) => true,
                    Err(e) => {
                        log::error!("BroadcastQueue: append to {} failed: {}", file_path.display(), e);
                        false
                    }
                },
                Err(e) => {
                    log::error!("BroadcastQueue: open {} failed: {}", file_path.display(), e);
                    false
                }
            };
            if !append_ok {
                return;
            }

            *count += 1;
            *count >= self.max_per_file
        };
        // Roll to a new file when the active file is full (releases the
        // borrow on `active` before the next insert).
        if full {
            active.remove(target_node);
            log::debug!(
                "BroadcastQueue: rolled queue for {} (full), new files will be created on demand",
                target_node
            );
        }
    }

    /// Build a fresh queue file path for a node.
    fn new_queue_file(&self, node_id: &str) -> PathBuf {
        let safe = sanitize_node_id(node_id);
        self.dir.join(format!("broadcast-{}-{}.bin", safe, now_micros()))
    }

    /// Current active (being-written) file paths — the consumer must skip these.
    fn active_file_paths(&self) -> Vec<PathBuf> {
        let active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        active.values().map(|(p, _, _)| p.clone()).collect()
    }

    /// List queue files that are ready to be consumed (not active).
    fn ready_files(&self) -> Vec<PathBuf> {
        let active = self.active_file_paths();
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "bin")
                    && path.file_name().map_or(false, |n| n.to_string_lossy().starts_with("broadcast-"))
                    && !active.contains(&path)
                {
                    files.push(path);
                }
            }
        }
        files.sort();
        files
    }

    /// Roll over any active files that have exceeded the age limit, so the
    /// consumer can drain them even with no further enqueues. Called by the
    /// consumer loop before each scan.
    fn rollover_stale(&self) {
        let stale: Vec<(String, PathBuf)> = {
            let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
            let now = now_micros();
            active
                .iter()
                .filter(|(_, (_, _, created_at))| now.saturating_sub(*created_at) > ROLLOVER_AGE)
                .map(|(node, (path, _, _))| (node.clone(), path.clone()))
                .collect()
        };
        if stale.is_empty() {
            return;
        }
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        for (node, _) in &stale {
            active.remove(node);
        }
        log::debug!("BroadcastQueue: rolled {} stale active file(s)", stale.len());
    }

    /// Load all entries from a queue file, in order.
    fn load_file(&self, path: &Path) -> Vec<QueuedBroadcast> {
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
        let url = format!(
            "http://{}{}{}",
            entry.target_addr,
            req.path,
            req.query.as_ref().map(|q| format!("?{}", q)).unwrap_or_default()
        );

        let client = reqwest::Client::new();
        let method = req.method.to_uppercase();
        let request = match method.as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            _ => {
                log::error!("BroadcastQueue: unsupported method '{}' for {}", req.method, url);
                // Unsupported method can never succeed — drop it.
                return true;
            }
        };

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
                // Any HTTP response means the worker received the request.
                // A business-level failure (e.g. duplicate replay) is
                // acceptable under at-least-once — don't block the queue.
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

    /// Drain a whole queue file in order. Blocks (with backoff) until every
    /// entry is delivered, then deletes the file.
    async fn drain_file(&self, path: &Path) {
        let entries = self.load_file(path);
        if entries.is_empty() {
            // Nothing parseable — drop the file to avoid an infinite loop.
            let _ = std::fs::remove_file(path);
            log::warn!("BroadcastQueue: removed unparseable/empty queue file {}", path.display());
            return;
        }

        log::info!(
            "BroadcastQueue: draining {} entries from {}",
            entries.len(),
            path.display()
        );

        let mut backoff = Duration::from_secs(1);
        let mut idx = 0;
        while idx < entries.len() {
            if Self::deliver(&entries[idx]).await {
                idx += 1;
                backoff = Duration::from_secs(1);
            } else {
                // Retry forever until the node succeeds.
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }

        // Entire file delivered — delete it.
        if let Err(e) = std::fs::remove_file(path) {
            log::error!("BroadcastQueue: failed to remove {}: {}", path.display(), e);
        } else {
            log::info!("BroadcastQueue: delivered and removed {}", path.display());
        }
    }

    /// Spawn the background consumer loop. Scans for ready files, drains
    /// them one at a time (oldest first), then waits and rescans.
    pub fn start_consumer(self: &Arc<Self>) {
        let queue = self.clone();
        tokio::spawn(async move {
            loop {
                queue.rollover_stale();
                let ready = queue.ready_files();
                if ready.is_empty() {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                for path in ready {
                    queue.drain_file(&path).await;
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
