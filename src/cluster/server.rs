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

use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    Json,
    Router,
    routing::post,
};

use serde::{Deserialize, Serialize};

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
        .route("/cluster/execute", post(handle_execute))
        .route("/cluster/touch", post(handle_touch))
        .route("/cluster/tokenizer-sync", post(handle_tokenizer_sync))
        .with_state(state)
}

// ── Heartbeat ────────────────────────────────────────────────────────────────

/// POST /cluster/heartbeat
///
/// Worker sends its identity; master records/refreshes the worker.
/// On the master, the reported graph list is compared against the
/// master's registry and the worker is told what to create/update (6.2).
async fn handle_heartbeat(
    State(state): State<ClusterAppState>,
    Json(msg): Json<ClusterMessage>,
) -> Result<Json<ClusterMessage>, StatusCode> {
    match msg {
        ClusterMessage::Heartbeat { node_id, api_addr, cluster_addr, last_acked_seq: _, graphs, default_graph } => {
            let info = WorkerInfo::new(&node_id, &api_addr, &cluster_addr);
            state.registry.register(info);

            // Master: compute graph sync commands from the worker's reported
            // graph list vs the master's registry (6.2).
            let sync_commands = if state.is_master {
                compute_graph_sync_commands(&state.gm, &graphs, &default_graph)
            } else {
                Vec::new()
            };

            Ok(Json(ClusterMessage::HeartbeatAck {
                master_time: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u64,
                sync_commands,
            }))
        }
        ClusterMessage::Shutdown { node_id } => {
            state.registry.remove(&node_id);
            Ok(Json(ClusterMessage::HeartbeatAck { master_time: 0, sync_commands: Vec::new() }))
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

/// Compare the worker's reported graph list against the master's registry
/// and produce commands that bring the worker's graphs in sync:
/// - graphs the worker is missing → CreateGraph (with master's config)
/// - graphs whose metadata (description/time_travel) differs → UpdateGraphMeta
/// - graphs whose config (storage/lock/indices) differs → UpdateGraphConfig
/// - graphs the worker has but master doesn't → DeleteGraph
/// - worker's default graph differs → SetDefaultGraph
fn compute_graph_sync_commands(
    gm: &GraphManager,
    worker_graphs: &[crate::cluster::node::WorkerGraphSnapshot],
    worker_default: &str,
) -> Vec<crate::cluster::node::GraphSyncCommand> {
    let (master_graphs, master_default) = gm.get_registry();
    let mut commands = Vec::new();

    for master_meta in &master_graphs {
        match worker_graphs.iter().find(|w| w.meta.name == master_meta.name) {
            Some(worker_snapshot) => {
                let worker_meta = &worker_snapshot.meta;
                if worker_meta.description != master_meta.description
                    || worker_meta.time_travel != master_meta.time_travel
                {
                    commands.push(crate::cluster::node::GraphSyncCommand::UpdateGraphMeta {
                        name: master_meta.name.clone(),
                        description: master_meta.description.clone(),
                        time_travel: master_meta.time_travel,
                    });
                }
                // Compare per-graph config (storage/lock/indices).
                let master_config = gm.get_graph_config(&master_meta.name);
                if master_config != worker_snapshot.config {
                    commands.push(crate::cluster::node::GraphSyncCommand::UpdateGraphConfig {
                        name: master_meta.name.clone(),
                        config: master_config,
                    });
                }
            }
            None => {
                // Worker is missing the graph — create it with the master's config.
                commands.push(crate::cluster::node::GraphSyncCommand::CreateGraph {
                    name: master_meta.name.clone(),
                    description: master_meta.description.clone(),
                    time_travel: master_meta.time_travel,
                    config: gm.get_graph_config(&master_meta.name),
                });
            }
        }
    }

    // Delete graphs the worker has but the master does not.
    for worker_snapshot in worker_graphs {
        let name = &worker_snapshot.meta.name;
        if !master_graphs.iter().any(|m| m.name == *name) {
            commands.push(crate::cluster::node::GraphSyncCommand::DeleteGraph {
                name: name.clone(),
            });
        }
    }

    // Sync the default graph if it differs.
    if !master_default.is_empty() && worker_default != master_default {
        commands.push(crate::cluster::node::GraphSyncCommand::SetDefaultGraph {
            name: master_default,
        });
    }

    commands
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
    let result = proxy_to_api(&state.api_addr, &req, None).await;

    // Tokenizer operations: broadcast directly to workers' tokenizer-sync endpoint.
    // Regular vertex/edge operations are already broadcast by the REST API
    // handlers via ClusterGateway::broadcast, so skip them here.
    if result.success && req.path == "/settings/tokenizer/words" {
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
    }

    Json(result)
}

// ── Cluster execute ──────────────────────────────────────────────────────────

/// POST /cluster/execute — execute a forwarded request on this node's
/// REST API with REPLAYING=true (used by master→worker broadcast).
pub async fn handle_execute(
    State(state): State<ClusterAppState>,
    Json(req): Json<crate::cluster::forward::ForwardedRequest>,
) -> Json<crate::cluster::forward::ForwardedResponse> {
    log::warn!("handle_execute: {} {} (graph={:?})", req.method, req.path, req.headers.get("X-Graph-Name"));
    let req_id = uuid::Uuid::new_v4().to_string();
    crate::graph::graph::INFLIGHT_REQUESTS.lock().unwrap().insert(req_id.clone());
    let result = proxy_to_api(&state.api_addr, &req, Some(&req_id)).await;
    crate::graph::graph::INFLIGHT_REQUESTS.lock().unwrap().remove(&req_id);
    Json(result)
}

/// Build broadcast entries using a raw GraphManager (used by both
/// the cluster forward handler and direct write broadcast).
pub(crate) fn build_broadcast_entries_raw(
    gm: &crate::graph_manager::GraphManager,
    req: &ForwardedRequest,
    result: &ForwardedResponse,
) -> Vec<crate::storage::redo_log::RedoLogEntry> {
    let mut entries = Vec::new();
    let method = req.method.to_uppercase();

    let graph_name = req.headers.get("X-Graph-Name").cloned()
        .unwrap_or_else(|| gm.get_default_name());
    let graph = match gm.get(&graph_name) {
        Ok(g) => g,
        Err(_) => return entries,
    };

    let path_id: Option<u32> = {
        let path = req.path.split('?').next().unwrap_or(&req.path);
        let parts: Vec<&str> = path.split('/').collect();
        parts.last().and_then(|s| s.parse().ok())
    };

    let body = match result.body { Some(ref b) => b.clone(), None => return entries };
    let parsed: std::collections::HashMap<String, serde_json::Value> = match serde_json::from_str(&body) {
        Ok(v) => v, Err(_) => return entries,
    };

    match (method.as_str(), req.path.as_str()) {
        ("POST", "/vertices") | ("PUT", "/vertices") => {
            let id = parsed.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if id == 0 { return entries; }
            let found = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).vertex_id.get(id).copied();
            if let Some(ptr) = found {
                if let Ok(dh) = crate::graph::crud::read_header_by_ptr(&graph, &ptr) {
                    if dh.status != crate::storage::types::DataStatus::Deleted {
                        let payload_len = dh.payload_len as usize;
                        if let Ok(data) = crate::graph::crud::read_data_chunks(
                            &graph, ptr.block_idx, ptr.chunk_offset + 1, dh.payload_len as u16,
                        ) {
                            if let Ok(payload) = crate::graph::serialize::deserialize_vertex(&data[..payload_len]) {
                                if let Ok(serialized) = crate::graph::serialize::serialize_vertex(&payload) {
                                    entries.push(crate::storage::redo_log::RedoLogEntry {
                                        op_type: OpType::VertexCreate,
                                        op_id: id as u64,
                                        data: serialized,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        ("POST", "/edges") | ("PUT", "/edges") => {
            let id = parsed.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if id == 0 { return entries; }
            let found = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).edge_id.get(id).copied();
            if let Some(ptr) = found {
                if let Ok(dh) = crate::graph::crud::read_header_by_ptr(&graph, &ptr) {
                    if dh.status != crate::storage::types::DataStatus::Deleted {
                        let payload_len = dh.payload_len as usize;
                        if let Ok(data) = crate::graph::crud::read_data_chunks(
                            &graph, ptr.block_idx, ptr.chunk_offset + 1, dh.payload_len as u16,
                        ) {
                            if let Ok(payload) = crate::graph::serialize::deserialize_edge(&data[..payload_len]) {
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
        }
        ("DELETE", path) if path.starts_with("/vertices/") => {
            if let Some(vid) = path_id {
                entries.push(crate::storage::redo_log::RedoLogEntry {
                    op_type: OpType::VertexDelete,
                    op_id: vid as u64,
                    data: Vec::new(),
                });
            }
        }
        ("DELETE", path) if path.starts_with("/edges/") => {
            if let Some(eid) = path_id {
                entries.push(crate::storage::redo_log::RedoLogEntry {
                    op_type: OpType::EdgeDelete,
                    op_id: eid as u64,
                    data: Vec::new(),
                });
            }
        }
        _ => {}
    }
    entries
}

/// After a successful forwarded write, build redo-log entries from the
/// actual data stored on the master so workers can replay them correctly.
fn build_broadcast_entries(
    state: &ClusterAppState,
    req: &ForwardedRequest,
    result: &ForwardedResponse,
) -> Vec<crate::storage::redo_log::RedoLogEntry> {
    build_broadcast_entries_raw(&state.gm, req, result)
}

/// Proxy a ForwardedRequest to the master's main API server via HTTP.
async fn proxy_to_api(api_addr: &str, req: &ForwardedRequest, request_id: Option<&str>) -> ForwardedResponse {
    let url = format!(
        "http://{}{}{}",
        api_addr,
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

    // Forward all original request headers (X-Graph-Name, X-Time-Travel, etc.)
    // so the downstream handler receives the full original request context.
    let mut request = request;
    for (k, v) in &req.headers {
        // Skip headers that proxy_to_api sets explicitly or would conflict.
        if k.eq_ignore_ascii_case("host")
            || k.eq_ignore_ascii_case("content-length")
            || k.eq_ignore_ascii_case("content-type")
        {
            continue;
        }
        request = request.header(k.as_str(), v.as_str());
    }

    // Override X-Request-Id if one was provided (for replay detection).
    let request = if let Some(id) = request_id {
        request.header("X-Request-Id", id)
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

    // Write the entry to the graph's redo log and replay it
    // into the in-memory state so the worker can immediately see changes.
    // redo_log.append() uses synchronous Condvar — defer to spawn_blocking.
    let graph_name = entry.graph_name.clone();
    let graph = match state.gm.get(&graph_name) {
        Ok(g) => g,
        Err(e) => {
            log::error!("replicate: failed to get graph '{}': {}", graph_name, e);
            return Json(ReplicationAck {
                worker_id: "local".to_string(),
                acked_seq: entry.cluster_seq,
                success: false,
                error: Some(format!("Failed to get graph '{}': {}", graph_name, e)),
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
#[derive(Deserialize, Serialize)]
pub struct TouchRequest {
    pub vertex_ids: Vec<u32>,
    pub edge_ids: Vec<u32>,
}

/// Apply touch (read access) for vertices/edges: update rank/atime in-place
/// via DataHeader. No WAL entry is needed — rank is soft state persisted
/// at the next checkpoint. In cluster mode, broadcasts the touch to all
/// peers via direct HTTP (no WAL involved) so every node keeps rank/atime
/// in sync.
///
/// Can be called directly from the master's gremlin handler (no HTTP needed)
/// or from the `/cluster/touch` endpoint (worker → master relay).
pub async fn process_touch(
    graph: &Arc<crate::graph::graph::Graph>,
    vertex_ids: &[u32],
    edge_ids: &[u32],
    registry: Option<&NodeRegistry>,
) {
    // 1. Apply local touch (in-place DataHeader update, no WAL).
    apply_touch(graph, vertex_ids, edge_ids);

    // 2. Broadcast touch to all cluster peers via direct HTTP.
    //    Each peer applies the touch locally on receipt.
    //    No WAL entries are generated — rank/atime are soft state.
    if let Some(reg) = registry {
        let workers = reg.alive_workers();
        if !workers.is_empty() {
            let req = TouchRequest {
                vertex_ids: vertex_ids.to_vec(),
                edge_ids: edge_ids.to_vec(),
            };
            let workers_for_broadcast = workers.clone();
            tokio::spawn(async move {
                for worker in &workers_for_broadcast {
                    let url = format!("http://{}/cluster/touch", worker.cluster_addr);
                    let client = reqwest::Client::new();
                    if let Err(e) = client.post(&url).json(&req).send().await {
                        log::debug!("broadcast touch to {} failed: {}", worker.node_id, e);
                    }
                }
            });
        }
    }
}

/// Update rank/atime in-place via DataHeader for accessed vertices/edges.
/// The read itself triggers the in-place update — no WAL entries needed.
fn apply_touch(
    graph: &Arc<crate::graph::graph::Graph>,
    vertex_ids: &[u32],
    edge_ids: &[u32],
) {
    for vid in vertex_ids {
        if let Err(e) = crate::graph::locked::get_vertex_locked(graph, *vid) {
            log::debug!("touch vertex {}: {}", vid, e);
        }
    }
    for eid in edge_ids {
        if let Err(e) = crate::graph::locked::get_edge_locked(graph, *eid) {
            log::debug!("touch edge {}: {}", eid, e);
        }
    }
}

/// POST /cluster/touch
///
/// Receive touch (read report) from any cluster peer. All nodes apply
/// the touch locally by updating rank/atime in-place via DataHeader
/// (no WAL needed). On the master, the touch is also relay-broadcast
/// to all workers so every node stays in sync.
async fn handle_touch(
    State(state): State<ClusterAppState>,
    Json(req): Json<TouchRequest>,
) -> StatusCode {
    let default_name = state.gm.get_default_name();
    let graph = match state.gm.get(&default_name) {
        Ok(g) => g,
        Err(e) => {
            log::warn!("touch: failed to get default graph: {}", e);
            return StatusCode::OK;
        }
    };

    if state.is_master {
        // Master: apply locally + relay-broadcast to all workers.
        process_touch(&graph, &req.vertex_ids, &req.edge_ids, Some(&state.registry)).await;
    } else {
        // Worker: apply locally only.
        process_touch(&graph, &req.vertex_ids, &req.edge_ids, None).await;
    }

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
