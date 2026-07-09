//! REST API handlers for the new block-based graph engine.
//!
//! These handlers replace the old `src/gremlin/` routes and operate on
//! `Arc<Graph>` through `GraphManager`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use std::sync::Mutex;

use crate::config::Settings;
use crate::documents::DocumentManager;
use crate::extract::task_manager::{ExtractionTaskManager, TaskResponse, TaskStatus, default_extraction_steps, update_step, compute_overall_pct};
use crate::graph::graph::Graph;
use crate::graph::gremlin::{execute_gremlin, GremlinQuery, GremlinResponse, GremlinResult};
use crate::graph_manager::GraphManager;
use crate::cluster::node::NodeRegistry;

pub mod settings;
use crate::storage::types::{PropertyValue, StorageResult};

/// Shared application state for all graph routes.
#[derive(Clone)]
pub struct AppState {
    pub gm: Arc<GraphManager>,
    pub settings: Arc<Mutex<Settings>>,
    pub doc_mgr: DocumentManager,
    pub task_mgr: ExtractionTaskManager,
    /// NodeRegistry for cluster-mode broadcasts (None in standalone).
    pub cluster_registry: Option<Arc<NodeRegistry>>,
}

/// Build the axum router for all block-engine graph routes.
pub fn build_router(
    gm: Arc<GraphManager>,
    settings: Settings,
    cluster_registry: Option<Arc<NodeRegistry>>,
) -> axum::Router {
    let doc_mgr = DocumentManager::new(&settings.storage.data_dir);
    let state = AppState {
        gm,
        settings: Arc::new(Mutex::new(settings)),
        doc_mgr,
        task_mgr: ExtractionTaskManager::new(),
        cluster_registry,
    };

    use axum::routing::{delete, get, post, put};

    axum::Router::new()
        // Graph lifecycle
        .route("/graphs", get(list_graphs))
        .route("/graphs", post(create_graph))
        .route("/graphs/:name", delete(delete_graph))
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
        .route("/settings/search", get(settings::get_search_settings))
        .route("/settings/search", put(settings::update_search_settings))
        .route("/settings/llm", get(settings::get_llm_settings))
        .route("/settings/llm", put(settings::update_llm_settings))
        // Health
        .route("/health", get(health_check))
        // MaaS — OpenAI-compatible proxy
        .route("/maas/openai/v1/models", get(crate::maas::openai::list_models_handler))
        .route("/maas/openai/v1/chat/completions", post(crate::maas::openai::chat_completions_handler))
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
        .route("/extract/task/:task_id", get(get_extraction_task))
        .route("/extract/tasks", get(list_extraction_tasks))
        // Shared state
        .with_state(state)
}

// ── Health ──────────────────────────────────────────────────────────────────

use std::time::SystemTime;

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

// ── Helper: resolve graph name from header or query ─────────────────────────

fn resolve_graph(state: &AppState, graph_name: Option<&str>) -> StorageResult<Arc<Graph>> {
    let name = graph_name.unwrap_or("graph0");
    state.gm.get(name)
}

// ── POST /gremlin2 ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct GremlinParams {
    pub graph: Option<String>,
}

pub async fn handle_gremlin(
    State(state): State<AppState>,
    Query(params): Query<GremlinParams>,
    Json(mut query): Json<GremlinQuery>,
) -> Json<GremlinResponse> {
    let graph = match resolve_graph(&state, params.graph.as_deref()) {
        Ok(g) => g,
        Err(e) => {
            return Json(GremlinResponse {
                success: false,
                data: vec![],
                error: Some(e.to_string()),
            });
        }
    };

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
        let cfg = if mode == "exact" { &settings.search.exact } else { &settings.search.greedy };

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

    let response = execute_gremlin(&graph, &query);

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
    // to persist IndexUpdate entries to the redo log and optionally broadcast.
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
            let do_touch = settings.rank.auto_inc_rank_when_read;
            drop(settings);
            if has_ids && do_touch {
                if let Ok(g) = state.gm.get("default") {
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
    pub at: Option<u64>,
    pub limit: Option<u32>,
    pub graph: Option<String>,
}

pub async fn handle_search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Json<GremlinResponse> {
    let graph = match resolve_graph(&state, params.graph.as_deref()) {
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
    let query = GremlinQuery {
        steps: vec![GremlinStep::Search {
            text: params.text,
            mode: params.mode,
            match_mode: None,
            at: params.at,
            limit: params.limit,
            min_rank: None,
        }],
    };

    let response = execute_gremlin(&graph, &query);
    Json(response)
}

// ── Shared query types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct GraphQuery {
    pub graph: Option<String>,
}

// ── POST /vertices ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
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
    Query(query): Query<GraphQuery>,
    Json(body): Json<CreateVertexBody>,
) -> Result<Json<CreateVertexResponse>, StatusCode> {
    let graph = resolve_graph(&state, query.graph.as_deref()).map_err(|_| StatusCode::NOT_FOUND)?;
    let vid = crate::graph::locked::create_vertex_locked(
        &graph,
        &body.name,
        &body.labels.unwrap_or_default(),
        &body.keywords.unwrap_or_default(),
        &body.properties,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(CreateVertexResponse { id: vid }))
}

// ── PUT /vertices2/:id ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UpdateVertexBody {
    pub name: Option<String>,
    pub labels: Option<Vec<String>>,
    pub keywords: Option<Vec<String>>,
    pub properties: Option<std::collections::HashMap<String, crate::storage::types::PropertyValue>>,
}

pub async fn update_vertex(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Query(query): Query<GraphQuery>,
    Json(body): Json<UpdateVertexBody>,
) -> StatusCode {
    let graph = match resolve_graph(&state, query.graph.as_deref()) {
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
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── DELETE /vertices/:id ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DeleteVertexParams {
    pub force: Option<bool>,
    pub graph: Option<String>,
}

pub async fn delete_vertex(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Query(params): Query<DeleteVertexParams>,
) -> StatusCode {
    let graph = match resolve_graph(&state, params.graph.as_deref()) {
        Ok(g) => g,
        Err(_) => return StatusCode::NOT_FOUND,
    };

    let force = params.force.unwrap_or(false);
    let result = if force {
        crate::graph::locked::hard_delete_vertex_locked(&graph, id)
    } else {
        crate::graph::locked::soft_delete_vertex_locked(&graph, id)
    };

    match result {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── GET /vertices/:id/meta ──────────────────────────────────────────────────

/// Read a vertex's full metadata (status, version, ctime, mtime, atime, rank).
/// Does NOT trigger any rank/atime update.
pub async fn handle_get_vertex_meta(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Json<serde_json::Value> {
    let graph = match resolve_graph(&state, None) {
        Ok(g) => g,
        Err(e) => return Json(serde_json::json!({"error": e.to_string()})),
    };
    let _vlock = graph.locks.read_vertex(id);
    let result = crate::graph::crud::get_vertex_index_record(&graph, id);
    drop(_vlock);
    match result {
        Ok(Some(rec)) => Json(serde_json::json!({
            "success": true,
            "status": rec.status as u8,
            "version": rec.version,
            "ctime": rec.ctime,
            "mtime": rec.mtime,
            "atime": rec.atime,
            "rank": rec.rank,
        })),
        Ok(None) => Json(serde_json::json!({"success": false, "error": "not found"})),
        Err(e) => Json(serde_json::json!({"success": false, "error": e.to_string()})),
    }
}

// ── PUT /vertices/:id/meta ─────────────────────────────────────────────────

/// Update a vertex's rank and/or atime. Body: `{"rank": u32, "atime": u64}`.
/// Either field is optional — only provided fields are updated.
pub async fn handle_update_vertex_meta(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    let new_rank = body.get("rank").and_then(|v| v.as_u64()).map(|v| v as u32);
    let new_atime = body.get("atime").and_then(|v| v.as_u64());
    if new_rank.is_none() && new_atime.is_none() {
        return StatusCode::BAD_REQUEST;
    }
    let graph = match resolve_graph(&state, None) {
        Ok(g) => g,
        Err(_) => return StatusCode::NOT_FOUND,
    };
    let _meta = graph.locks.read_metadata();
    let _vlock = graph.locks.write_vertex(id);
    let result = crate::graph::crud::update_vertex_meta(&graph, id, new_rank, new_atime);
    drop(_vlock);
    drop(_meta);
    match result {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── POST /edges ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
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
    Query(query): Query<GraphQuery>,
    Json(body): Json<CreateEdgeBody>,
) -> Result<Json<CreateEdgeResponse>, StatusCode> {
    let graph = resolve_graph(&state, query.graph.as_deref()).map_err(|_| StatusCode::NOT_FOUND)?;
    let eid = crate::graph::locked::create_edge_locked(
        &graph,
        body.source,
        body.target,
        &body.name,
        &body.labels.unwrap_or_default(),
        &body.keywords.unwrap_or_default(),
        body.strength.unwrap_or(1.0),
        &body.properties,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(CreateEdgeResponse { id: eid }))
}

// ── PUT /edges ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
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
    Query(query): Query<GraphQuery>,
    Json(body): Json<UpdateEdgeBody>,
) -> StatusCode {
    let graph = match resolve_graph(&state, query.graph.as_deref()) {
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
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── DELETE /edges ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DeleteEdgeParams {
    pub force: Option<bool>,
    pub graph: Option<String>,
}

pub async fn delete_edge(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Query(params): Query<DeleteEdgeParams>,
) -> StatusCode {
    let graph = match resolve_graph(&state, params.graph.as_deref()) {
        Ok(g) => g,
        Err(_) => return StatusCode::NOT_FOUND,
    };

    let force = params.force.unwrap_or(false);
    let result = if force {
        crate::graph::locked::hard_delete_edge_locked(&graph, id)
    } else {
        crate::graph::locked::soft_delete_edge_locked(&graph, id)
    };

    match result {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── GET /edges/:id/meta ────────────────────────────────────────────────────

/// Read an edge's full metadata (status, version, ctime, mtime, atime, rank).
/// Does NOT trigger any rank/atime update.
pub async fn handle_get_edge_meta(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Json<serde_json::Value> {
    let graph = match resolve_graph(&state, None) {
        Ok(g) => g,
        Err(e) => return Json(serde_json::json!({"error": e.to_string()})),
    };
    let _elock = graph.locks.read_edge(id);
    let result = crate::graph::crud::get_edge_index_record(&graph, id);
    drop(_elock);
    match result {
        Ok(Some(rec)) => Json(serde_json::json!({
            "success": true,
            "status": rec.status as u8,
            "version": rec.version,
            "ctime": rec.ctime,
            "mtime": rec.mtime,
            "atime": rec.atime,
            "rank": rec.rank,
        })),
        Ok(None) => Json(serde_json::json!({"success": false, "error": "not found"})),
        Err(e) => Json(serde_json::json!({"success": false, "error": e.to_string()})),
    }
}

// ── PUT /edges/:id/meta ───────────────────────────────────────────────────

/// Update an edge's rank and/or atime. Body: `{"rank": u32, "atime": u64}`.
pub async fn handle_update_edge_meta(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    let new_rank = body.get("rank").and_then(|v| v.as_u64()).map(|v| v as u32);
    let new_atime = body.get("atime").and_then(|v| v.as_u64());
    if new_rank.is_none() && new_atime.is_none() {
        return StatusCode::BAD_REQUEST;
    }
    let graph = match resolve_graph(&state, None) {
        Ok(g) => g,
        Err(_) => return StatusCode::NOT_FOUND,
    };
    let _meta = graph.locks.read_metadata();
    let _elock = graph.locks.write_edge(id);
    let result = crate::graph::crud::update_edge_meta(&graph, id, new_rank, new_atime);
    drop(_elock);
    drop(_meta);
    match result {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

// ── GET /graphs2 ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct GraphInfo {
    pub name: String,
}

pub async fn list_graphs(State(state): State<AppState>) -> Json<serde_json::Value> {
    let names = state.gm.list().unwrap_or_default();
    let graphs: Vec<GraphInfo> = names.into_iter().map(|n| GraphInfo { name: n }).collect();
    Json(serde_json::json!({
        "graphs": graphs,
        "time_travel": {}
    }))
}

// ── POST /graphs2 ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateGraphParams {
    pub name: String,
}

#[derive(Serialize)]
pub struct CreateGraphResponse {
    pub name: String,
    pub created: bool,
}

pub async fn create_graph(
    State(state): State<AppState>,
    Json(params): Json<CreateGraphParams>,
) -> Json<CreateGraphResponse> {
    let exists = state.gm.list().ok().map_or(false, |names| names.contains(&params.name));
    if exists {
        return Json(CreateGraphResponse {
            name: params.name,
            created: false,
        });
    }
    // Opening the graph creates it.
    match state.gm.get(&params.name) {
        Ok(_) => Json(CreateGraphResponse {
            name: params.name,
            created: true,
        }),
        Err(e) => Json(CreateGraphResponse {
            name: params.name,
            created: false,
        }),
    }
}

// ── DELETE /graphs/:name ────────────────────────────────────────────────────

pub async fn delete_graph(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> StatusCode {
    match state.gm.delete(&name) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
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
    match state.gm.set_graph_config(&name, &body) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── Document CRUD ───────────────────────────────────────────────────────────

/// List all documents.
pub async fn list_documents(
    State(state): State<AppState>,
) -> Json<Vec<crate::documents::Document>> {
    Json(state.doc_mgr.list())
}

/// Create a new document.
#[derive(Deserialize)]
pub struct CreateDocumentBody {
    pub graph: Option<String>,
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
    let id = uuid::Uuid::new_v4().to_string();
    let graph_name = body.graph.unwrap_or_else(|| "default".to_string());
    let tags = body.tags.unwrap_or_default();
    state.doc_mgr.add(&id, &body.title, &body.content, &tags, &graph_name);
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
#[derive(Deserialize)]
pub struct UpdateDocumentBody {
    pub title: Option<String>,
    pub tags: Option<Vec<String>>,
    pub graph: Option<String>,
}

pub async fn update_document(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateDocumentBody>,
) -> StatusCode {
    let title = body.title.as_deref().unwrap_or("");
    let tags = body.tags.as_deref().unwrap_or(&[]);
    match state.doc_mgr.update(&id, title, tags, body.graph.as_deref()) {
        Some(_) => StatusCode::OK,
        None => StatusCode::NOT_FOUND,
    }
}

/// Delete a document.
pub async fn delete_document(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    if state.doc_mgr.delete(&id) {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
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
#[derive(Deserialize)]
pub struct SubmitExtractionBody {
    pub document_id: String,
    pub graph: Option<String>,
}

#[derive(Serialize)]
pub struct SubmitExtractionResponse {
    pub task_id: String,
    pub status: String,
    pub message: String,
}

pub async fn submit_extraction(
    State(state): State<AppState>,
    Json(body): Json<SubmitExtractionBody>,
) -> Result<Json<SubmitExtractionResponse>, StatusCode> {
    let graph_name = body.graph.as_deref().unwrap_or("default");
    let doc_id = &body.document_id;

    // Verify document exists
    let doc = state.doc_mgr.get(doc_id).ok_or(StatusCode::NOT_FOUND)?;
    let content = state.doc_mgr.get_content(doc_id).ok_or(StatusCode::NOT_FOUND)?;

    // Resolve the graph
    let graph = state.gm.get(graph_name).map_err(|_| StatusCode::NOT_FOUND)?;

    // Create task
    let task_id = state.task_mgr.create_task(graph_name, &doc.title);
    {
        let mut tasks = state.task_mgr.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&task_id) {
            task.document_id = Some(doc_id.clone());
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

    tokio::spawn(async move {
        let tid = task_id_clone.clone();

        // Step 1: Reading document — done
        task_mgr.complete_step(&tid, "Reading document content");

        // Step 2: Build ExtractionConfig from settings
        let config = {
            let s = settings.lock().unwrap();
            crate::extract::config::ExtractionConfig::from_llm_config(&s.llm)
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

        // Step 3: Create vertices
        {
            let mut tasks = task_mgr.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&tid) {
                update_step(&mut task.steps, "Creating graph vertices", "running", 0.0,
                    Some(&format!("0/{} entities", parsed.entities.len())));
            }
        }

        let total_entities = parsed.entities.len();
        let mut name_to_vid: HashMap<String, u32> = HashMap::new();
        let mut vertex_count = 0usize;

        for entity in &parsed.entities {
            let name = entity.name.as_deref().unwrap_or("unknown");
            let type_labels = entity.labels.clone().unwrap_or_else(|| vec!["entity".to_string()]);
            let entity_kw = entity.keywords.clone().unwrap_or_default();
            let entity_props: HashMap<String, PropertyValue> = entity.properties.as_ref()
                .map(|p| p.iter().map(|(k, v)| (k.clone(), json_to_property_value(v))).collect())
                .unwrap_or_default();

            match crate::graph::locked::create_vertex_locked(
                &graph_arc, name, &type_labels, &entity_kw, &entity_props,
            ) {
                Ok(vid) => {
                    name_to_vid.insert(name.to_string(), vid);
                    vertex_count += 1;
                }
                Err(e) => {
                    log::warn!("Failed to create vertex '{}': {}", name, e);
                }
            }

            let pct = if total_entities > 0 {
                (vertex_count as f64 / total_entities as f64) * 100.0
            } else {
                100.0
            };
            task_mgr.update_task_steps(&tid, {
                let mut tasks = task_mgr.tasks.lock().unwrap();
                if let Some(task) = tasks.get_mut(&tid) {
                    update_step(&mut task.steps, "Creating graph vertices", "running", pct,
                        Some(&format!("{}/{} vertices created", vertex_count, total_entities)));
                    task.steps.clone()
                } else { vec![] }
            });
        }

        task_mgr.complete_step(&tid, "Creating graph vertices");

        // Step 4: Create edges
        {
            let mut tasks = task_mgr.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&tid) {
                update_step(&mut task.steps, "Creating graph edges", "running", 0.0,
                    Some(&format!("0/{} edges", parsed.relations.len())));
            }
        }

        let total_relations = parsed.relations.len();
        let mut edge_count = 0usize;

        for relation in &parsed.relations {
            let src_name = relation.source.as_deref().unwrap_or("");
            let tgt_name = relation.target.as_deref().unwrap_or("");
            let rel_name = relation.name.as_deref().unwrap_or("related_to");
            let rel_labels = relation.labels.clone().unwrap_or_default();
            let rel_keywords = relation.keywords.clone().unwrap_or_default();

            if let (Some(&src_vid), Some(&tgt_vid)) = (name_to_vid.get(src_name), name_to_vid.get(tgt_name)) {
                match crate::graph::locked::create_edge_locked(
                    &graph_arc, src_vid, tgt_vid, rel_name, &rel_labels, &rel_keywords, relation.strength, &HashMap::new(),
                ) {
                    Ok(_) => edge_count += 1,
                    Err(e) => log::warn!("Failed to create edge '{}->{}': {}", src_name, tgt_name, e),
                }
            }

            let pct = if total_relations > 0 {
                (edge_count as f64 / total_relations as f64) * 100.0
            } else {
                100.0
            };
            task_mgr.update_task_steps(&tid, {
                let mut tasks = task_mgr.tasks.lock().unwrap();
                if let Some(task) = tasks.get_mut(&tid) {
                    update_step(&mut task.steps, "Creating graph edges", "running", pct,
                        Some(&format!("{}/{} edges created", edge_count, total_relations)));
                    task.steps.clone()
                } else { vec![] }
            });
        }

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
                task.stats = Some(crate::extract::ExtractionStats {
                    total_sections: 1,
                    processed_sections: 1,
                    total_entities: total_entities,
                    total_relations: total_relations,
                    new_vertices: vertex_count,
                    new_edges: edge_count,
                    ..Default::default()
                });
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

/// Get extraction task status.
pub async fn get_extraction_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<TaskResponse>, StatusCode> {
    state.task_mgr.get_task(&task_id)
        .map(|t| Json(t.into()))
        .ok_or(StatusCode::NOT_FOUND)
}

/// List all extraction tasks.
pub async fn list_extraction_tasks(
    State(state): State<AppState>,
) -> Json<Vec<TaskResponse>> {
    let tasks = state.task_mgr.list_tasks();
    Json(tasks.into_iter().map(|t| t.into()).collect())
}

/// POST /documents/:id/extract — extract from a document by ID.
pub async fn extract_document_handler(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<SubmitExtractionResponse>, StatusCode> {
    // Get graph name from X-Graph-Name header
    let graph_name = headers
        .get("X-Graph-Name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default");

    // Verify document exists
    let doc = state.doc_mgr.get(&document_id).ok_or(StatusCode::NOT_FOUND)?;
    let _content = state.doc_mgr.get_content(&document_id).ok_or(StatusCode::NOT_FOUND)?;

    // Resolve the graph
    let _graph = state.gm.get(graph_name).map_err(|_| StatusCode::NOT_FOUND)?;

    // Forward to submit_extraction logic
    submit_extraction(
        State(state),
        Json(SubmitExtractionBody {
            document_id,
            graph: Some(graph_name.to_string()),
        }),
    ).await
}

/// Convert serde_json::Value to PropertyValue.
fn json_to_property_value(val: &serde_json::Value) -> PropertyValue {
    match val {
        serde_json::Value::String(s) => PropertyValue::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { PropertyValue::Integer(i) }
            else if let Some(f) = n.as_f64() { PropertyValue::Float(f) }
            else { PropertyValue::Null }
        }
        serde_json::Value::Bool(b) => PropertyValue::Boolean(*b),
        serde_json::Value::Array(arr) => PropertyValue::List(arr.iter().map(json_to_property_value).collect()),
        _ => PropertyValue::String(val.to_string()),
    }
}
