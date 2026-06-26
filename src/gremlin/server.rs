use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::cors::CorsLayer;
use crate::ui_serve::{ui_handler, ui_root_handler};
use crate::config::{Settings, save_settings};
use crate::graph_manager::GraphManager;
use crate::extract::{ExtractionTaskManager, ExtractionConfig, TaskResponse};

use crate::documents::DocumentManager;

use super::query::{GremlinQuery, QueryResponse};
use super::steps::{execute_query, execute_query_with_llm};

// ─── AppState ────────────────────────────────────────────────────

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub graph_manager: Arc<Mutex<GraphManager>>,
    pub document_manager: DocumentManager,
    pub task_manager: ExtractionTaskManager,
    /// Mutable settings that can be updated at runtime via PUT /settings.
    pub settings: Arc<Mutex<Settings>>,
}

/// Default graph name used when no X-Graph-Name header is present.
const DEFAULT_GRAPH: &str = "graph0";

fn resolve_graph_name(headers: &HeaderMap) -> String {
    headers
        .get("x-graph-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(DEFAULT_GRAPH)
        .to_string()
}

// ─── Response types ──────────────────────────────────────────────

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    graphs: usize,
    vertices: usize,
    edges: usize,
    neurons: usize,
    time_travel: std::collections::HashMap<String, bool>,
}

#[derive(Deserialize)]
struct AddVertexRequest {
    name: String,
    #[serde(default)]
    keywords: Vec<String>,
    labels: Vec<String>,
    #[serde(default)]
    properties: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Serialize)]
struct AddVertexResponse {
    id: u64,
}

#[derive(Deserialize)]
struct AddEdgeRequest {
    label: String,
    source: u64,
    target: u64,
    #[serde(default)]
    properties: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Serialize)]
struct AddEdgeResponse {
    id: u64,
}

#[derive(Deserialize)]
struct CreateNeuronRequest {
    label: String,
    keywords: Vec<String>,
    #[serde(default)]
    vertex_refs: Vec<u64>,
}

#[derive(Deserialize)]
struct CreateGraphRequest {
    name: String,
    #[serde(default)]
    time_travel: bool,
}

#[derive(Serialize)]
struct GraphListResponse {
    graphs: Vec<String>,
    default: String,
    time_travel: std::collections::HashMap<String, bool>,
}

// ─── Router ──────────────────────────────────────────────────────

/// Build the REST API router.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        // Health
        .route("/health", get(health_handler))

        // Graph management
        .route("/graphs", get(list_graphs_handler))
        .route("/graphs", post(create_graph_handler))
        .route("/graphs/:id", delete(delete_graph_handler))

        // Gremlin query
        .route("/gremlin", post(gremlin_handler))

        // Data management
        .route("/vertices", post(add_vertex_handler))
        .route("/edges", post(add_edge_handler))
        .route("/neurons", post(create_neuron_handler))
        .route("/neurons/:id/link", post(link_neuron_handler))
        .route("/neurons/:id/synapse", post(add_synapse_handler))

        // Neural search
        .route("/search", post(search_handler))

        // Compaction
        .route("/compact", post(compact_handler))

        // Vertex/Edge management
        .route("/vertices/:id", delete(delete_vertex_handler))
        .route("/vertices/:id", put(update_vertex_handler))
        .route("/edges/:id", put(update_edge_handler))
        .route("/edges/:id", delete(delete_edge_handler))

        // MaaS — OpenAI-compatible proxy
        .route("/maas/openai/v1/models", get(crate::maas::openai::list_models_handler))
        .route("/maas/openai/v1/chat/completions", post(crate::maas::openai::chat_completions_handler))

        // Settings
        .route("/settings", get(get_settings_handler))
        .route("/settings", put(update_settings_handler))
        .route("/settings/neural", get(get_neural_settings_handler))
        .route("/settings/neural", put(update_neural_settings_handler))

        // Document management
        .route("/documents", get(list_documents_handler))
        .route("/documents", post(add_document_handler))
        .route("/documents/:id", get(get_document_handler))
        .route("/documents/:id", put(update_document_handler))
        .route("/documents/:id", delete(delete_document_handler))
        .route("/documents/:id/content", get(get_document_content_handler))

        // Document extraction (background task)
        .route("/documents/:id/extract", post(start_extraction_handler))

        // Extraction task status
        .route("/extract/tasks", get(list_tasks_handler))
        .route("/extract/tasks/:task_id", get(get_task_handler))

        // Re-index edges into neural network
        .route("/reindex", post(reindex_handler))

        // UI — redirect / → /ui/
        .route("/", get(|| async { axum::response::Redirect::to("/ui/") }))
        // Serve embedded frontend from binary (built via rust-embed)
        .route("/ui", get(ui_root_handler))
        .route("/ui/", get(ui_root_handler))
        .route("/ui/*path", get(ui_handler))

        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ─── Graph Management ────────────────────────────────────────────

/// GET /graphs — List all graphs with time_travel status.
async fn list_graphs_handler(
    State(state): State<AppState>,
) -> Json<GraphListResponse> {
    let gm = state.graph_manager.lock().unwrap();
    let mut time_travel = std::collections::HashMap::new();
    for name in gm.list() {
        if let Some(h) = gm.get(&name) {
            if let Ok(g) = h.disk_graph.lock() {
                time_travel.insert(name, g.time_travel_enabled);
            }
        }
    }
    Json(GraphListResponse {
        graphs: gm.list(),
        default: DEFAULT_GRAPH.to_string(),
        time_travel,
    })
}

/// POST /graphs — Create a new graph.
async fn create_graph_handler(
    State(state): State<AppState>,
    Json(req): Json<CreateGraphRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut gm = state.graph_manager.lock().unwrap();
    gm.create_with_opts(&req.name, req.time_travel).map_err(|e| {
        let err = serde_json::json!({"success": false, "error": e});
        (StatusCode::CONFLICT, Json(err))
    })?;
    Ok(Json(serde_json::json!({
        "success": true, "name": req.name, "time_travel": req.time_travel
    })))
}

/// DELETE /graphs/{name} — Delete a graph (cannot delete "default").
async fn delete_graph_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut gm = state.graph_manager.lock().unwrap();
    gm.delete(&name).map_err(|e| {
        let err = serde_json::json!({"success": false, "error": e});
        (StatusCode::BAD_REQUEST, Json(err))
    })?;
    Ok(Json(serde_json::json!({"success": true, "deleted": name})))
}

// ─── Data handlers ───────────────────────────────────────────────

/// GET /health — Aggregate stats across all graphs.
async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let gm = state.graph_manager.lock().unwrap();
    let mut total_v = 0;
    let mut total_e = 0;
    let mut total_n = 0;
    for name in gm.list() {
        if let Some(h) = gm.get(&name) {
            if let Ok(g) = h.disk_graph.lock() {
                total_v += g.vertex_count();
                total_e += g.edge_count();
            }
            if let Ok(nn) = h.neural_network.lock() {
                total_n += nn.neuron_count();
            }
        }
    }
    let mut time_travel = std::collections::HashMap::new();
    for name in gm.list() {
        if let Some(h) = gm.get(&name) {
            if let Ok(g) = h.disk_graph.lock() {
                time_travel.insert(name, g.time_travel_enabled);
            }
        }
    }
    Json(HealthResponse {
        status: "ok".to_string(),
        graphs: gm.len(),
        vertices: total_v,
        edges: total_e,
        neurons: total_n,
        time_travel,
    })
}

/// POST /gremlin — Execute a Gremlin pipeline query.
async fn gremlin_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(query): Json<GremlinQuery>,
) -> Json<QueryResponse> {
    let graph_name = resolve_graph_name(&headers);
    // Extract handles and drop lock before calling execute_query_with_llm,
    // since it may use block_on internally for LLM calls.
    let (g, n) = {
        let gm = state.graph_manager.lock().unwrap();
        match gm.get(&graph_name) {
            Some(handle) => (handle.disk_graph.clone(), handle.neural_network.clone()),
            None => return Json(QueryResponse {
                success: false, data: vec![], error: Some(format!("Graph '{}' not found", graph_name)),
                ticks_used: None, neurons_fired: None,
            }),
        }
    };
    // Run query on a blocking thread to avoid block_on within tokio runtime
    let result = tokio::task::spawn_blocking(move || {
        execute_query_with_llm(&g, &n, &query, None)
    }).await.unwrap_or_else(|_| QueryResponse {
        success: false, data: vec![], error: Some("Query execution panicked".to_string()),
        ticks_used: None, neurons_fired: None,
    });
    Json(result)
}

/// POST /vertices — Add a vertex.
async fn add_vertex_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AddVertexRequest>,
) -> Result<Json<AddVertexResponse>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let gm = state.graph_manager.lock().unwrap();
    let props: std::collections::HashMap<String, crate::graph::PropertyValue> = req
        .properties.into_iter().map(|(k, v)| (k, json_to_property(&v))).collect();
    match gm.add_vertex_to_graph(&graph_name, &req.name, &req.keywords, &req.labels, &props) {
        Ok(id) => Ok(Json(AddVertexResponse { id })),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e})))),
    }
}

/// POST /edges — Add an edge.
async fn add_edge_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AddEdgeRequest>,
) -> Result<Json<AddEdgeResponse>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let gm = state.graph_manager.lock().unwrap();
    let props: std::collections::HashMap<String, crate::graph::PropertyValue> = req
        .properties.into_iter().map(|(k, v)| (k, json_to_property(&v))).collect();
    match gm.add_edge_to_graph(&graph_name, &req.label, req.source, req.target, &props) {
        Ok(id) => Ok(Json(AddEdgeResponse { id })),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e})))),
    }
}

/// POST /neurons — Create a neuron.
async fn create_neuron_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateNeuronRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;
    let mut nn = handle.neural_network.lock().unwrap();
    let id = (nn.neuron_count() as u64) + 1;
    let mut neuron = crate::neuron::Neuron::new(id, &req.label);
    neuron.keywords = req.keywords;
    neuron.vertex_refs = req.vertex_refs;
    nn.add_neuron(neuron.clone());
    if let Ok(mut wal) = handle.redolog_wal.lock() {
        let _ = wal.append_add_neuron(&neuron);
    }
    Ok(Json(serde_json::json!({"id": id})))
}

/// POST /neurons/{id}/link — Link neuron to vertex.
async fn link_neuron_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(neuron_id): Path<u64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let vertex_id = body.get("vertex_id").and_then(|v| v.as_u64())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing vertex_id"}))))?;
    let graph_name = resolve_graph_name(&headers);
    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;
    {
        let mut nn = handle.neural_network.lock().unwrap();
        nn.link_vertex(neuron_id, vertex_id);
        if let Ok(mut wal) = handle.redolog_wal.lock() {
            let _ = wal.append_link_vertex(neuron_id, vertex_id);
        }
    }
    Ok(Json(serde_json::json!({"status": "linked"})))
}

/// POST /neurons/{id}/synapse — Add synapse.
async fn add_synapse_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pre_id): Path<u64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let post_id = body.get("post_neuron_id").and_then(|v| v.as_u64())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing post_neuron_id"}))))?;
    let strength = body.get("strength").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
    let plasticity = body.get("plasticity").and_then(|v| v.as_f64()).unwrap_or(0.05) as f32;

    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;
    let mut nn = handle.neural_network.lock().unwrap();
    nn.add_synapse(pre_id, post_id, strength, plasticity)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "neuron not found"}))))?;
    if let Ok(mut wal) = handle.redolog_wal.lock() {
        let _ = wal.append_add_synapse(pre_id, post_id, strength, plasticity);
    }
    Ok(Json(serde_json::json!({"status": "synapse_created"})))
}

/// POST /search — Quick neural search.
async fn search_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<QueryResponse> {
    let graph_name = resolve_graph_name(&headers);
    let query_text = body.get("query").and_then(|v| v.as_str()).unwrap_or("");

    let (g, n) = {
        let gm = state.graph_manager.lock().unwrap();
        match gm.get(&graph_name) {
            Some(handle) => (handle.disk_graph.clone(), handle.neural_network.clone()),
            None => return Json(QueryResponse {
                success: false, data: vec![], error: Some(format!("Graph '{}' not found", graph_name)),
                ticks_used: None, neurons_fired: None,
            }),
        }
    };
    let gremlin_query = GremlinQuery::new(vec![
        super::query::TraversalStep::Search {
            at: None,
            mode: body.get("mode").and_then(|v| v.as_str().map(|s| s.to_string())),
            keywords: query_text.split_whitespace().map(|s| s.to_string()).collect(),
        },
    ]);
    let result = tokio::task::spawn_blocking(move || {
        execute_query(&g, &n, &gremlin_query)
    }).await.unwrap_or_else(|_| QueryResponse {
        success: false, data: vec![], error: Some("Query execution panicked".to_string()),
        ticks_used: None, neurons_fired: None,
    });
    Json(result)
}

// ─── Compaction ─────────────────────────────────────────────────

/// POST /compact — Trigger history compaction on a graph.
async fn compact_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let before = body.get("before")
        .and_then(|v| {
            v.as_i64().or_else(|| {
                v.as_str().and_then(|s| crate::gremlin::steps::parse_time_value(
                    &serde_json::Value::String(s.to_string())
                ).ok())
            })
        })
        .unwrap_or_else(|| {
            // Default: compact everything older than 30 days
            let now = crate::graph::vertex::now_micros();
            now - 30 * 24 * 3600 * 1_000_000
        });

    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;

    let mut g = handle.disk_graph.lock().unwrap().snapshot();
    let stats = crate::storage::compaction::compact_graph(
        &mut g,
        handle.data_dir(),
        before,
        0,
    );

    Ok(Json(serde_json::json!({
        "success": true,
        "graph": graph_name,
        "compacted": {
            "vertices_scanned": stats.vertices_scanned,
            "vertices_compacted": stats.vertices_compacted,
            "records_archived": stats.records_archived,
            "records_truncated": stats.records_truncated,
            "elapsed_us": stats.elapsed_us,
        }
    })))
}

// ─── Vertex Delete ───────────────────────────────────────────────

/// DELETE /vertices/{id} — Delete a vertex and its connected edges.
/// Supports optional `?force=true` query param to override default behavior.
async fn delete_vertex_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;
    // ── Step 1: Collect incident edges & snapshots ──────────
    let mut g = handle.disk_graph.lock().unwrap();
    let force = params.get("force").map(|v| v == "true").unwrap_or(!g.time_travel_enabled);
    let now = crate::graph::vertex::now_micros();
    let edge_ids: Vec<u64> = g.all_edges().into_iter().filter(|e| e.source == id || e.target == id).map(|e| e.id).collect();
    // Save edge clones for rollback (only needed for hard-delete)
    let saved_edges: Vec<crate::graph::Edge> = if force {
        edge_ids.iter().into_iter().filter_map(|eid| g.get_edge(eid)).collect()
    } else { vec![] };
    drop(g);
    // ── Step 2: Collect neuron snapshots + do in-memory ─────
    struct NeuronSave {
        id: crate::neuron::NeuronId,
        old: crate::neuron::Neuron,
    }
    let mut neuron_saves: Vec<NeuronSave> = Vec::new();
    if let Ok(mut nn) = handle.neural_network.lock() {
        use crate::neuron::neuron::EntityType;
        // Vertex neuron
        let vnid = {
            let mut result = None;
            for n in nn.all_neurons() {
                if matches!(n.entity_type, Some(EntityType::Vertex(v)) if v == id) {
                    result = Some(n.id);
                    break;
                }
            }
            result
        };
        if let Some(nid) = vnid {
            if let Some(n) = nn.get_neuron_mut(nid) {
                let old = n.clone();
                n.mark_deleted(now);
                neuron_saves.push(NeuronSave { id: nid, old });
            }
        }
        // Edge neurons
        for &eid in &edge_ids {
            let enid = {
                let mut result = None;
                for n in nn.all_neurons() {
                    if matches!(n.entity_type, Some(EntityType::Edge(e)) if e == eid) {
                        result = Some(n.id);
                        break;
                    }
                }
                result
            };
            if let Some(nid) = enid {
                if let Some(n) = nn.get_neuron_mut(nid) {
                    let old = n.clone();
                    n.mark_deleted(now);
                    neuron_saves.push(NeuronSave { id: nid, old });
                }
            }
        }
        nn.mark_dirty();
    }
    // ── Step 3: In-memory graph mutations ───────────────────
    let mut g = handle.disk_graph.lock().unwrap();
    for &eid in &edge_ids {
        if force { let _ = g.remove_edge(eid); }
        else { let _ = g.soft_delete_edge(eid, true); }
    }
    let _ = g.remove_vertex(id);
    drop(g);
    // ── Step 4: Build atomic batch WAL ──────────────────────
    let mut entries: Vec<(u8, Vec<u8>)> = Vec::new();
    if force {
        for &eid in &edge_ids {
            let p = bincode::serialize(&crate::storage::redolog_wal::RemoveEdgePayload { id: eid })
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;
            entries.push((crate::storage::redolog_wal::OP_REMOVE_EDGE, p));
        }
    }
    let vertex_payload = bincode::serialize(&crate::storage::redolog_wal::RemoveVertexPayload { id })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;
    entries.push((crate::storage::redolog_wal::OP_REMOVE_VERTEX, vertex_payload));
    for ns in &neuron_saves {
        let np = bincode::serialize(&ns.old)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;
        entries.push((crate::storage::redolog_wal::OP_UPDATE_NEURON, np));
    }
    // ── Step 5: Atomic write ────────────────────────────────
    if let Ok(mut wal) = handle.redolog_wal.lock() {
        if let Err(e) = wal.write_batch(&entries) {
            // Rollback: restore graph (best-effort)
            let mut g = handle.disk_graph.lock().unwrap();
            // Remove the vertex if it was added back... actually remove_vertex hard-deletes
            // For simplicity, just log the error and return
            // Rollback: if hard-delete, try to re-add edges
            if force {
                for edge in &saved_edges {
                    let _ = g.add_edge_with_props(edge.label.clone(), edge.source, edge.target, edge.properties.clone());
                }
            }
            drop(g);
            // Rollback neurons
            if let Ok(mut nn) = handle.neural_network.lock() {
                for ns in &neuron_saves {
                    if nn.get_neuron(ns.id).is_some() {
                        // Remove and re-add to fully restore
                        nn.remove_neuron(ns.id);
                    }
                    nn.add_neuron(ns.old.clone());
                }
                nn.mark_dirty();
            }
            return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("WAL write failed: {}", e)}))));
        }
    }
    Ok(Json(serde_json::json!({"success": true, "deleted": id})))
}

// ─── Settings ────────────────────────────────────────────────────

/// GET /settings — Return full LLM settings (api_key is stripped from each provider).
async fn get_settings_handler(
    State(state): State<AppState>,
) -> Json<Value> {
    let s = state.settings.lock().unwrap();
    // Build providers without api_key for security
    let providers_json: Vec<Value> = s.llm.providers.iter().map(|p| {
        serde_json::json!({
            "name": p.name,
            "api_base_url": p.api_base_url,
            "models": p.models,
        })
    }).collect();
    Json(serde_json::json!({
        "llm": {
            "providers": providers_json,
            "default_model": s.llm.default_model,
            "context_window": s.llm.context_window,
            "max_output_tokens": s.llm.max_output_tokens,
            "max_retries": s.llm.max_retries,
        }
    }))
}

#[derive(Deserialize)]
struct UpdateLlmRequest {
    providers: Vec<crate::config::LlmProvider>,
    #[serde(default)]
    default_model: Option<String>,
    context_window: Option<usize>,
    max_output_tokens: Option<usize>,
    max_retries: Option<u32>,
}

/// PUT /settings — Update full LLM settings and persist to file.
/// If a provider's api_key is empty, the existing value is preserved.
async fn update_settings_handler(
    State(state): State<AppState>,
    Json(req): Json<UpdateLlmRequest>,
) -> Json<Value> {
    let mut s = state.settings.lock().unwrap();

    // Snapshot existing api_keys before overwriting, so we can preserve
    // them when the incoming request sends an empty value.
    let old_keys: std::collections::HashMap<String, String> = s.llm.providers
        .iter()
        .map(|p| (p.name.clone(), p.api_key.clone()))
        .collect();

    s.llm.providers = req.providers;

    // Restore api_key for providers that sent an empty value
    for prov in &mut s.llm.providers {
        if prov.api_key.is_empty() {
            if let Some(old_key) = old_keys.get(&prov.name) {
                if !old_key.is_empty() {
                    prov.api_key = old_key.clone();
                }
            }
        }
    }

    if let Some(v) = req.default_model { s.llm.default_model = v; }
    if let Some(v) = req.context_window { s.llm.context_window = v; }
    if let Some(v) = req.max_output_tokens { s.llm.max_output_tokens = v; }
    if let Some(v) = req.max_retries { s.llm.max_retries = v; }

    let _ = save_settings(&s);

    Json(serde_json::json!({"success": true}))
}

/// GET /settings/neural — Return current neural configuration (nested groups).
async fn get_neural_settings_handler(
    State(state): State<AppState>,
) -> Json<Value> {
    let s = state.settings.lock().unwrap();
    Json(serde_json::json!({
        "neural": {
            "activate": {
                "default_threshold": s.neural.activate.default_threshold,
                "default_decay_rate": s.neural.activate.default_decay_rate,
                "default_refractory_ticks": s.neural.activate.default_refractory_ticks,
                "max_ticks": s.neural.activate.max_ticks,
                "hot_threshold": s.neural.activate.hot_threshold,
                "min_synapse_strength": s.neural.activate.min_synapse_strength,
                "auto_stabilize": s.neural.activate.auto_stabilize,
            },
            "search": {
                "default_search_mode": s.neural.search.default_search_mode,
                "greedy_exact_score": s.neural.search.greedy_exact_score,
                "greedy_partial_score": s.neural.search.greedy_partial_score,
                "exact_min_score": s.neural.search.exact_min_score,
                "fuzzy_match_enabled": s.neural.search.fuzzy_match_enabled,
                "fuzzy_match_threshold": s.neural.search.fuzzy_match_threshold,
            },
            "learn": {
                "enabled": s.neural.learn.enabled,
                "co_fire_window": s.neural.learn.co_fire_window,
                "min_plasticity": s.neural.learn.min_plasticity,
                "synaptic_decay": s.neural.learn.synaptic_decay,
            },
        }
    }))
}

/// PUT /settings/neural — Update neural configuration and persist to file.
/// Accepts flat or nested keys for backward compatibility.
async fn update_neural_settings_handler(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Json<Value> {
    let mut s = state.settings.lock().unwrap();

    // Helper to read a value from nested or flat path
    let val = |flat: &str, nested: &str| -> Option<&serde_json::Value> {
        body.get(nested).or_else(|| body.get(flat))
    };

    // ── activate group ──
    if let Some(v) = val("default_threshold", "default_threshold").and_then(|v| v.as_f64()) {
        s.neural.activate.default_threshold = v as f32;
    }
    if let Some(v) = val("default_decay_rate", "default_decay_rate").and_then(|v| v.as_f64()) {
        s.neural.activate.default_decay_rate = v as f32;
    }
    if let Some(v) = val("default_refractory_ticks", "default_refractory_ticks").and_then(|v| v.as_u64()) {
        s.neural.activate.default_refractory_ticks = v as usize;
    }
    if let Some(v) = val("max_ticks", "max_ticks").and_then(|v| v.as_u64()) {
        s.neural.activate.max_ticks = v as usize;
    }
    if let Some(v) = val("hot_threshold", "hot_threshold").and_then(|v| v.as_f64()) {
        s.neural.activate.hot_threshold = v as f32;
    }
    if let Some(v) = val("min_synapse_strength", "min_synapse_strength").and_then(|v| v.as_f64()) {
        s.neural.activate.min_synapse_strength = v as f32;
    }
    if let Some(v) = val("auto_stabilize", "auto_stabilize").and_then(|v| v.as_bool()) {
        s.neural.activate.auto_stabilize = v;
    }

    // ── search group ──
    if let Some(v) = val("default_search_mode", "default_search_mode").and_then(|v| v.as_str()) {
        s.neural.search.default_search_mode = v.to_string();
    }
    if let Some(v) = val("greedy_exact_score", "greedy_exact_score").and_then(|v| v.as_f64()) {
        s.neural.search.greedy_exact_score = v as f32;
    }
    if let Some(v) = val("greedy_partial_score", "greedy_partial_score").and_then(|v| v.as_f64()) {
        s.neural.search.greedy_partial_score = v as f32;
    }
    if let Some(v) = val("exact_min_score", "exact_min_score").and_then(|v| v.as_f64()) {
        s.neural.search.exact_min_score = v as f32;
    }
    if let Some(v) = val("fuzzy_match_enabled", "fuzzy_match_enabled").and_then(|v| v.as_bool()) {
        s.neural.search.fuzzy_match_enabled = v;
    }
    if let Some(v) = val("fuzzy_match_threshold", "fuzzy_match_threshold").and_then(|v| v.as_f64()) {
        s.neural.search.fuzzy_match_threshold = v as f32;
    }

    // ── learn group ──
    if let Some(v) = val("learning_enabled", "enabled").and_then(|v| v.as_bool()) {
        s.neural.learn.enabled = v;
    }
    if let Some(v) = val("co_fire_window", "co_fire_window").and_then(|v| v.as_u64()) {
        s.neural.learn.co_fire_window = v as usize;
    }
    if let Some(v) = val("min_plasticity", "min_plasticity").and_then(|v| v.as_f64()) {
        s.neural.learn.min_plasticity = v as f32;
    }
    if let Some(v) = val("synaptic_decay", "synaptic_decay").and_then(|v| v.as_f64()) {
        s.neural.learn.synaptic_decay = v as f32;
    }

    let _ = save_settings(&s);
    Json(serde_json::json!({"success": true}))
}

// ─── Document Management ─────────────────────────────────────────

#[derive(Serialize)]
struct DocumentListResponse {
    documents: Vec<crate::documents::Document>,
}

#[derive(Deserialize)]
struct AddDocumentRequest {
    title: String,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    graph_name: String,
}

/// GET /documents — List all documents.
async fn list_documents_handler(
    State(state): State<AppState>,
) -> Json<DocumentListResponse> {
    Json(DocumentListResponse {
        documents: state.document_manager.list(),
    })
}

/// GET /documents/{id} — Get document metadata.
async fn get_document_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.document_manager.get(&id) {
        Some(doc) => Ok(Json(serde_json::json!(doc))),
        None => Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document not found"})))),
    }
}

/// GET /documents/{id}/content — Get document content.
async fn get_document_content_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<String, (StatusCode, Json<Value>)> {
    match state.document_manager.get_content(&id) {
        Some(content) => Ok(content),
        None => Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document not found"})))),
    }
}

/// POST /documents — Add a new document.
async fn add_document_handler(
    State(state): State<AppState>,
    Json(req): Json<AddDocumentRequest>,
) -> Json<Value> {
    let id = uuid::Uuid::new_v4().to_string();
    let doc = state.document_manager.add(&id, &req.title, &req.content, &req.tags, &req.graph_name);
    Json(serde_json::json!(doc))
}

#[derive(Deserialize)]
struct UpdateDocumentRequest {
    title: String,
    tags: Vec<String>,
    #[serde(default)]
    graph_name: Option<String>,
}

/// PUT /documents/{id} — Update document.
async fn update_document_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateDocumentRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.document_manager.update(&id, &req.title, &req.tags, req.graph_name.as_deref()) {
        Some(doc) => Ok(Json(serde_json::json!(doc))),
        None => Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document not found"})))),
    }
}

/// DELETE /documents/{id} — Delete a document and its associated graph data.
async fn delete_document_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let clean_graph = params.get("clean").map(|s| s == "true").unwrap_or(false);
    // Get doc metadata first
    let doc = state.document_manager.get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document not found"}))))?;

    let _doc_title = doc.title.clone();

    // Clean up graph vertices associated with this document (all graphs)
    let deleted_vertices: usize = if clean_graph {
        let graph_name = if doc.graph_name.is_empty() { "default" } else { &doc.graph_name };
        let gm = state.graph_manager.lock().unwrap();
        if let Some(handle) = gm.get(graph_name) {
            let mut g = handle.disk_graph.lock().unwrap();
            let to_delete: Vec<u64> = g.vertex_ids()
                .into_iter().filter_map(|vid| {
                    let v = g.get_vertex(*vid)?;
                    if v.document == id { Some(*vid) } else { None }
                })
                .collect();
            let total = to_delete.len();
            for vid in to_delete {
                let edge_ids: Vec<u64> = g.all_edges()
                    .into_iter().filter(|e| e.source == vid || e.target == vid)
                    .map(|e| e.id).collect();
                let edge_force = !g.time_travel_enabled;
                let edge_now = crate::graph::vertex::now_micros();
                for eid in &edge_ids {
                    if edge_force {
                        let _ = g.remove_edge(eid);
                    } else {
                        let _ = g.soft_delete_edge(eid, true);
                    }
                    if let Ok(mut wal) = handle.redolog_wal.lock() { let _ = wal.append_remove_edge(eid); }
                    // Mark the edge's neuron as deleted
                    if let Ok(mut nn) = handle.neural_network.lock() {
                        use crate::neuron::neuron::EntityType;
                        let nid = {
                            let mut result = None;
                            for n in nn.all_neurons() {
                                if matches!(n.entity_type, Some(EntityType::Edge(e)) if e == eid) {
                                    result = Some(n.id);
                                    break;
                                }
                            }
                            result
                        };
                        if let Some(nid) = nid {
                            if let Some(neuron) = nn.get_neuron_mut(nid) {
                                neuron.mark_deleted(edge_now);
                                nn.mark_dirty();
                            }
                        }
                    }
                }
                let force = !g.time_travel_enabled;
                let _ = g.remove_vertex(vid);
                if let Ok(mut wal) = handle.redolog_wal.lock() { let _ = wal.append_remove_vertex(vid); }
                if let Ok(mut nn) = handle.neural_network.lock() {
                    use crate::neuron::neuron::EntityType;
                    let now = crate::graph::vertex::now_micros();
                    let nid = nn.all_neurons().find_map(|n| {
                        if matches!(n.entity_type, Some(EntityType::Vertex(v)) if v == vid) { Some(n.id) } else { None }
                    });
                    if let Some(nid) = nid {
                        let neuron_data;
                        {
                            let neuron = nn.get_neuron_mut(nid).expect("neuron should exist");
                            neuron.mark_deleted(now);
                            neuron_data = neuron.clone();
                        }
                        nn.mark_dirty();
                        if let Ok(mut wal) = handle.redolog_wal.lock() { let _ = wal.append_update_neuron(&neuron_data); }
                    }
                }
            }
            total
        } else {
            0
        }
    } else {
        0
    };

    // Delete the document
    if state.document_manager.delete(&id) {
        Ok(Json(serde_json::json!({
            "success": true,
            "deleted_vertices": deleted_vertices,
        })))
    } else {
        Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document not found"}))))
    }
}

// ─── Document Extraction (Background Task) ────────────────────────

/// POST /documents/{id}/extract — Start extraction for a document.
/// Optional query param `?model=Provider/ModelName` overrides the LLM model used.
async fn start_extraction_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let model_override = params.get("model").and_then(|m| {
        if m.is_empty() { None } else { Some(m.clone()) }
    });

    // Check document exists
    let doc = state.document_manager.get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document not found"}))))?;

    // Get document content
    let content = state.document_manager.get_content(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document content not found"}))))?;

    let title = doc.title.clone();

    // Build ExtractionConfig from current settings at runtime.
    // If a model override ("Provider/Model") is specified, look up the provider
    // and override the config with its api_key, api_base_url, and model name.
    let extract_config = {
        let s = state.settings.lock().unwrap();
        let mut config = ExtractionConfig::from_llm_config(&s.llm);
        if let Some(ref model_key) = model_override {
            if let Some((prov_name, model_name)) = model_key.split_once('/') {
                if let Some(provider) = s.llm.providers.iter().find(|p| p.name == prov_name) {
                    config.api_key = provider.api_key.clone();
                    config.api_base_url = provider.api_base_url.clone();
                    config.model = model_name.to_string();
                }
            }
        }
        config
    };

    let task_id = state.task_manager.submit_document_extraction(
        extract_config,
        id,
        content,
        title,
        graph_name.clone(),
        state.graph_manager.clone(),
        Arc::new(state.document_manager.clone()),
    );

    Ok(Json(serde_json::json!({
        "task_id": task_id,
        "status": "running",
    })))
}

/// GET /extract/tasks — List all extraction tasks.
async fn list_tasks_handler(
    State(state): State<AppState>,
) -> Json<Value> {
    let tasks: Vec<TaskResponse> = state.task_manager.list_tasks()
        .into_iter()
        .map(|t| t.into())
        .collect();
    Json(serde_json::json!({ "tasks": tasks }))
}

/// GET /extract/tasks/{task_id} — Get extraction task status.
async fn get_task_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.task_manager.get_task(&task_id) {
        Some(task) => {
            let resp: TaskResponse = task.into();
            Ok(Json(serde_json::json!(resp)))
        }
        None => Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Task not found"})))),
    }
}

// ─── Update Vertex ──────────────────────────────────────────────

/// PUT /vertices/:id — Update vertex labels and properties.
#[derive(Deserialize)]
struct UpdateVertexRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    keywords: Option<Vec<String>>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    properties: std::collections::HashMap<String, serde_json::Value>,
}

async fn update_vertex_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<UpdateVertexRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;
    let mut g = handle.disk_graph.lock().unwrap();
    let record_history = g.time_travel_enabled;
    drop(g);
    // ── Step 1: In-memory vertex update & save old state ──────
    let mut g = handle.disk_graph.lock().unwrap();
    let vertex = g.get_vertex_mut(id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "vertex not found"})))
    })?;
    let old_name = vertex.name.clone();
    let old_keywords = vertex.keywords.clone();
    let old_labels = vertex.labels.clone();
    let old_properties = vertex.properties.clone();
    let old_history = vertex._history.clone();
    let old_version = vertex._version;
    let old_updated_at = vertex._updated_at;

    let props: std::collections::HashMap<String, crate::graph::PropertyValue> = req
        .properties.into_iter().map(|(k, v)| (k, json_to_property(&v))).collect();
    if record_history {
        let now = crate::graph::now_micros();
        vertex._history.push(crate::graph::VersionRecord {
            version: vertex._version,
            updated_at: vertex._updated_at,
            name: vertex.name.clone(),
            keywords: vertex.keywords.clone(),
            document: vertex.document.clone(),
            labels: vertex.labels.clone(),
            properties: vertex.properties.clone(),
        });
        vertex._version += 1;
        vertex._updated_at = now;
    }
    if let Some(name) = req.name { vertex.name = name; }
    if let Some(keywords) = req.keywords { vertex.keywords = keywords; }
    if !req.labels.is_empty() { vertex.labels = req.labels; }
    vertex.properties = props;
    let new_keywords: Vec<String> = {
        let v = &vertex;
        let mut kw = v.labels.clone();
        kw.push(v.name.clone());
        kw.extend(v.keywords.iter().cloned());
        kw
    };
    drop(g);
    // ── Step 2: In-memory neuron update ───────────────────────
    let old_neuron_kw: Option<Vec<String>>;
    let neuron_for_wal: Option<crate::neuron::Neuron>;
    let neuron_needs_update: bool;
    if let Ok(mut nn) = handle.neural_network.lock() {
        use crate::neuron::neuron::EntityType;
        let nid = {
            let mut result = None;
            for n in nn.all_neurons() {
                if matches!(n.entity_type, Some(EntityType::Vertex(v)) if v == id) {
                    result = Some(n.id);
                    break;
                }
            }
            result
        };
        if let Some(nid) = nid {
            let n = nn.get_neuron_mut(nid).unwrap();
            old_neuron_kw = Some(n.keywords.clone());
            n.keywords = new_keywords;
            neuron_for_wal = Some(n.clone());
            nn.mark_dirty();
            neuron_needs_update = true;
        } else {
            old_neuron_kw = None;
            neuron_for_wal = None;
            neuron_needs_update = false;
        }
    } else {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "lock error"}))));
    }
    // ── Step 3: Atomic WAL batch ──────────────────────────────
    let entries = {
        let mut g = handle.disk_graph.lock().unwrap();
        let v = g.get_vertex(id).unwrap();
        let vertex_payload = bincode::serialize(
            &crate::storage::redolog_wal::UpdateVertexPayload {
                id, labels: v.labels.clone(), properties: v.properties.clone(),
            }
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;
        drop(g);
        let mut batch = vec![(crate::storage::redolog_wal::OP_UPDATE_VERTEX, vertex_payload)];
        if let Some(ref n) = neuron_for_wal {
            let np = bincode::serialize(n)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;
            batch.push((crate::storage::redolog_wal::OP_UPDATE_NEURON, np));
        }
        batch
    };
    // write_batch — on failure, rollback
    if entries.len() > 1 {
        if let Ok(mut wal) = handle.redolog_wal.lock() {
            if let Err(e) = wal.write_batch(&entries) {
                // Rollback: restore vertex + neuron
                let mut g = handle.disk_graph.lock().unwrap();
                if let Some(v) = g.get_vertex_mut(id) {
                    v.name = old_name;
                    v.keywords = old_keywords;
                    v.labels = old_labels;
                    v.properties = old_properties;
                    v._history = old_history;
                    v._version = old_version;
                    v._updated_at = old_updated_at;
                }
                drop(g);
                if neuron_needs_update {
                    if let Ok(mut nn) = handle.neural_network.lock() {
                        use crate::neuron::neuron::EntityType;
                        let nid = {
                            let mut result = None;
                            for n in nn.all_neurons() {
                                if matches!(n.entity_type, Some(EntityType::Vertex(v)) if v == id) {
                                    result = Some(n.id);
                                    break;
                                }
                            }
                            result
                        };
                        if let Some(nid) = nid {
                            if let Some(kw) = &old_neuron_kw {
                                if let Some(n) = nn.get_neuron_mut(nid) {
                                    n.keywords = kw.clone();
                                    nn.mark_dirty();
                                }
                            }
                        }
                    }
                }
                return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("WAL write failed: {}", e)}))));
            }
        }
    }
    Ok(Json(serde_json::json!({"success": true, "id": id})))
}

// ─── Update Edge ────────────────────────────────────────────────

/// PUT /edges/:id — Update edge label and properties.
#[derive(Deserialize)]
struct UpdateEdgeRequest {
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    properties: std::collections::HashMap<String, serde_json::Value>,
}

async fn update_edge_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<UpdateEdgeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;
    // ── Step 1: In-memory edge update & save old state ──────
    let mut g = handle.disk_graph.lock().unwrap();
    let edge = g.get_edge(id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "edge not found"})))
    })?;
    let old_label = edge.label.clone();
    let old_properties = edge.properties.clone();
    let props: std::collections::HashMap<String, crate::graph::PropertyValue> = req
        .properties.into_iter().map(|(k, v)| (k, json_to_property(&v))).collect();
    g.update_edge(id, req.label.as_deref(), props);
    drop(g);
    // ── Step 2: In-memory neuron update ──────────────────────
    let old_neuron_kw: Option<Vec<String>>;
    let neuron_for_wal: Option<crate::neuron::Neuron>;
    let neuron_needs_update: bool;
    if req.label.is_some() {
        if let Ok(mut nn) = handle.neural_network.lock() {
            use crate::neuron::neuron::EntityType;
            let nid = {
                let mut result = None;
                for n in nn.all_neurons() {
                    if matches!(n.entity_type, Some(EntityType::Edge(e)) if e == id) {
                        result = Some(n.id);
                        break;
                    }
                }
                result
            };
            if let Some(nid) = nid {
                let n = nn.get_neuron_mut(nid).unwrap();
                old_neuron_kw = Some(n.keywords.clone());
                n.keywords = vec![req.label.as_ref().unwrap().clone()];
                neuron_for_wal = Some(n.clone());
                nn.mark_dirty();
                neuron_needs_update = true;
            } else {
                old_neuron_kw = None;
                neuron_for_wal = None;
                neuron_needs_update = false;
            }
        } else {
            return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "lock error"}))));
        }
    } else {
        old_neuron_kw = None;
        neuron_for_wal = None;
        neuron_needs_update = false;
    }
    // ── Step 3: Atomic WAL batch ─────────────────────────────
    let entries = {
        let mut g = handle.disk_graph.lock().unwrap();
        let e = g.get_edge(id).unwrap();
        let edge_payload = bincode::serialize(
            &crate::storage::redolog_wal::UpdateEdgePayload {
                id, label: e.label.clone(), properties: e.properties.clone(),
            }
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;
        drop(g);
        let mut batch = vec![(crate::storage::redolog_wal::OP_UPDATE_EDGE, edge_payload)];
        if let Some(ref n) = neuron_for_wal {
            let np = bincode::serialize(n)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;
            batch.push((crate::storage::redolog_wal::OP_UPDATE_NEURON, np));
        }
        batch
    };
    if let Ok(mut wal) = handle.redolog_wal.lock() {
        if let Err(e) = wal.write_batch(&entries) {
            // Rollback: restore edge + neuron
            let mut g = handle.disk_graph.lock().unwrap();
            g.update_edge(id, Some(&old_label), old_properties);
            drop(g);
            if neuron_needs_update {
                if let Ok(mut nn) = handle.neural_network.lock() {
                    use crate::neuron::neuron::EntityType;
                    let nid = {
                        let mut result = None;
                        for n in nn.all_neurons() {
                            if matches!(n.entity_type, Some(EntityType::Edge(e)) if e == id) {
                                result = Some(n.id);
                                break;
                            }
                        }
                        result
                    };
                    if let Some(nid) = nid {
                        if let Some(kw) = &old_neuron_kw {
                            if let Some(n) = nn.get_neuron_mut(nid) {
                                n.keywords = kw.clone();
                                nn.mark_dirty();
                            }
                        }
                    }
                }
            }
            return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("WAL write failed: {}", e)}))));
        }
    }
    Ok(Json(serde_json::json!({"success": true, "id": id})))
}

// ─── Re-index ────────────────────────────────────────────────────

/// POST /reindex — Create neurons for all edges that don't have one yet.
/// Returns the count of new edge neurons created.
async fn reindex_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;

    let count = {
        let mut dg = handle.disk_graph.lock().unwrap();
        let g = dg.snapshot();
        drop(dg);
        let mut nn = handle.neural_network.lock().unwrap();
        nn.reindex_edges(&g)
    };

    Ok(Json(serde_json::json!({
        "success": true,
        "graph": graph_name,
        "new_edge_neurons": count,
    })))
}

/// DELETE /edges/{id} — Delete an edge.
/// Supports optional `?force=true` query param.
async fn delete_edge_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;
    let mut g = handle.disk_graph.lock().unwrap();
    let force = params.get("force").map(|v| v == "true").unwrap_or(!g.time_travel_enabled);
    let now = crate::graph::vertex::now_micros();
    // ── Step 1: Save old state + in-memory delete ───────────
    let saved_edge = if force { g.get_edge(id).cloned() } else { None };
    if force { let _ = g.remove_edge(id); }
    else { let _ = g.soft_delete_edge(id, true); }
    drop(g);
    // ── Step 2: Neuron mark_deleted (in-memory) ─────────────
    let saved_neuron: Option<crate::neuron::Neuron>;
    if let Ok(mut nn) = handle.neural_network.lock() {
        use crate::neuron::neuron::EntityType;
        let nid = {
            let mut result = None;
            for n in nn.all_neurons() {
                if matches!(n.entity_type, Some(EntityType::Edge(e)) if e == id) {
                    result = Some(n.id);
                    break;
                }
            }
            result
        };
        if let Some(nid) = nid {
            let n = nn.get_neuron_mut(nid).unwrap();
            saved_neuron = Some(n.clone());
            n.mark_deleted(now);
            nn.mark_dirty();
        } else {
            saved_neuron = None;
        }
    } else {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "lock error"}))));
    }
    // ── Step 3: Atomic batch WAL ────────────────────────────
    let mut entries = Vec::new();
    if force {
        let p = bincode::serialize(&crate::storage::redolog_wal::RemoveEdgePayload { id })
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;
        entries.push((crate::storage::redolog_wal::OP_REMOVE_EDGE, p));
    }
    if let Some(ref n) = saved_neuron {
        let np = bincode::serialize(n)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;
        entries.push((crate::storage::redolog_wal::OP_UPDATE_NEURON, np));
    }
    if !entries.is_empty() {
        if let Ok(mut wal) = handle.redolog_wal.lock() {
            if let Err(e) = wal.write_batch(&entries) {
                // Rollback
                if force {
                    if let Some(edge) = &saved_edge {
                        let mut g = handle.disk_graph.lock().unwrap();
                        let _ = g.add_edge_with_props(edge.label.clone(), edge.source, edge.target, edge.properties.clone());
                        drop(g);
                    }
                }
                if let Some(ref old) = saved_neuron {
                    if let Ok(mut nn) = handle.neural_network.lock() {
                        if nn.get_neuron(old.id).is_some() {
                            nn.remove_neuron(old.id);
                        }
                        nn.add_neuron(old.clone());
                        nn.mark_dirty();
                    }
                }
                return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("WAL write failed: {}", e)}))));
            }
        }
    }
    Ok(Json(serde_json::json!({"success": true, "deleted": id})))
}

// ─── Helpers ─────────────────────────────────────────────────────

fn json_to_property(value: &Value) -> crate::graph::PropertyValue {
    match value {
        Value::String(s) => crate::graph::PropertyValue::String(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() { crate::graph::PropertyValue::Integer(i) }
            else if let Some(f) = n.as_f64() { crate::graph::PropertyValue::Float(f) }
            else { crate::graph::PropertyValue::Null }
        }
        Value::Bool(b) => crate::graph::PropertyValue::Boolean(*b),
        Value::Array(arr) => crate::graph::PropertyValue::List(arr.iter().map(json_to_property).collect()),
        Value::Null => crate::graph::PropertyValue::Null,
        Value::Object(_) => crate::graph::PropertyValue::String(value.to_string()),
    }
}
