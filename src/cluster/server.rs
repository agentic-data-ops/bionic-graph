//! Cluster HTTP server — handles heartbeat, write forwarding, and
//! redo-log replication between master and workers.
//!
//! # Endpoints
//!
//! | Method | Path | Direction | Description |
//! |--------|------|-----------|-------------|
//! | POST | `/cluster/heartbeat` | Worker → Master | Worker registration + heartbeat |
//! | POST | `/cluster/forward` | Worker → Master | Forwarded write request |
//! | POST | `/cluster/replicate` | Master → Worker | Redo log entry push |
//! | POST | `/cluster/touch` | Worker → Master | Report read vertex/edge IDs for rank/atime update |

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    Json,
    Router,
    routing::post,
};

use serde::Deserialize;

use crate::cluster::forward::{ForwardedRequest, ForwardedResponse};
use crate::cluster::node::{ClusterMessage, NodeRegistry, WorkerInfo};
use crate::cluster::replication::{ReplicatedEntry, ReplicationAck};
use crate::graph_manager::GraphManager;
use crate::storage::types::OpType;

/// Shared state for the cluster communication server.
#[derive(Clone)]
pub struct ClusterAppState {
    pub gm: Arc<GraphManager>,
    pub registry: Arc<NodeRegistry>,
    /// This node's role (master or worker).
    pub is_master: bool,
    /// Address of the main API HTTP server (for forwarding).
    pub api_addr: String,
}

/// Build the axum router for the cluster communication server.
pub fn build_cluster_router(state: ClusterAppState) -> Router {
    Router::new()
        .route("/cluster/heartbeat", post(handle_heartbeat))
        .route("/cluster/forward", post(handle_forward))
        .route("/cluster/replicate", post(handle_replicate))
        .route("/cluster/touch", post(handle_touch))
        .route("/cluster/tokenizer-sync", post(handle_tokenizer_sync))
        .with_state(state)
}

// ── Heartbeat ────────────────────────────────────────────────────────────────

/// POST /cluster/heartbeat
///
/// Worker sends its identity; master records/refreshes the worker.
async fn handle_heartbeat(
    State(state): State<ClusterAppState>,
    Json(msg): Json<ClusterMessage>,
) -> Result<Json<ClusterMessage>, StatusCode> {
    match msg {
        ClusterMessage::Heartbeat { node_id, api_addr, cluster_addr, last_acked_seq: _ } => {
            let info = WorkerInfo::new(&node_id, &api_addr, &cluster_addr);
            state.registry.register(info);
            Ok(Json(ClusterMessage::HeartbeatAck {
                master_time: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u64,
            }))
        }
        ClusterMessage::Shutdown { node_id } => {
            state.registry.remove(&node_id);
            Ok(Json(ClusterMessage::HeartbeatAck { master_time: 0 }))
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

// ── Forward ─────────────────────────────────────────────────────────────────

/// POST /cluster/forward
///
/// Master proxies a forwarded write to the local API server, then broadcasts
/// the resulting redo-log entry to all workers.
async fn handle_forward(
    State(state): State<ClusterAppState>,
    Json(req): Json<ForwardedRequest>,
) -> Json<ForwardedResponse> {
    if !state.is_master {
        return Json(ForwardedResponse {
            success: false,
            status_code: 403,
            body: None,
            error: Some("Only master handles forwarded writes".to_string()),
        });
    }

    // Proxy the request to the master's main API server.
    let result = proxy_to_api(&state.api_addr, &req).await;

    // If the write succeeded, broadcast to all workers.
    if result.success {
        // Tokenizer operations: broadcast directly to workers' tokenizer-sync endpoint.
        if req.path == "/settings/tokenizer/words" {
            let workers = state.registry.alive_workers();
            let op = match req.method.to_uppercase().as_str() {
                "POST" => "add",
                "DELETE" => "remove",
                _ => "",
            };
            if !op.is_empty() && !workers.is_empty() {
                if let Some(ref req_body) = req.body {
                    let workers_for_broadcast = workers.clone();
                    let body_clone = req_body.clone();
                    let op_str = op.to_string();
                    tokio::spawn(async move {
                        for worker in &workers_for_broadcast {
                            let url = format!("http://{}/cluster/tokenizer-sync", worker.cluster_addr);
                            let client = reqwest::Client::new();
                            let sync_body = serde_json::json!({
                                "operation": &op_str,
                                "words": serde_json::from_str::<serde_json::Value>(&body_clone)
                                    .ok().and_then(|v| v.get("words").cloned())
                                    .unwrap_or(serde_json::Value::Null),
                            });
                            if let Err(e) = client.post(&url)
                                .json(&sync_body)
                                .send().await
                            {
                                log::warn!("Tokenizer sync to worker {} failed: {}", worker.node_id, e);
                            }
                        }
                    });
                }
            }
            return Json(result);
        }

        let workers = state.registry.alive_workers();
        if !workers.is_empty() {
            let entries = build_broadcast_entries(&state, &req, &result);
            for entry in entries {
                let seq = state.registry.next_seq();
                let replicated = ReplicatedEntry {
                    cluster_seq: seq,
                    entry,
                    master_timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u64,
                };
                let w = workers.clone();
                tokio::spawn(async move {
                    let results = crate::cluster::replication::broadcast_entry(&w, &replicated).await;
                    for (wid, res) in &results {
                        if let Err(e) = res {
                            log::warn!("Replication to worker {} failed: {}", wid, e);
                        }
                    }
                });
            }
        }
    }

    Json(result)
}

/// After a successful forwarded write, build redo-log entries from the
/// actual data stored on the master so workers can replay them correctly.
fn build_broadcast_entries(
    state: &ClusterAppState,
    req: &ForwardedRequest,
    result: &ForwardedResponse,
) -> Vec<crate::storage::redo_log::RedoLogEntry> {
    let mut entries = Vec::new();
    let method = req.method.to_uppercase();
    let default_name = state.gm.get_default_name();
    let graph = match state.gm.get(&default_name) {
        Ok(g) => g,
        Err(_) => return entries,
    };

    // Parse created/updated ID from response body {"id": N}.
    let body = match result.body {
        Some(ref b) => b.clone(),
        None => return entries,
    };
    let parsed: std::collections::HashMap<String, serde_json::Value> = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return entries,
    };
    let id = match parsed.get("id").and_then(|v| v.as_u64()) {
        Some(id) => id as u32,
        None => return entries,
    };

    match (method.as_str(), req.path.as_str()) {
        ("POST", "/vertices") | ("PUT", "/vertices") => {
            // Use read_vertex_by_record to avoid updating rank/atime.
            let found = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).vertices.get(id).copied();
            if let Some(ptr) = found {
                if let Ok(rec) = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset) {
                    if let Ok(Some(payload)) = crate::graph::crud::read_vertex_by_record(&graph, &rec, None) {
                        if let Ok(data) = crate::graph::serialize::serialize_vertex(&payload) {
                            entries.push(crate::storage::redo_log::RedoLogEntry {
                                op_type: OpType::VertexCreate,
                                op_id: id as u64,
                                data,
                            });
                        }
                    }
                }
            }
        }
        ("POST", "/edges") | ("PUT", "/edges") => {
            // Read edge payload directly without updating rank/atime.
            let found = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).edges.get(id).copied();
            if let Some(ptr) = found {
                if let Ok(rec) = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset) {
                    if let Ok(data) = crate::graph::crud::read_data_chunks(
                        &graph, rec.data_block_idx, rec.data_chunk_offset, rec.data_len,
                    ) {
                        if let Ok(payload) = crate::graph::serialize::deserialize_edge(&data) {
                            if let Ok(serialized) = crate::graph::serialize::serialize_edge(&payload) {
                                entries.push(crate::storage::redo_log::RedoLogEntry {
                                    op_type: OpType::EdgeCreate,
                                    op_id: id as u64,
                                    data: serialized,
                                });
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    entries
}

/// Proxy a ForwardedRequest to the master's main API server via HTTP.
async fn proxy_to_api(api_addr: &str, req: &ForwardedRequest) -> ForwardedResponse {
    let url = format!(
        "http://{}{}{}",
        api_addr,
        req.path,
        req.query.as_ref().map(|q| format!("?{}", q)).unwrap_or_default()
    );

    let client = reqwest::Client::new();
    let method = req.method.to_uppercase();

    let request = match method.as_str() {
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        _ => {
            return ForwardedResponse {
                success: false,
                status_code: 400,
                body: None,
                error: Some(format!("Unsupported method: {}", req.method)),
            };
        }
    };

    let request = if let Some(ref body) = req.body {
        request.header("Content-Type", "application/json").body(body.clone())
    } else {
        request
    };

    match request.send().await {
        Ok(resp) => {
            let status_code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            ForwardedResponse {
                success: status_code < 500,
                status_code,
                body: Some(body),
                error: if status_code >= 400 { Some(format!("HTTP {}", status_code)) } else { None },
            }
        }
        Err(e) => ForwardedResponse {
            success: false,
            status_code: 502,
            body: None,
            error: Some(format!("Proxy error: {}", e)),
        },
    }
}

// ── Replicate ────────────────────────────────────────────────────────────────

/// POST /cluster/replicate
///
/// Worker receives a redo log entry from the master and writes it to
/// the default graph's redo log.
async fn handle_replicate(
    State(state): State<ClusterAppState>,
    Json(entry): Json<ReplicatedEntry>,
) -> Json<ReplicationAck> {
    if state.is_master {
        return Json(ReplicationAck {
            worker_id: "local".to_string(),
            acked_seq: entry.cluster_seq,
            success: false,
            error: Some("Workers handle replication, not master".to_string()),
        });
    }

    // Write the entry to the default graph's redo log and replay it
    // into the in-memory state so the worker can immediately see changes.
    // redo_log.append() uses synchronous Condvar — defer to spawn_blocking.
    let default_name = state.gm.get_default_name();
    let graph = match state.gm.get(&default_name) {
        Ok(g) => g,
        Err(e) => {
            log::error!("replicate: failed to get default graph: {}", e);
            return Json(ReplicationAck {
                worker_id: "local".to_string(),
                acked_seq: entry.cluster_seq,
                success: false,
                error: Some(format!("Failed to get default graph: {}", e)),
            });
        }
    };
    let log_entry = entry.entry;
    let seq = entry.cluster_seq;
    let g = graph.clone();

    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        g.redo_log.append(log_entry.op_type, log_entry.op_id, &log_entry.data)
            .map_err(|e| format!("Failed to append to redo log: {}", e))?;
        crate::graph::crud::replay_entry(&g, &log_entry)
            .map_err(|e| format!("Failed to replay entry: {}", e))?;
        Ok(())
    })
    .await
    .unwrap_or(Err("spawn_blocking panicked".to_string()));

    let success = result.is_ok();
    if !success {
        log::error!(
            "Replication failed for seq {}: {:?}",
            seq,
            result.as_ref().unwrap_err()
        );
    }

    Json(ReplicationAck {
        worker_id: "local".to_string(),
        acked_seq: seq,
        success,
        error: result.err(),
    })
}

// ── Touch (read report) ──────────────────────────────────────────────────────

/// Request body for `/cluster/touch`: IDs of vertices/edges that were read.
#[derive(Deserialize)]
pub struct TouchRequest {
    pub vertex_ids: Vec<u32>,
    pub edge_ids: Vec<u32>,
}

/// Shared touch logic: update local rank/atime, create IndexUpdate entries,
/// append to local redo_log (durable), and optionally broadcast to workers.
///
/// Can be called directly from the master's gremlin handler (no HTTP needed)
/// or from the `/cluster/touch` endpoint (worker → master).
pub async fn process_touch(
    graph: &Arc<crate::graph::graph::Graph>,
    vertex_ids: &[u32],
    edge_ids: &[u32],
    registry: Option<&NodeRegistry>,
) {
    let entries = build_touch_entries(graph, vertex_ids, edge_ids);
    if entries.is_empty() {
        return;
    }

    // Append all entries to the local redo_log (blocking — defer to spawn_blocking).
    let g = graph.clone();
    let entries_sync = entries.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || -> Result<(), String> {
        for entry in &entries_sync {
            g.redo_log.append(entry.op_type, entry.op_id, &entry.data)
                .map_err(|e| format!("append to redo_log: {}", e))?;
        }
        Ok(())
    })
    .await
    .unwrap_or(Err("spawn_blocking panicked".to_string()))
    {
        log::warn!("process_touch: redo_log append failed: {}", e);
        return;
    }

    // Broadcast to workers if registry is provided and has alive workers.
    if let Some(reg) = registry {
        let workers = reg.alive_workers();
        if !workers.is_empty() {
            for entry in entries {
                let seq = reg.next_seq();
                let replicated = ReplicatedEntry {
                    cluster_seq: seq,
                    entry,
                    master_timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u64,
                };
                let w = workers.clone();
                tokio::spawn(async move {
                    let results = crate::cluster::replication::broadcast_entry(&w, &replicated).await;
                    for (wid, res) in &results {
                        if let Err(e) = res {
                            log::debug!("broadcast IndexUpdate to {} failed: {}", wid, e);
                        }
                    }
                });
            }
        }
    }
}

/// Build IndexUpdate redo-log entries for the given vertex/edge IDs.
/// Updates local rank/atime via `get_vertex_locked` / `get_edge_locked`.
fn build_touch_entries(
    graph: &Arc<crate::graph::graph::Graph>,
    vertex_ids: &[u32],
    edge_ids: &[u32],
) -> Vec<crate::storage::redo_log::RedoLogEntry> {
    let mut entries = Vec::new();

    for vid in vertex_ids {
        if let Err(e) = crate::graph::locked::get_vertex_locked(graph, *vid) {
            log::debug!("touch vertex {}: {}", vid, e);
            continue;
        }
        let found = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).vertices.get(*vid).copied();
        if let Some(ptr) = found {
            if let Ok(rec) = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset) {
                let mut data = Vec::with_capacity(12);
                data.extend_from_slice(&rec.rank.to_le_bytes());
                data.extend_from_slice(&rec.atime.to_le_bytes());
                entries.push(crate::storage::redo_log::RedoLogEntry {
                    op_type: OpType::VertexIndexUpdate,
                    op_id: *vid as u64,
                    data,
                });
            }
        }
    }

    for eid in edge_ids {
        if let Err(e) = crate::graph::locked::get_edge_locked(graph, *eid) {
            log::debug!("touch edge {}: {}", eid, e);
            continue;
        }
        let found = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).edges.get(*eid).copied();
        if let Some(ptr) = found {
            if let Ok(rec) = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset) {
                let mut data = Vec::with_capacity(12);
                data.extend_from_slice(&rec.rank.to_le_bytes());
                data.extend_from_slice(&rec.atime.to_le_bytes());
                entries.push(crate::storage::redo_log::RedoLogEntry {
                    op_type: OpType::EdgeIndexUpdate,
                    op_id: *eid as u64,
                    data,
                });
            }
        }
    }

    entries
}

/// POST /cluster/touch
///
/// Worker reports which vertices/edges were read. Delegates to `process_touch`
/// for local rank/atime update, redo_log persistence, and worker broadcast.
async fn handle_touch(
    State(state): State<ClusterAppState>,
    Json(req): Json<TouchRequest>,
) -> StatusCode {
    if !state.is_master {
        return StatusCode::OK;
    }

    let default_name = state.gm.get_default_name();
    let graph = match state.gm.get(&default_name) {
        Ok(g) => g,
        Err(e) => {
            log::warn!("touch: failed to get default graph: {}", e);
            return StatusCode::OK;
        }
    };

    process_touch(&graph, &req.vertex_ids, &req.edge_ids, Some(&state.registry)).await;
    StatusCode::OK
}

/// POST /cluster/tokenizer-sync
///
/// Master broadcasts tokenizer word changes to workers.
/// Workers apply the changes directly to their local jieba instance.
async fn handle_tokenizer_sync(
    Json(body): Json<TokenizerSyncBody>,
) -> Json<serde_json::Value> {
    let words: Vec<String> = body.words.into_iter().filter(|w| w.chars().count() >= 2).collect();
    if words.is_empty() {
        return Json(serde_json::json!({"status": "ok", "applied": 0}));
    }
    match body.operation.as_str() {
        "add" => crate::graph::tokenizer::add_custom_words(&words),
        "remove" => crate::graph::tokenizer::remove_custom_words(&words),
        _ => return Json(serde_json::json!({"status": "error", "message": "unknown operation"})),
    }
    Json(serde_json::json!({"status": "ok", "applied": words.len()}))
}

#[derive(serde::Deserialize)]
struct TokenizerSyncBody {
    operation: String,
    words: Vec<String>,
}
