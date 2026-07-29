//! REST API handlers for the new block-based graph engine.
//!
//! These handlers replace the old `src/gremlin/` routes and operate on
//! `Arc<Graph>` through `GraphManager`.

use std::sync::atomic::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use std::sync::Mutex;
use tokio::task::spawn_blocking;

use crate::cluster::node::NodeRegistry;

use crate::config::Settings;
use crate::documents::DocumentManager;
use crate::graph::graph::Graph;
use crate::graph::gremlin::{execute_gremlin, GremlinQuery, GremlinResponse, GremlinResult};
use crate::graph_manager::GraphManager;
use crate::task::{TaskManager, TaskResponse, TaskStatus, default_extraction_steps, update_step, compute_overall_pct};

pub mod settings;
pub mod tokenizer_settings;
pub mod indices;
use crate::storage::types::{PropertyValue, StorageResult};
use crate::config::NodeRole;

/// If this node is a cluster worker, forward the write request to the master
/// and return the master's JSON response. Returns None on master / standalone.
pub(crate) async fn try_forward_json(
    state: &AppState,
    method: &str,
    path: &str,
    query: Option<&str>,
    graph_name: Option<&str>,
    body: Option<&str>,
) -> Option<Result<Json<serde_json::Value>, StatusCode>> {
    // Extract settings before the await point (MutexGuard is not Send).
    let (is_worker, master_addr) = {
        let settings = state.settings.lock().unwrap();
        let is_worker = settings.cluster.enabled && settings.cluster.role == NodeRole::Worker;
        let addr = state.master_api_addr.clone();
        (is_worker, addr)
    };
    // During replay, skip forwarding to prevent recursion.
    if crate::graph::graph::REPLAYING.load(Ordering::Relaxed) {
        return None;
    }
    if !is_worker { return None; }
    let master_addr = master_addr.as_ref()?;
    let req = crate::cluster::forward::ForwardedRequest {
        method: method.to_string(),
        path: path.to_string(),
        query: query.map(|s| s.to_string()),
        body: body.map(|s| s.to_string()),
        graph: graph_name.map(|s| s.to_string()),
    };
    match crate::cluster::forward::forward_write(master_addr, &req).await {
        Ok(resp) => {
            if resp.success {
                if let Some(body_str) = resp.body {
                    match serde_json::from_str(&body_str) {
                        Ok(val) => Some(Ok(Json(val))),
                        Err(_) => Some(Err(StatusCode::INTERNAL_SERVER_ERROR)),
                    }
                } else {
                    Some(Ok(Json(serde_json::json!({"status": "ok"}))))
                }
            } else {
                Some(Err(StatusCode::from_u16(resp.status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)))
            }
        }
        Err(e) => {
            log::warn!("Forward to master failed: {}", e);
            Some(Err(StatusCode::BAD_GATEWAY))
        }
    }
}

/// Same as try_forward_json but for handlers that return StatusCode.
pub(crate) async fn try_forward_status(
    state: &AppState,
    method: &str,
    path: &str,
    query: Option<&str>,
    graph_name: Option<&str>,
) -> Option<StatusCode> {
    let (is_worker, master_addr) = {
        let settings = state.settings.lock().unwrap();
        let is_worker = settings.cluster.enabled && settings.cluster.role == NodeRole::Worker;
        let addr = state.master_api_addr.clone();
        (is_worker, addr)
    };
    // During replay, skip forwarding to prevent recursion.
    if crate::graph::graph::REPLAYING.load(Ordering::Relaxed) {
        return None;
    }
    if !is_worker { return None; }
    let master_addr = master_addr.as_ref()?;
    let req = crate::cluster::forward::ForwardedRequest {
        method: method.to_string(),
        path: path.to_string(),
        query: query.map(|s| s.to_string()),
        body: None,
        graph: graph_name.map(|s| s.to_string()),
    };
    match crate::cluster::forward::forward_write(master_addr, &req).await {
        Ok(resp) => {
            if resp.success {
                Some(StatusCode::OK)
            } else {
                Some(StatusCode::from_u16(resp.status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
            }
        }
        Err(_) => Some(StatusCode::BAD_GATEWAY),
    }
}

/// Shared application state for all graph routes.
#[derive(Clone)]
pub struct AppState {
    pub gm: Arc<GraphManager>,
    pub settings: Arc<Mutex<Settings>>,
    pub doc_mgr: DocumentManager,
    pub task_mgr: TaskManager,
    /// NodeRegistry for cluster-mode broadcasts (None in standalone).
    pub cluster_registry: Option<Arc<NodeRegistry>>,
    /// Master's API address for worker→master forwarding (None on master / standalone).
    pub master_api_addr: Option<String>,
}

/// Broadcast a ForwardedRequest from the master to all workers' REST API.
/// Used for operations that don't use the graph engine's WAL (documents,
/// graph metadata, indices, etc.). Workers process the request via their
/// own REST handlers with REPLAYING=true to prevent recursion.
pub(crate) fn broadcast_request_to_workers(
    cluster_registry: &Option<Arc<NodeRegistry>>,
    method: &str,
    path: &str,
    graph_name: Option<&str>,
    body: Option<&str>,
) {
    // During replay, skip broadcasting to prevent recursion.
    if crate::graph::graph::REPLAYING.load(Ordering::Relaxed) {
        return;
    }
    let Some(registry) = cluster_registry.as_ref() else {
        log::warn!("broadcast_to_workers: no registry");
        return;
    };
    let workers = registry.alive_workers();
    if workers.is_empty() {
        log::warn!("broadcast_to_workers: alive_workers empty");
        return;
    }

    let payload = crate::cluster::forward::ForwardedRequest {
        method: method.to_string(),
        path: path.to_string(),
        query: None,
        body: body.map(|s| s.to_string()),
        graph: graph_name.map(|s| s.to_string()),
    };

    for worker in workers {
        let url = format!("http://{}/cluster/execute", worker.cluster_addr);
        let client = reqwest::Client::new();
        let mut req = client.request(
            method.parse().unwrap_or(reqwest::Method::POST),
            &url,
        );
        if let Some(ref body_str) = payload.body {
            req = req.header("Content-Type", "application/json").body(body_str.clone());
        }
        if let Some(ref gn) = payload.graph {
            req = req.header("X-Graph-Name", gn.clone());
        }
        let _ = tokio::spawn(async move {
            if let Err(e) = req.send().await {
                log::warn!("Broadcast to worker {} failed: {}", worker.node_id, e);
            }
        });
    }
}

/// Broadcast a write result from the master to all workers.
/// Called by write handlers after a successful mutation on the master.
pub(crate) fn broadcast_write_result(
    cluster_registry: &Option<Arc<NodeRegistry>>,
    graph: &Arc<Graph>,
    method: &str,
    path: &str,
    graph_name: &str,
    response_body: &str,
) {
    // During replay, skip broadcasting to prevent recursion.
    if crate::graph::graph::REPLAYING.load(Ordering::Relaxed) {
        return;
    }
    let Some(registry) = cluster_registry.as_ref() else {
        log::warn!("broadcast_to_workers: no registry");
        return;
    };
    let workers = registry.alive_workers();
    if workers.is_empty() {
        log::warn!("broadcast_to_workers: alive_workers empty");
        return;
    }

    // Determine op_type and op_id.
    let (op_type, op_id) = match method {
        "POST" => {
            let id = serde_json::from_str::<serde_json::Value>(response_body)
                .ok().and_then(|v| v.get("id").and_then(|id| id.as_u64()))
                .unwrap_or(0);
            if id == 0 { return; }
            let op = if path == "/vertices" || path.starts_with("/vertices/") {
                crate::storage::types::OpType::VertexCreate
            } else if path == "/edges" || path.starts_with("/edges/") {
                crate::storage::types::OpType::EdgeCreate
            } else { return };
            (op, id)
        }
        "PUT" => {
            let id = serde_json::from_str::<serde_json::Value>(response_body)
                .ok().and_then(|v| v.get("id").and_then(|id| id.as_u64()))
                .unwrap_or(0);
            if id == 0 { return; }
            let op = if path.starts_with("/vertices/") {
                crate::storage::types::OpType::VertexUpdate
            } else if path.starts_with("/edges/") {
                crate::storage::types::OpType::EdgeUpdate
            } else { return };
            (op, id)
        }
        "DELETE" => {
            let clean = path.split('?').next().unwrap_or(path);
            let id: u64 = match clean.rsplit('/').next().and_then(|s| s.parse().ok()) {
                Some(v) => v, None => return,
            };
            let op = if path.starts_with("/vertices/") {
                crate::storage::types::OpType::VertexDelete
            } else if path.starts_with("/edges/") {
                crate::storage::types::OpType::EdgeDelete
            } else { return };
            (op, id)
        }
        _ => return,
    };

    // Read the actual data from the graph to build a proper replay entry.
    let data = if method != "DELETE" {
        let g = graph.clone();
        (move || -> Vec<u8> {
            let id_u32 = op_id as u32;
            let ptr = g.memory_index.read().ok()
                .and_then(|mi| {
                    if op_type == crate::storage::types::OpType::VertexCreate
                        || op_type == crate::storage::types::OpType::VertexUpdate {
                        mi.vertex_id.get(id_u32).copied()
                    } else if op_type == crate::storage::types::OpType::EdgeCreate
                        || op_type == crate::storage::types::OpType::EdgeUpdate {
                        mi.edge_id.get(id_u32).copied()
                    } else { None }
                });
            let Some(ptr) = ptr else { return Vec::new() };
            let Ok(dh) = crate::graph::crud::read_header_by_ptr(&g, &ptr) else { return Vec::new() };
            let plen = dh.payload_len as usize;
            let Ok(raw) = crate::graph::crud::read_data_chunks(
                &g, ptr.block_idx, ptr.chunk_offset + 1, dh.payload_len as u16,
            ) else { return Vec::new() };
            let data_for_ser = &raw[..plen.min(raw.len())];
            // Re-serialize to ensure valid WAL format (avoids chunk padding issues)
            let result = if op_type == crate::storage::types::OpType::VertexCreate
                || op_type == crate::storage::types::OpType::VertexUpdate {
                crate::graph::serialize::deserialize_vertex(data_for_ser)
                    .and_then(|p| crate::graph::serialize::serialize_vertex(&p))
            } else {
                crate::graph::serialize::deserialize_edge(data_for_ser)
                    .and_then(|p| crate::graph::serialize::serialize_edge(&p))
            };
            result.unwrap_or_default()
        })()
    } else {
        Vec::new()
    };
    if data.is_empty() && method != "DELETE" {
        log::warn!("broadcast_write: empty data for {} {} op={:?}", method, path, op_type);
    }

    let entry = crate::storage::redo_log::RedoLogEntry { op_type, op_id, data };
    let reg = registry.clone();
    let gn = graph_name.to_string();
    let w = workers.clone();

    tokio::spawn(async move {
        let seq = reg.next_seq();
        let replicated = crate::cluster::replication::ReplicatedEntry {
            cluster_seq: seq,
            entry,
            graph_name: gn,
            master_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
        };
        let results = crate::cluster::replication::broadcast_entry(&w, &replicated).await;
        for (wid, res) in &results {
            if let Err(e) = res {
                log::warn!("Broadcast to worker {} failed: {}", wid, e);
            }
        }
    });
}

/// Build the axum router for all block-engine graph routes.
pub fn build_router(
    gm: Arc<GraphManager>,
    settings: Settings,
    cluster_registry: Option<Arc<NodeRegistry>>,
    master_api_addr: Option<String>,
) -> axum::Router {
    let doc_mgr = DocumentManager::new(&settings.graph.storage.data_dir);
    let state = AppState {
        gm,
        settings: Arc::new(Mutex::new(settings)),
        doc_mgr,
        task_mgr: TaskManager::new(),
        cluster_registry,
        master_api_addr,
    };

    use axum::routing::{delete, get, post, put};

    axum::Router::new()
        // UI — serve the frontend SPA
        .route("/", get(crate::ui_serve::ui_root_handler))
        .route("/ui", get(crate::ui_serve::ui_root_handler))
        .route("/ui/*path", get(crate::ui_serve::ui_handler))
        // Graph lifecycle
        .route("/graphs", get(list_graphs))
        .route("/graphs", post(create_graph))
        .route("/graphs", put(set_default_graph))
        .route("/graphs/:name", delete(delete_graph))
        .route("/graphs/:name", put(update_graph_meta))
        .route("/graphs/:name/config", get(get_graph_config_handler))
        .route("/graphs/:name/config", put(put_graph_config_handler))
        // Query
        .route("/gremlin", post(handle_gremlin))
        .route("/search", get(handle_search))
        // Vertex CRUD
        .route("/vertices", post(create_vertex))
        .route("/vertices/:id", put(update_vertex))
        .route("/vertices/:id", delete(delete_vertex))
        .route("/vertices/:id/meta", get(handle_get_vertex_meta))
        .route("/vertices/:id/meta", put(handle_update_vertex_meta))
        // Edge CRUD
        .route("/edges", post(create_edge))
        .route("/edges/:id", put(update_edge))
        .route("/edges/:id", delete(delete_edge))
        .route("/edges/:id/meta", get(handle_get_edge_meta))
        .route("/edges/:id/meta", put(handle_update_edge_meta))
        // Settings
        .route("/settings/graph/search", get(settings::get_search_settings))
        .route("/settings/graph/search", put(settings::update_search_settings))
        .route("/settings/llm", get(settings::get_llm_settings))
        .route("/settings/llm", put(settings::update_llm_settings))
        .route("/settings/graph/rank", get(settings::get_rank_settings))
        .route("/settings/graph/rank", put(settings::update_rank_settings))
        .route("/settings/web-search", get(settings::get_web_search_settings))
        .route("/settings/web-search", put(settings::update_web_search_settings))
        .route("/proxy/web-search", post(settings::web_search_proxy))
        .route("/settings/tokenizer/words", get(tokenizer_settings::get_tokenizer_words))
        .route("/settings/tokenizer/words", post(tokenizer_settings::add_tokenizer_words))
        .route("/settings/tokenizer/words", delete(tokenizer_settings::remove_tokenizer_words))
        // Data import
        // Batch operations
        .route("/batch/load", post(handle_batch_import))
        .route("/batch/delete", post(handle_batch_delete))
        // Health
        .route("/health", get(health_check))
        // MaaS — OpenAI-compatible proxy
        .route("/proxy/openai/v1/models", get(crate::maas::openai::list_models_handler))
        .route("/proxy/openai/v1/chat/completions", post(crate::maas::openai::chat_completions_handler))
        // Document CRUD
        .route("/documents", get(list_documents))
        .route("/documents", post(create_document))
        .route("/documents/:id", get(get_document))
        .route("/documents/:id", put(update_document))
        .route("/documents/:id", delete(delete_document))
        .route("/documents/:id/content", get(get_document_content))
        // Extraction
        .route("/extract", post(submit_extraction))
        .route("/documents/:id/extract", post(extract_document_handler))
        // Tasks (generic async task tracking)
        .route("/tasks/:task_id", get(get_task_handler))
        .route("/tasks", get(list_tasks_handler))
        // Custom property indices
        .route("/indices/vertex/properties", post(indices::create_vertex_property_index))
        .route("/indices/vertex/properties", get(indices::list_vertex_property_indices))
        .route("/indices/vertex/properties/:key", get(indices::show_vertex_property_index))
        .route("/indices/vertex/properties/:key", delete(indices::delete_vertex_property_index))
        .route("/indices/vertex/properties", delete(indices::delete_vertex_property_indices))
        .route("/indices/edge/properties", post(indices::create_edge_property_index))
        .route("/indices/edge/properties", get(indices::list_edge_property_indices))
        .route("/indices/edge/properties/:key", get(indices::show_edge_property_index))
        .route("/indices/edge/properties/:key", delete(indices::delete_edge_property_index))
        .route("/indices/edge/properties", delete(indices::delete_edge_property_indices))
        // Shared state
        .with_state(state)
}

// ── Health ──────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: &'static str,
    pub uptime_secs: u64,
    pub graphs: usize,
    pub cluster_enabled: bool,
}

pub async fn health_check(
    State(state): State<AppState>,
) -> Json<HealthResponse> {
    let graphs = state.gm.list().unwrap_or_default().len();
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: 0,
        graphs,
        cluster_enabled: false,
    })
}

// ── Helper: resolve graph name from X-Graph-Name header ─────────────────────

/// Resolve graph name from X-Graph-Name header only, then fall back to default.
fn resolve_graph_from_request(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> StorageResult<Arc<Graph>> {
    let name = headers
        .get("X-Graph-Name")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| state.gm.get_default_name());
    state.gm.get(&name)
}

/// Serde helper: default value `true` for optional boolean fields.
fn default_true() -> bool { true }

// ── POST /gremlin2 ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct GremlinParams {
}

pub async fn handle_gremlin(
    State(state): State<AppState>,
    Query(_params): Query<GremlinParams>,
    headers: axum::http::HeaderMap,
    Json(mut query): Json<GremlinQuery>,
) -> Json<GremlinResponse> {
    let graph = match resolve_graph_from_request(&state, &headers) {
        Ok(g) => g,
        Err(e) => {
            return Json(GremlinResponse {
                success: false,
                data: vec![],
                error: Some(e.to_string()),
            });
        }
    };

    // Extract time-travel timestamp from header (microseconds since epoch).
    let time_travel_at: Option<u64> = headers
        .get("X-Time-Travel")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    // Inject match_mode and auto-append traverse step if search with traverse enabled.
    let should_inject = query.steps.last().map_or(false, |s| {
        matches!(s, crate::graph::gremlin::GremlinStep::Search { .. })
    });
    if should_inject {
        let mode = query.steps.last().and_then(|s| {
            if let crate::graph::gremlin::GremlinStep::Search { ref mode, .. } = s {
                mode.as_deref()
            } else { None }
        }).unwrap_or("greedy");

        let settings = state.settings.lock().unwrap();
        let cfg = if mode == "exact" { &settings.graph.search.exact } else { &settings.graph.search.greedy };

        // Inject match_mode into the search step if not already set.
        if let Some(crate::graph::gremlin::GremlinStep::Search { ref mut match_mode, .. }) = query.steps.last_mut() {
            if match_mode.is_none() {
                *match_mode = Some(cfg.match_mode.clone());
            }
        }

        if cfg.traverse {
            query.steps.push(crate::graph::gremlin::GremlinStep::Traverse {
                decay: Some(cfg.decay),
                activate: Some(cfg.activate),
                max_depth: Some(cfg.depth),
                min_score: Some(cfg.score),
            });
        }
    }

    let response = execute_gremlin(&graph, &query, time_travel_at);

    // If this node is a worker in cluster mode, report read vertex/edge IDs
    // to the master so it can update their rank and atime.
    if response.success && !response.data.is_empty() {
        let settings = state.settings.lock().unwrap();
        if settings.cluster.enabled && settings.cluster.role == crate::config::NodeRole::Worker {
            if let Some(ref master_addr) = settings.cluster.master_addr {
                let mut vertex_ids = Vec::new();
                let mut edge_ids = Vec::new();
                for item in &response.data {
                    match item {
                        crate::graph::gremlin::GremlinResult::Vertex { id, .. } => {
                            vertex_ids.push(*id);
                        }
                        crate::graph::gremlin::GremlinResult::Edge { id, .. } => {
                            edge_ids.push(*id);
                        }
                        _ => {}
                    }
                }
                if !vertex_ids.is_empty() || !edge_ids.is_empty() {
                    let master_addr = master_addr.clone();
                    std::thread::spawn(move || {
                        let client = reqwest::blocking::Client::new();
                        let touch_url = format!("http://{}/cluster/touch", master_addr);
                        let body = serde_json::json!({
                            "vertex_ids": vertex_ids,
                            "edge_ids": edge_ids,
                        });
                        if let Err(e) = client.post(&touch_url).json(&body).send() {
                            log::debug!("touch report to master failed: {}", e);
                        }
                    });
                }
            }
        }
    }

    // On the master (standalone or cluster), call process_touch directly
    // to persist metadata to the redo log and optionally broadcast.
    if response.success && !response.data.is_empty() {
        let settings = state.settings.lock().unwrap();
        if !settings.cluster.enabled || settings.cluster.role == crate::config::NodeRole::Master {
            let mut vertex_ids = Vec::new();
            let mut edge_ids = Vec::new();
            for item in &response.data {
                match item {
                    GremlinResult::Vertex { id, .. } => vertex_ids.push(*id),
                    GremlinResult::Edge { id, .. } => edge_ids.push(*id),
                    _ => {}
                }
            }
            let has_ids = !vertex_ids.is_empty() || !edge_ids.is_empty();
            let reg = state.cluster_registry.clone();
            let do_touch = settings.graph.rank.auto_inc_rank_when_read;
            drop(settings);
            if has_ids && do_touch {
                let default_name = state.gm.get_default_name();
                if let Ok(g) = state.gm.get(&default_name) {
                    tokio::spawn(async move {
                        crate::cluster::server::process_touch(
                            &g, &vertex_ids, &edge_ids, reg.as_deref(),
                        ).await;
                    });
                }
            }
        }
    }

    Json(response)
}

// ── POST /search2 ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SearchParams {
    pub text: String,
    pub mode: Option<String>,
    pub limit: Option<u32>,
}

pub async fn handle_search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
    headers: axum::http::HeaderMap,
) -> Json<GremlinResponse> {
    let graph = match resolve_graph_from_request(&state, &headers) {
        Ok(g) => g,
        Err(e) => {
            return Json(GremlinResponse {
                success: false,
                data: vec![],
                error: Some(e.to_string()),
            });
        }
    };

    use crate::graph::gremlin::GremlinStep;
    let time_travel_at: Option<u64> = headers
        .get("X-Time-Travel")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let query = GremlinQuery {
        steps: vec![GremlinStep::Search {
            text: params.text,
            mode: params.mode,
            match_mode: None,
            limit: params.limit,
            min_rank: None,
        }],
    };

    let response = execute_gremlin(&graph, &query, time_travel_at);
    Json(response)
}

// ── Shared query types ──────────────────────────────────────────────────────

// ── POST /vertices ─────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
pub struct CreateVertexBody {
    pub name: String,
    pub labels: Option<Vec<String>>,
    pub keywords: Option<Vec<String>>,
    #[serde(default)]
    pub properties: std::collections::HashMap<String, crate::storage::types::PropertyValue>,
}

#[derive(Serialize)]
pub struct CreateVertexResponse {
    pub id: u32,
}

pub async fn create_vertex(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateVertexBody>,
) -> Result<Json<CreateVertexResponse>, StatusCode> {
    // Worker → Master forwarding
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    if let Some(resp) = try_forward_json(
        &state, "POST", "/vertices", None, graph_name,
        Some(&serde_json::to_string(&body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?),
    ).await {
        return resp.map(|json| {
            let id = json.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            Json(CreateVertexResponse { id })
        });
    }

    let graph = resolve_graph_from_request(&state, &headers)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let vid = crate::graph::locked::create_vertex_locked(
        &graph,
        &body.name,
        &body.labels.unwrap_or_default(),
        &body.keywords.unwrap_or_default(),
        &body.properties,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Broadcast to workers in cluster mode.
    let graph_name = graph.name.clone();
    let response_body = serde_json::json!({"id": vid}).to_string();
    broadcast_write_result(
        &state.cluster_registry, &graph,
        "POST", "/vertices",
        &graph_name, &response_body,
    );

    Ok(Json(CreateVertexResponse { id: vid }))
}

// ── PUT /vertices2/:id ──────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
pub struct UpdateVertexBody {
    pub name: Option<String>,
    pub labels: Option<Vec<String>>,
    pub keywords: Option<Vec<String>>,
    pub properties: Option<std::collections::HashMap<String, crate::storage::types::PropertyValue>>,
}

pub async fn update_vertex(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    headers: axum::http::HeaderMap,
    Json(body): Json<UpdateVertexBody>,
) -> StatusCode {
    // Worker → Master forwarding
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let path = format!("/vertices/{}", id);
    if let Some(resp) = try_forward_json(
        &state, "PUT", &path, None, graph_name,
        Some(&serde_json::to_string(&body).unwrap_or_default()),
    ).await {
        return if resp.is_ok() { StatusCode::OK } else { StatusCode::BAD_GATEWAY };
    }

    let graph = match resolve_graph_from_request(&state, &headers) {
        Ok(g) => g,
        Err(_) => return StatusCode::NOT_FOUND,
    };

    match crate::graph::locked::update_vertex_locked(
        &graph,
        id,
        body.name.as_deref(),
        body.labels.as_deref(),
        body.keywords.as_deref(),
        body.properties.as_ref(),
        true,
    ) {
        Ok(_) => {
            let graph_name = graph.name.clone();
            let response_body = serde_json::json!({"id": id}).to_string();
            broadcast_write_result(&state.cluster_registry, &graph, "PUT", &format!("/vertices/{}", id), &graph_name, &response_body);
            StatusCode::OK
        }
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── DELETE /vertices/:id ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DeleteVertexParams {
    pub force: Option<bool>,
}

pub async fn delete_vertex(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Query(params): Query<DeleteVertexParams>,
    headers: axum::http::HeaderMap,
) -> StatusCode {
    // Worker → Master forwarding
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let query_str = params.force.map(|f| format!("force={}", f));
    let path = format!("/vertices/{}", id);
    if let Some(status) = try_forward_status(
        &state, "DELETE", &path, query_str.as_deref(), graph_name,
    ).await {
        return status;
    }

    let graph = match resolve_graph_from_request(&state, &headers) {
        Ok(g) => g,
        Err(_) => return StatusCode::NOT_FOUND,
    };

    let tt_enabled = state.gm.time_travel_enabled(&graph.name);
    let force = match params.force {
        Some(false) if !tt_enabled => return StatusCode::BAD_REQUEST,
        Some(v) => v,
        None => !tt_enabled,
    };
    let result = if force {
        crate::graph::locked::hard_delete_vertex_locked(&graph, id)
    } else {
        crate::graph::locked::soft_delete_vertex_locked(&graph, id)
    };

    match result {
        Ok(_) => {
            let graph_name = graph.name.clone();
            let path = format!("/vertices/{}?force=true", id);
            broadcast_write_result(
                &state.cluster_registry, &graph,
                "DELETE", &path, &graph_name, "{}",
            );
            StatusCode::OK
        }
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── GET /vertices/:id/meta ──────────────────────────────────────────────────

/// Read a vertex's full metadata (status, version, ctime, mtime, atime, rank).
/// Does NOT trigger any rank/atime update.
pub async fn handle_get_vertex_meta(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    headers: axum::http::HeaderMap,
) -> Json<serde_json::Value> {
    let graph = match resolve_graph_from_request(&state, &headers) {
        Ok(g) => g,
        Err(e) => return Json(serde_json::json!({"error": e.to_string()})),
    };
    let _vlock = graph.locks.read_vertex(id);
    let ptr = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).vertex_id.get(id).copied();
    let result = ptr.and_then(|p| crate::graph::crud::read_header_by_ptr(&graph, &p).ok());
    drop(_vlock);
    match result {
        Some(header) => Json(serde_json::json!({
            "success": true,
            "status": header.status as u8,
            "version": header.version,
            "ctime": header.ctime,
            "mtime": header.mtime,
            "atime": header.atime,
            "rank": header.rank,
        })),
        None => Json(serde_json::json!({"success": false, "error": "not found"})),
    }
}

// ── PUT /vertices/:id/meta ─────────────────────────────────────────────────

/// Update a vertex's rank and/or atime. Body: `{"rank": u32, "atime": u64}`.
/// Either field is optional — only provided fields are updated.
pub async fn handle_update_vertex_meta(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    headers: axum::http::HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    let new_rank = body.get("rank").and_then(|v| v.as_u64()).map(|v| v as u32);
    let new_atime = body.get("atime").and_then(|v| v.as_u64());
    let new_name = body.get("name").and_then(|v| v.as_str());
    if new_rank.is_none() && new_atime.is_none() && new_name.is_none() {
        return StatusCode::BAD_REQUEST;
    }
    let graph = match resolve_graph_from_request(&state, &headers) {
        Ok(g) => g,
        Err(_) => return StatusCode::NOT_FOUND,
    };
    let _meta = graph.locks.read_metadata();
    let _vlock = graph.locks.write_vertex(id);

    // If name is being changed, it requires a full payload update.
    // For rank/atime only, use the lightweight meta update.
    let result = if new_name.is_some() {
        // Delegate to update_vertex for full payload rewrite.
        crate::graph::crud::update_vertex(&graph, id, new_name, None, None, None, false)
            .and_then(|_| {
                if new_rank.is_some() || new_atime.is_some() {
                    crate::graph::crud::update_vertex_meta(&graph, id, new_rank, new_atime)
                } else {
                    Ok(())
                }
            })
    } else {
        crate::graph::crud::update_vertex_meta(&graph, id, new_rank, new_atime)
    };

    drop(_vlock);
    drop(_meta);
    match result {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── POST /edges ─────────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
pub struct CreateEdgeBody {
    pub name: String,
    pub source: u32,
    pub target: u32,
    pub labels: Option<Vec<String>>,
    pub keywords: Option<Vec<String>>,
    pub strength: Option<f32>,
    #[serde(default)]
    pub properties: std::collections::HashMap<String, crate::storage::types::PropertyValue>,
}

#[derive(Serialize)]
pub struct CreateEdgeResponse {
    pub id: u32,
}

pub async fn create_edge(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateEdgeBody>,
) -> Result<Json<CreateEdgeResponse>, StatusCode> {
    // Worker → Master forwarding
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    if let Some(resp) = try_forward_json(
        &state, "POST", "/edges", None, graph_name,
        Some(&serde_json::to_string(&body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?),
    ).await {
        return resp.map(|json| {
            let id = json.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            Json(CreateEdgeResponse { id })
        });
    }

    let graph = resolve_graph_from_request(&state, &headers)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let eid = crate::graph::locked::create_edge_locked(
        &graph, body.source, body.target,
        &body.name,
        &body.labels.unwrap_or_default(),
        &body.keywords.unwrap_or_default(),
        body.strength.unwrap_or(1.0),
        &body.properties,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let graph_name = graph.name.clone();
    let response_body = serde_json::json!({"id": eid}).to_string();
    broadcast_write_result(&state.cluster_registry, &graph, "POST", "/edges", &graph_name, &response_body);

    Ok(Json(CreateEdgeResponse { id: eid }))
}

// ── PUT /edges ──────────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
pub struct UpdateEdgeBody {
    pub name: Option<String>,
    pub labels: Option<Vec<String>>,
    pub keywords: Option<Vec<String>>,
    pub strength: Option<f32>,
    pub properties: Option<std::collections::HashMap<String, crate::storage::types::PropertyValue>>,
}

pub async fn update_edge(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    headers: axum::http::HeaderMap,
    Json(body): Json<UpdateEdgeBody>,
) -> StatusCode {
    // Worker → Master forwarding
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let path = format!("/edges/{}", id);
    if let Some(resp) = try_forward_json(
        &state, "PUT", &path, None, graph_name,
        Some(&serde_json::to_string(&body).unwrap_or_default()),
    ).await {
        return if resp.is_ok() { StatusCode::OK } else { StatusCode::BAD_GATEWAY };
    }

    let graph = match resolve_graph_from_request(&state, &headers) {
        Ok(g) => g,
        Err(_) => return StatusCode::NOT_FOUND,
    };

    match crate::graph::locked::update_edge_locked(
        &graph,
        id,
        body.name.as_deref(),
        body.labels.as_deref(),
        body.keywords.as_deref(),
        body.strength,
        body.properties.as_ref(),
        true,
    ) {
        Ok(_) => {
            let graph_name = graph.name.clone();
            broadcast_write_result(&state.cluster_registry, &graph, "PUT", &format!("/edges/{}", id), &graph_name, &serde_json::json!({"id": id}).to_string());
            StatusCode::OK
        }
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── DELETE /edges ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DeleteEdgeParams {
    pub force: Option<bool>,
}

pub async fn delete_edge(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Query(params): Query<DeleteEdgeParams>,
    headers: axum::http::HeaderMap,
) -> StatusCode {
    // Worker → Master forwarding
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let query_str = params.force.map(|f| format!("force={}", f));
    let path = format!("/edges/{}", id);
    if let Some(status) = try_forward_status(
        &state, "DELETE", &path, query_str.as_deref(), graph_name,
    ).await {
        return status;
    }

    let graph = match resolve_graph_from_request(&state, &headers) {
        Ok(g) => g,
        Err(_) => return StatusCode::NOT_FOUND,
    };

    let tt_enabled = state.gm.time_travel_enabled(&graph.name);
    let force = match params.force {
        Some(false) if !tt_enabled => return StatusCode::BAD_REQUEST,
        Some(v) => v,
        None => !tt_enabled,
    };
    let result = if force {
        crate::graph::locked::hard_delete_edge_locked(&graph, id)
    } else {
        crate::graph::locked::soft_delete_edge_locked(&graph, id)
    };

    match result {
        Ok(_) => {
            let graph_name = graph.name.clone();
            broadcast_write_result(&state.cluster_registry, &graph, "DELETE", &format!("/edges/{}", id), &graph_name, "{}");
            StatusCode::OK
        }
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── GET /edges/:id/meta ────────────────────────────────────────────────────

/// Read an edge's full metadata (status, version, ctime, mtime, atime, rank).
/// Does NOT trigger any rank/atime update.
pub async fn handle_get_edge_meta(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    headers: axum::http::HeaderMap,
) -> Json<serde_json::Value> {
    let graph = match resolve_graph_from_request(&state, &headers) {
        Ok(g) => g,
        Err(e) => return Json(serde_json::json!({"error": e.to_string()})),
    };
    let _elock = graph.locks.read_edge(id);
    let ptr = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).edge_id.get(id).copied();
    let result = ptr.and_then(|p| crate::graph::crud::read_header_by_ptr(&graph, &p).ok());
    drop(_elock);
    match result {
        Some(header) => Json(serde_json::json!({
            "success": true,
            "status": header.status as u8,
            "version": header.version,
            "ctime": header.ctime,
            "mtime": header.mtime,
            "atime": header.atime,
            "rank": header.rank,
        })),
        None => Json(serde_json::json!({"success": false, "error": "not found"})),
    }
}

// ── PUT /edges/:id/meta ───────────────────────────────────────────────────

/// Update an edge's rank and/or atime. Body: `{"rank": u32, "atime": u64}`.
pub async fn handle_update_edge_meta(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    headers: axum::http::HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    let new_rank = body.get("rank").and_then(|v| v.as_u64()).map(|v| v as u32);
    let new_atime = body.get("atime").and_then(|v| v.as_u64());
    let new_name = body.get("name").and_then(|v| v.as_str());
    if new_rank.is_none() && new_atime.is_none() && new_name.is_none() {
        return StatusCode::BAD_REQUEST;
    }
    let graph = match resolve_graph_from_request(&state, &headers) {
        Ok(g) => g,
        Err(_) => return StatusCode::NOT_FOUND,
    };
    let _meta = graph.locks.read_metadata();
    let _elock = graph.locks.write_edge(id);

    let result = if new_name.is_some() {
        crate::graph::crud::update_edge(&graph, id, new_name, None, None, None, None, false)
            .and_then(|_| {
                if new_rank.is_some() || new_atime.is_some() {
                    crate::graph::crud::update_edge_meta(&graph, id, new_rank, new_atime)
                } else {
                    Ok(())
                }
            })
    } else {
        crate::graph::crud::update_edge_meta(&graph, id, new_rank, new_atime)
    };

    drop(_elock);
    drop(_meta);
    match result {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── GET /graphs ────────────────────────────────────────────────────────────

pub async fn list_graphs(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (graphs, default) = state.gm.get_registry();
    Json(serde_json::json!({
        "graphs": graphs,
        "default": default,
    }))
}

// ── POST /graphs ────────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
pub struct CreateGraphParams {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub time_travel: bool,
}

#[derive(Serialize)]
pub struct CreateGraphResponse {
    pub name: String,
    pub description: String,
    pub time_travel: bool,
    pub created: bool,
}

pub async fn create_graph(
    State(state): State<AppState>,
    Json(params): Json<CreateGraphParams>,
) -> Result<Json<CreateGraphResponse>, StatusCode> {
    // Worker → Master forwarding
    let body_str = serde_json::to_string(&params).unwrap_or_default();
    if let Some(resp) = try_forward_json(
        &state, "POST", "/graphs", None, None, Some(&body_str),
    ).await {
        // Forward succeeded — worker also creates the graph locally
        // so it's available immediately (broadcast may lag).
        if let Ok(g) = state.gm.get(&params.name) {
            let _ = state.gm.update_meta(&params.name, &params.description, params.time_travel);
        }
        return resp.map(|json| {
            Json(CreateGraphResponse {
                name: json.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                description: json.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                time_travel: json.get("time_travel").and_then(|v| v.as_bool()).unwrap_or(false),
                created: json.get("created").and_then(|v| v.as_bool()).unwrap_or(false),
            })
        });
    }

    // Reject empty or whitespace-only names.
    if params.name.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Check existence via registry (not dir scan).
    {
        let (graphs, _) = state.gm.get_registry();
        if graphs.iter().any(|g| g.name == params.name) {
            return Ok(Json(CreateGraphResponse {
                name: params.name,
                description: params.description,
                time_travel: params.time_travel,
                created: false,
            }));
        }
    }
    // Opening the graph creates it on disk.
    match state.gm.get(&params.name) {
        Ok(_) => {
            // Persist the provided description / time_travel to the registry.
            let _ = state.gm.update_meta(&params.name, &params.description, params.time_travel);
            broadcast_request_to_workers(&state.cluster_registry, "POST", "/graphs", None, Some(&body_str));
            Ok(Json(CreateGraphResponse {
                name: params.name,
                description: params.description,
                time_travel: params.time_travel,
                created: true,
            }))
        },
        Err(_) => Ok(Json(CreateGraphResponse {
            name: params.name,
            description: params.description,
            time_travel: params.time_travel,
            created: false,
        })),
    }
}

// ── DELETE /graphs/:name ────────────────────────────────────────────────────

pub async fn delete_graph(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<serde_json::Value> {
    // Worker → Master forwarding
    if let Some(resp) = try_forward_json(&state, "DELETE", &format!("/graphs/{}", name), None, None, None).await {
        return match resp {
            Ok(json) => json,
            Err(_) => Json(serde_json::json!({"status": "error", "message": "forward failed"})),
        };
    }
    match state.gm.delete(&name) {
        Ok(_) => Json(serde_json::json!({"status": "ok"})),
        Err(_) => Json(serde_json::json!({"status": "error", "message": "not found"})),
    }
}

// ── PUT /graphs — set default graph ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct SetDefaultGraphBody {
    #[serde(default)]
    pub default: String,
}

pub async fn set_default_graph(
    State(state): State<AppState>,
    Json(body): Json<SetDefaultGraphBody>,
) -> Json<serde_json::Value> {
    match state.gm.set_default(&body.default) {
        Ok(_) => Json(serde_json::json!({"status": "ok"})),
        Err(_) => Json(serde_json::json!({"status": "error", "message": "graph not found"})),
    }
}

// ── PUT /graphs/:name — update graph metadata ────────────────────────────────

#[derive(Deserialize, Serialize)]
pub struct UpdateGraphMetaBody {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub time_travel: bool,
}

pub async fn update_graph_meta(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<UpdateGraphMetaBody>,
) -> Json<serde_json::Value> {
    // Worker → Master forwarding
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    if let Some(resp) = try_forward_json(&state, "PUT", &format!("/graphs/{}", name), None, None, Some(&body_str)).await {
        return match resp {
            Ok(json) => json,
            Err(_) => Json(serde_json::json!({"status": "error"})),
        };
    }

    // Master processes locally
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    match state.gm.update_meta(&name, &body.description, body.time_travel) {
        Ok(true) => {
            broadcast_request_to_workers(&state.cluster_registry, "PUT", &format!("/graphs/{}", name), None, Some(&body_str));
            Json(serde_json::json!({"status": "ok"}))
        }
        Ok(false) => Json(serde_json::json!({"status": "error", "message": "not found"})),
        Err(e) => Json(serde_json::json!({"status": "error", "message": e.to_string()})),
    }
}

// ── GET /graphs/:name/config ────────────────────────────────────────────────

pub async fn get_graph_config_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<crate::graph::graph::GraphConfig>, StatusCode> {
    let config = state.gm.get_graph_config(&name);
    Ok(Json(config))
}

// ── PUT /graphs/:name/config ────────────────────────────────────────────────

pub async fn put_graph_config_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<crate::graph::graph::GraphConfig>,
) -> StatusCode {
    // Worker → Master forwarding
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    if let Some(status) = try_forward_status(&state, "PUT", &format!("/graphs/{}/config", name), None, None).await {
        return status;
    }
    match state.gm.set_graph_config(&name, &body) {
        Ok(_) => {
            let body_str = serde_json::to_string(&body).unwrap_or_default();
            broadcast_request_to_workers(&state.cluster_registry, "PUT", &format!("/graphs/{}/config", name), None, Some(&body_str));
            StatusCode::OK
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── POST /batch/load — batch import vertices and edges ────────────

#[derive(Deserialize, Serialize)]
pub struct BatchImportBody {
    #[serde(default)]
    pub entities: Vec<crate::graph::batch::BatchEntity>,
    #[serde(default)]
    pub relations: Vec<crate::graph::batch::BatchRelation>,
    #[serde(default = "crate::gremlin::default_true")]
    pub update_existing: bool,
}

pub async fn handle_batch_import(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<BatchImportBody>,
) -> Result<Json<crate::graph::batch::BatchImportResult>, StatusCode> {
    // Worker → Master forwarding
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let body_str = serde_json::to_string(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    if let Some(resp) = try_forward_json(
        &state, "POST", "/batch/load", None, graph_name, Some(&body_str),
    ).await {
        let json_val = resp.map_err(|e| e)?;
        let result: crate::graph::batch::BatchImportResult = serde_json::from_value(json_val.0).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(Json(result));
    }

    let graph = crate::gremlin::resolve_graph_from_request(&state, &headers)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let result = crate::graph::batch::batch_import(
        &graph, &body.entities, &body.relations, "", body.update_existing,
    );
    // Broadcast to workers via /cluster/execute
    let graph_name = graph.name.clone();
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    broadcast_request_to_workers(&state.cluster_registry, "POST", "/batch/load", Some(&graph_name), Some(&body_str));
    Ok(Json(result))
}

// ── POST /batch/delete — batch delete vertices and edges ─────────────

#[derive(Deserialize, Serialize)]
pub struct BatchDeleteBody {
    #[serde(default)]
    pub vertices: Vec<String>,
    #[serde(default)]
    pub edges: Vec<crate::graph::batch::BatchDeleteEdge>,
}

pub async fn handle_batch_delete(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<BatchDeleteBody>,
) -> Result<Json<crate::graph::batch::BatchDeleteResult>, StatusCode> {
    let graph = crate::gremlin::resolve_graph_from_request(&state, &headers)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let result = crate::graph::batch::batch_delete(&graph, &body.vertices, &body.edges);
    Ok(Json(result))
}

// ── Document CRUD ───────────────────────────────────────────────────────────

/// List all documents.
pub async fn list_documents(
    State(state): State<AppState>,
) -> Json<Vec<crate::documents::Document>> {
    Json(state.doc_mgr.list())
}

/// Create a new document.
#[derive(Deserialize, Serialize)]
pub struct CreateDocumentBody {
    pub title: String,
    pub content: String,
    pub tags: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct CreateDocumentResponse {
    pub id: String,
    pub title: String,
    pub created: bool,
}

pub async fn create_document(
    State(state): State<AppState>,
    Json(body): Json<CreateDocumentBody>,
) -> Json<CreateDocumentResponse> {
    // Worker → Master forwarding
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    if let Some(resp) = try_forward_json(&state, "POST", "/documents", None, None, Some(&body_str)).await {
        return match resp {
            Ok(json) => {
                let master_id = json.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                // Also create locally so the Worker has immediate access
                if !master_id.is_empty() {
                    let tags = body.tags.as_deref().unwrap_or(&[]);
                    state.doc_mgr.add(&master_id, &body.title, &body.content, tags, "");
                }
                Json(CreateDocumentResponse {
                    id: master_id,
                    title: body.title.clone(),
                    created: true,
                })
            }
            Err(_) => Json(CreateDocumentResponse { id: String::new(), title: body.title, created: false }),
        };
    }

    let id = uuid::Uuid::new_v4().to_string();
    // Documents are created without a graph association.
    // The graph is assigned during extraction.
    let graph_name = "";
    let tags = body.tags.unwrap_or_default();
    state.doc_mgr.add(&id, &body.title, &body.content, &tags, graph_name);
    broadcast_request_to_workers(&state.cluster_registry, "POST", &format!("/documents/{}", id), None, Some(&serde_json::json!({"title": &body.title}).to_string()));
    Json(CreateDocumentResponse {
        id,
        title: body.title,
        created: true,
    })
}

/// Get document metadata by ID.
pub async fn get_document(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::documents::Document>, StatusCode> {
    state.doc_mgr.get(&id).map(Json).ok_or(StatusCode::NOT_FOUND)
}

/// Update document metadata.
#[derive(Deserialize, Serialize)]
pub struct UpdateDocumentBody {
    pub title: Option<String>,
    pub tags: Option<Vec<String>>,
}

pub async fn update_document(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateDocumentBody>,
) -> Json<serde_json::Value> {
    // Worker → Master forwarding
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    if let Some(resp) = try_forward_json(&state, "PUT", &format!("/documents/{}", id), None, None, Some(&body_str)).await {
        return match resp {
            Ok(json) => json,
            Err(_) => Json(serde_json::json!({"status": "error"})),
        };
    }

    let title = body.title.as_deref().unwrap_or("");
    let tags = body.tags.as_deref().unwrap_or(&[]);
    // Document graph association is set only during extraction, not via update.
    match state.doc_mgr.update(&id, title, tags, None) {
        Some(_) => {
            broadcast_request_to_workers(&state.cluster_registry, "PUT", &format!("/documents/{}", id), None, Some(&body_str));
            Json(serde_json::json!({"status": "ok"}))
        }
        None => Json(serde_json::json!({"status": "error", "message": "not found"})),
    }
}

/// Delete a document and optionally clean up associated graph data.
pub async fn delete_document(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    // Worker → Master forwarding
    if let Some(resp) = try_forward_json(&state, "DELETE", &format!("/documents/{}", id), None, None, None).await {
        return match resp {
            Ok(json) => json,
            Err(_) => Json(serde_json::json!({"status": "error"})),
        };
    }

    // Get the document before deleting, so we know which graph to clean.
    let doc = state.doc_mgr.get(&id);
    let graph_name = doc.as_ref().map(|d| d.graph_name.clone());

    let deleted = state.doc_mgr.delete(&id);

    // Clean up graph vertices/edges that carry this doc's _source_doc_id.
    if let Some(ref gname) = graph_name {
        if let Ok(graph) = state.gm.get(gname) {
            // Phase 1: Collect all vertex IDs while holding memory_index lock.
            let all_vids: Vec<u32> = {
                let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
                mi.vertex_id.keys().copied().collect()
            };

            // Phase 2: Check each vertex's _source_doc_id property (lock released).
            let mut match_vids: Vec<u32> = Vec::new();
            for vid in &all_vids {
                if let Ok(Some(vertex)) = crate::graph::locked::get_vertex_locked(&graph, *vid) {
                    if vertex.properties.get("_source_doc_id")
                        .map_or(false, |v| matches!(v, PropertyValue::String(s) if s == &id))
                    {
                        match_vids.push(*vid);
                    }
                }
            }

            // Phase 3: Collect connected edges and delete everything.
            if !match_vids.is_empty() {
                let mut edge_ids: Vec<u32> = Vec::new();
                {
                    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
                    for vid in &match_vids {
                        for &(eid, _, _) in mi.vertex_adjacency.out_edges(*vid) {
                            edge_ids.push(eid);
                        }
                        for &(eid, _, _) in mi.vertex_adjacency.in_edges(*vid) {
                            edge_ids.push(eid);
                        }
                    }
                }
                for eid in &edge_ids {
                    let _ = crate::graph::locked::hard_delete_edge_locked(&graph, *eid);
                }
                for vid in &match_vids {
                    let _ = crate::graph::locked::hard_delete_vertex_locked(&graph, *vid);
                }
                log::info!("Cleaned {} vertices and {} edges for doc '{}' in graph '{}'",
                    match_vids.len(), edge_ids.len(), id, gname);
            }
        }
    }

    if deleted {
        broadcast_request_to_workers(&state.cluster_registry, "DELETE", &format!("/documents/{}", id), None, None);
        Json(serde_json::json!({"status": "ok"}))
    } else {
        Json(serde_json::json!({"status": "error", "message": "not found"}))
    }
}

/// Get document content.
pub async fn get_document_content(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<String, StatusCode> {
    state.doc_mgr.get_content(&id).ok_or(StatusCode::NOT_FOUND)
}

// ── Extraction ──────────────────────────────────────────────────────────────

/// Submit an extraction task.
#[derive(Deserialize, Serialize)]
pub struct SubmitExtractionBody {
    pub document_id: String,
}

#[derive(Serialize)]
pub struct SubmitExtractionResponse {
    pub task_id: String,
    pub status: String,
    pub message: String,
}

pub async fn submit_extraction(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<SubmitExtractionBody>,
) -> Result<Json<SubmitExtractionResponse>, StatusCode> {
    // Worker → Master forwarding
    let body_str = serde_json::to_string(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    if let Some(resp) = try_forward_json(&state, "POST", "/extract", None, None, Some(&body_str)).await {
        return resp.map(|json| {
            Json(SubmitExtractionResponse {
                task_id: json.get("task_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                status: json.get("status").and_then(|v| v.as_str()).unwrap_or("error").to_string(),
                message: json.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            })
        });
    }

    let default_name = state.gm.get_default_name();
    let graph_name = headers
        .get("X-Graph-Name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(&default_name);
    let doc_id = body.document_id.clone();

    // Verify document exists
    let doc = state.doc_mgr.get(&doc_id).ok_or(StatusCode::NOT_FOUND)?;
    let content = state.doc_mgr.get_content(&doc_id).ok_or(StatusCode::NOT_FOUND)?;

    // Resolve the graph
    let graph = state.gm.get(graph_name).map_err(|_| StatusCode::NOT_FOUND)?;

    // Create extraction task
    let task_id = state.task_mgr.create_task("extraction", graph_name, &doc.title);
    {
        let mut tasks = state.task_mgr.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&task_id) {
            task.status = TaskStatus::Running;
            task.started_at = Some(chrono::Utc::now().to_rfc3339());
            task.steps = default_extraction_steps();
        }
    }

    // Spawn background extraction
    let task_id_clone = task_id.clone();
    let task_mgr = state.task_mgr.clone();
    let settings = state.settings.clone();
    let doc_title = doc.title.clone();
    let graph_arc = graph.clone();
    let doc_mgr = state.doc_mgr.clone();
    let gname = graph_name.to_string();
    let cluster_registry = state.cluster_registry.clone();

    tokio::spawn(async move {
        let tid = task_id_clone.clone();

        // Step 1: Reading document — done
        task_mgr.complete_step(&tid, "Reading document content");

        // Step 2: Build ExtractionConfig from settings
        let config = {
            let s = settings.lock().unwrap();
            crate::extract::config::ExtractionConfig::from_llm_config(
                &s.llm,
                s.internet.proxy.clone(),
                s.internet.ssl_verify,
            )
        };
        let sys_prompt = r#"You are a knowledge graph extractor. Extract entities and their relationships from the given markdown document.

## Entity fields
- `name` (REQUIRED): entity name in original language
- `labels` (REQUIRED, at least 1): entity type labels, e.g. ["person"], ["technology"]
- `keywords` (optional): search keywords
- `properties` (optional): key-value attributes

## Relation fields
- `source` (REQUIRED): source entity name
- `target` (REQUIRED): target entity name
- `name` (REQUIRED): relationship type label
- `labels` (optional): relation type categories, e.g. ["dependency"]
- `keywords` (optional): search keywords
- `strength` (optional, default 1.0): relationship strength 0.0-1.0
- `properties` (optional): key-value attributes

Return ONLY valid JSON with this structure:

{
  "entities": [
    {
      "name": "EntityName",
      "labels": ["type1", "type2"],
      "keywords": ["keyword1"],
      "properties": {
        "key1": "value1"
      }
    }
  ],
  "relations": [
    {
      "source": "EntityName1",
      "target": "EntityName2",
      "name": "relationship_type",
      "labels": ["category1"],
      "keywords": ["keyword1"],
      "strength": 0.8,
      "properties": {
        "key1": "value1"
      }
    }
  ],
  "tags": ["tag1", "tag2"]
}

- Extract entities and edges as many as possible.
- Entity labels could be person, place, organization, concept, event, object.
- Entity name, labels, keywords should be in the original language.
- Generate 1~5 most important tags."#;

        // Mark step 2 as running
        {
            let mut tasks = task_mgr.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&tid) {
                update_step(&mut task.steps, "Calling LLM to extract knowledge", "running", 0.0, None);
                task.overall_pct = compute_overall_pct(&task.steps);
            }
        }

        // Call LLM
        let user_msg = format!("Document: {}\n\n---\n\n{}", doc_title, content);
        let llm_result = crate::extract::llm_client::chat_completion_with_retry(
            &config, sys_prompt, &user_msg,
        ).await;

        let llm_response = match llm_result {
            Ok(r) => r,
            Err(e) => {
                task_mgr.fail_task(&tid, format!("LLM call failed: {}", e));
                return;
            }
        };

        // Parse JSON response
        let cleaned = {
            let text = llm_response.content.trim();
            if let Some(inner) = text.strip_prefix("```json")
                .or_else(|| text.strip_prefix("```"))
            {
                if let Some(end) = inner.rfind("```") {
                    inner[..end].trim().to_string()
                } else {
                    inner.trim().to_string()
                }
            } else {
                text.to_string()
            }
        };

        #[derive(Deserialize)]
        struct ExtractionOutput {
            #[serde(default)]
            entities: Vec<EntityItem>,
            #[serde(default)]
            relations: Vec<RelationItem>,
            #[serde(default)]
            tags: Vec<String>,
        }

        #[derive(Deserialize)]
        struct EntityItem {
            name: Option<String>,
            labels: Option<Vec<String>>,
            keywords: Option<Vec<String>>,
            properties: Option<HashMap<String, serde_json::Value>>,
        }

        #[derive(Deserialize)]
        struct RelationItem {
            source: Option<String>,
            target: Option<String>,
            name: Option<String>,
            labels: Option<Vec<String>>,
            keywords: Option<Vec<String>>,
            #[serde(default = "default_strength")]
            strength: f32,
            #[serde(default)]
            properties: Option<HashMap<String, serde_json::Value>>,
        }

        fn default_strength() -> f32 { 1.0 }

        let parsed: ExtractionOutput = match serde_json::from_str(&cleaned) {
            Ok(p) => p,
            Err(e) => {
                task_mgr.fail_task(&tid, format!("Failed to parse LLM response: {}. Raw: {}",
                    e, &cleaned[..cleaned.len().min(500)]));
                return;
            }
        };

        task_mgr.complete_step(&tid, "Parsing LLM response");

        // Step 3-4: Batch import entities and relations via the shared batch_import function.
        {
            let mut tasks = task_mgr.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&tid) {
                update_step(&mut task.steps, "Importing graph data", "running", 0.0,
                    Some(&format!("0/{} entities, 0/{} relations",
                        parsed.entities.len(), parsed.relations.len())));
            }
        }

        let batch_entities: Vec<crate::graph::batch::BatchEntity> = parsed.entities.iter().map(|e| {
            let mut props = e.properties.clone().unwrap_or_default();
            props.insert("_source_doc_id".to_string(), serde_json::Value::String(doc_id.clone()));
            crate::graph::batch::BatchEntity {
                name: e.name.clone().unwrap_or_else(|| "unknown".to_string()),
                labels: e.labels.clone().unwrap_or_else(|| vec!["entity".to_string()]),
                keywords: e.keywords.clone().unwrap_or_default(),
                properties: props,
            }
        }).collect();

        let batch_relations: Vec<crate::graph::batch::BatchRelation> = parsed.relations.iter().map(|r| {
            let mut props = r.properties.clone().unwrap_or_default();
            props.insert("_source_doc_id".to_string(), serde_json::Value::String(doc_id.clone()));
            crate::graph::batch::BatchRelation {
                source: r.source.clone().unwrap_or_default(),
                target: r.target.clone().unwrap_or_default(),
                name: r.name.clone().unwrap_or_else(|| "related_to".to_string()),
                labels: r.labels.clone().unwrap_or_default(),
                keywords: r.keywords.clone().unwrap_or_default(),
                strength: r.strength,
                properties: props,
            }
        }).collect();

        let total_entities = parsed.entities.len();
        let total_relations = parsed.relations.len();

        let batch_result = crate::graph::batch::batch_import(
            &graph_arc, &batch_entities, &batch_relations, &doc_id, true,
        );

        let vertex_count = batch_result.vertices_created + batch_result.vertices_updated;
        let edge_count = batch_result.edges_created + batch_result.edges_updated;

        // Broadcast the batch import to workers so they can replay the same data.
        let batch_body = serde_json::json!({
            "entities": batch_entities,
            "relations": batch_relations,
            "update_existing": true,
        }).to_string();
        broadcast_request_to_workers(
            &cluster_registry, "POST", "/batch/load",
            Some(&gname), Some(&batch_body),
        );

        task_mgr.complete_step(&tid, "Importing graph data");

        // Write extracted tags back to the document metadata and
        // associate the document with the target graph.
        doc_mgr.update(&doc_id, &doc_title, &parsed.tags, Some(&gname));

        // Mark task as completed
        {
            let mut tasks = task_mgr.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&tid) {
                for step in &mut task.steps {
                    if step.status == "running" {
                        step.status = "completed".to_string();
                        step.progress_pct = 100.0;
                    }
                }
                task.status = TaskStatus::Completed;
                task.completed_at = Some(chrono::Utc::now().to_rfc3339());
                task.overall_pct = 100.0;
                task.stats = Some(serde_json::json!({
                    "total_sections": 1,
                    "processed_sections": 1,
                    "total_entities": total_entities,
                    "total_relations": total_relations,
                    "new_vertices": vertex_count,
                    "new_edges": edge_count,
                }));
            }
        }

        log::info!("Extraction task {} completed: {} vertices, {} edges",
            tid, vertex_count, edge_count);
    });

    Ok(Json(SubmitExtractionResponse {
        task_id: task_id.clone(),
        status: "running".to_string(),
        message: format!("Extraction task submitted for document '{}'", doc.title),
    }))
}

/// Get task status.
pub async fn get_task_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Worker → Master forwarding (tasks run on master)
    if let Some(resp) = try_forward_json(&state, "GET", &format!("/tasks/{}", task_id), None, None, None).await {
        return resp;
    }
    state.task_mgr.get_task(&task_id)
        .map(|t| {
            let resp: TaskResponse = t.into();
            Json(serde_json::to_value(resp).unwrap_or_default())
        })
        .ok_or(StatusCode::NOT_FOUND)
}

/// List all tasks (newest first).
pub async fn list_tasks_handler(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    // Worker → Master forwarding (tasks run on master)
    if let Some(resp) = try_forward_json(&state, "GET", "/tasks", None, None, None).await {
        match resp {
            Ok(json) => return json,
            Err(_) => return Json(serde_json::json!([])),
        }
    }
    let tasks: Vec<TaskResponse> = state.task_mgr.list_tasks()
        .into_iter().map(|t| t.into()).collect();
    Json(serde_json::to_value(tasks).unwrap_or_default())
}

/// POST /documents/:id/extract — extract from a document by ID.
pub async fn extract_document_handler(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<SubmitExtractionResponse>, StatusCode> {
    // Get graph name from X-Graph-Name header
    let default_name = state.gm.get_default_name();
    let graph_name = headers
        .get("X-Graph-Name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(&default_name);

    // Verify document exists
    let _doc = state.doc_mgr.get(&document_id).ok_or(StatusCode::NOT_FOUND)?;
    let _content = state.doc_mgr.get_content(&document_id).ok_or(StatusCode::NOT_FOUND)?;

    // Resolve the graph
    let _graph = state.gm.get(graph_name).map_err(|_| StatusCode::NOT_FOUND)?;

    // Forward to submit_extraction logic, passing the original headers
    // so graph name is derived from X-Graph-Name consistently.
    submit_extraction(
        State(state),
        headers,
        Json(SubmitExtractionBody {
            document_id,
        }),
    ).await
}
