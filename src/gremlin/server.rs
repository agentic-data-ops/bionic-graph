use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
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
const DEFAULT_GRAPH: &str = "default";

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
        .route("/graphs/:name", delete(delete_graph_handler))

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

        // Settings
        .route("/settings", get(get_settings_handler))
        .route("/settings", put(update_settings_handler))

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

/// GET /graphs — List all graphs.
async fn list_graphs_handler(
    State(state): State<AppState>,
) -> Json<GraphListResponse> {
    let gm = state.graph_manager.lock().unwrap();
    Json(GraphListResponse {
        graphs: gm.list(),
        default: DEFAULT_GRAPH.to_string(),
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
            if let Ok(g) = h.graph.lock() {
                total_v += g.vertex_count();
                total_e += g.edge_count();
            }
            if let Ok(nn) = h.neural_network.lock() {
                total_n += nn.neuron_count();
            }
        }
    }
    Json(HealthResponse {
        status: "ok".to_string(),
        graphs: gm.len(),
        vertices: total_v,
        edges: total_e,
        neurons: total_n,
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
            Some(handle) => (handle.graph.clone(), handle.neural_network.clone()),
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
    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;
    let labels = req.labels;
    let mut g = handle.graph.lock().unwrap();
    let props: std::collections::HashMap<String, crate::graph::PropertyValue> = req
        .properties.into_iter().map(|(k, v)| (k, json_to_property(&v))).collect();
    let id = g.create_vertex(labels.clone());
    if let Some(v) = g.get_vertex_mut(id) {
        v.name = req.name.clone();
        v.keywords = req.keywords.clone();
        // Remove built-in fields from custom properties
        let mut clean_props = props;
        clean_props.remove("name");
        clean_props.remove("keywords");
        v.properties = clean_props;
    }
    // Graph WAL
    if let Ok(mut wal) = handle.redolog_wal.lock() {
        let _ = wal.append_add_vertex(id, &labels);
    }
    // Auto-create a neuron for this vertex (keywords include name for search)
    let nn_label = labels.first().cloned().unwrap_or_else(|| "entity".to_string());
    let mut keywords = labels.clone();
    keywords.push(req.name.clone());
    for kw in &req.keywords {
        keywords.push(kw.clone());
    }
    {
        let mut nn = handle.neural_network.lock().unwrap();
        let nid = nn.neuron_count() as u64 + 1;
        let neuron = crate::neuron::neuron::Neuron::for_vertex(nid, &nn_label, id)
            .with_keywords(keywords.clone());
        nn.add_neuron(neuron.clone());
        if let Ok(mut wal) = handle.redolog_wal.lock() {
            let _ = wal.append_add_neuron(&neuron);
        }
    }
    Ok(Json(AddVertexResponse { id }))
}

/// POST /edges — Add an edge.
async fn add_edge_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AddEdgeRequest>,
) -> Result<Json<AddEdgeResponse>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;
    let label = req.label.clone();
    let mut g = handle.graph.lock().unwrap();
    let id = g.create_edge(label.clone(), req.source, req.target).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()})))
    })?;
    if let Ok(mut wal) = handle.redolog_wal.lock() {
        let _ = wal.append_add_edge(id, &label, req.source, req.target);
    }
    if let Some(e) = g.get_edge_mut(id) {
        e.properties = req.properties.into_iter().map(|(k, v)| (k, json_to_property(&v))).collect();
    }
    drop(g);
    // Auto-create a neuron for this edge
    let mut nn = handle.neural_network.lock().unwrap();
    let nid = (nn.neuron_count() as u64) + 1;
    let neuron = crate::neuron::neuron::Neuron::for_edge(nid, &label, id)
        .with_keywords(vec![label.clone()]);
    nn.add_neuron(neuron.clone());
    // WAL: log the new neuron
    if let Ok(mut wal) = handle.redolog_wal.lock() {
        let _ = wal.append_add_neuron(&neuron);
    }
    // Auto-create neural synapses between neurons referencing the two vertices
    nn.auto_synapse(req.source, req.target);
    Ok(Json(AddEdgeResponse { id }))
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
            Some(handle) => (handle.graph.clone(), handle.neural_network.clone()),
            None => return Json(QueryResponse {
                success: false, data: vec![], error: Some(format!("Graph '{}' not found", graph_name)),
                ticks_used: None, neurons_fired: None,
            }),
        }
    };
    let gremlin_query = GremlinQuery::new(vec![
        super::query::TraversalStep::Search {
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

    let mut g = handle.graph.lock().unwrap();
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
async fn delete_vertex_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let mut gm = state.graph_manager.lock().unwrap();
    let handle = gm.get_mut(&graph_name).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "graph not found"})))
    })?;
    let mut g = handle.graph.lock().unwrap();
    // Remove edges connected to this vertex
    let edge_ids: Vec<u64> = g.all_edges().filter(|e| e.source == id || e.target == id).map(|e| e.id).collect();
    for eid in edge_ids {
        let _ = g.remove_edge(eid);
        if let Ok(mut wal) = handle.redolog_wal.lock() {
            let _ = wal.append_remove_edge(eid);
        }
    }
    // Remove the vertex
    let _ = g.remove_vertex(id, true);
    if let Ok(mut wal) = handle.redolog_wal.lock() {
        let _ = wal.append_remove_vertex(id);
    }
    // Remove associated neuron
    if let Ok(mut nn) = handle.neural_network.lock() {
        use crate::neuron::neuron::EntityType;
        let nid = nn.all_neurons().find_map(|n| {
            if matches!(n.entity_type, Some(EntityType::Vertex(v)) if v == id) {
                Some(n.id)
            } else {
                None
            }
        });
        if let Some(nid) = nid {
            nn.remove_neuron(nid);
            if let Ok(mut wal) = handle.redolog_wal.lock() {
                let _ = wal.append_remove_neuron(nid);
            }
        }
    }
    Ok(Json(serde_json::json!({"success": true, "deleted": id})))
}

// ─── Settings ────────────────────────────────────────────────────

/// GET /settings — Return full LLM settings (providers list with api_key).
async fn get_settings_handler(
    State(state): State<AppState>,
) -> Json<Value> {
    let s = state.settings.lock().unwrap();
    Json(serde_json::json!({
        "llm": {
            "providers": s.llm.providers,
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
    default_model: String,
    context_window: Option<usize>,
    max_output_tokens: Option<usize>,
    max_retries: Option<u32>,
}

/// PUT /settings — Update full LLM settings and persist to file.
async fn update_settings_handler(
    State(state): State<AppState>,
    Json(req): Json<UpdateLlmRequest>,
) -> Json<Value> {
    let mut s = state.settings.lock().unwrap();
    s.llm.providers = req.providers;
    s.llm.default_model = req.default_model;
    if let Some(v) = req.context_window { s.llm.context_window = v; }
    if let Some(v) = req.max_output_tokens { s.llm.max_output_tokens = v; }
    if let Some(v) = req.max_retries { s.llm.max_retries = v; }

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
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Get doc metadata first (to find source_file in graph)
    let doc = state.document_manager.get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document not found"}))))?;

    let _doc_title = doc.title.clone();

    // Clean up graph vertices associated with this document
    let deleted_vertices = {
        let gm = state.graph_manager.lock().unwrap();
        if let Some(handle) = gm.get("default") {
            let mut g = handle.graph.lock().unwrap();
            // Find vertices with matching document ID
            let to_delete: Vec<u64> = g.vertex_ids()
                .filter_map(|vid| {
                    let v = g.get_vertex(*vid)?;
                    if v.document == id {
                        Some(*vid)
                    } else {
                        None
                    }
                })
                .collect();
            let count = to_delete.len();
            for vid in to_delete {
                // Remove connected edges first
                let edge_ids: Vec<u64> = g.all_edges()
                    .filter(|e| e.source == vid || e.target == vid)
                    .map(|e| e.id)
                    .collect();
                for eid in edge_ids {
                    let _ = g.remove_edge(eid);
                    if let Ok(mut wal) = handle.redolog_wal.lock() {
                        let _ = wal.append_remove_edge(eid);
                    }
                }
                let _ = g.remove_vertex(vid, true);
                if let Ok(mut wal) = handle.redolog_wal.lock() {
                    let _ = wal.append_remove_vertex(vid);
                }
                // Remove associated neuron
                if let Ok(mut nn) = handle.neural_network.lock() {
                    use crate::neuron::neuron::EntityType;
                    let nid = nn.all_neurons().find_map(|n| {
                        if matches!(n.entity_type, Some(EntityType::Vertex(v)) if v == vid) {
                            Some(n.id)
                        } else {
                            None
                        }
                    });
                    if let Some(nid) = nid {
                        nn.remove_neuron(nid);
                        if let Ok(mut wal) = handle.redolog_wal.lock() {
                            let _ = wal.append_remove_neuron(nid);
                        }
                    }
                }
            }
            count
        } else {
            0
        }
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
async fn start_extraction_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);

    // Check document exists
    let doc = state.document_manager.get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document not found"}))))?;

    // Get document content
    let content = state.document_manager.get_content(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document content not found"}))))?;

    // Get graph handle from X-Graph-Name header
    let (graph, neural) = {
        let gm = state.graph_manager.lock().unwrap();
        let handle = gm.get(&graph_name).ok_or_else(|| {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("Graph '{}' not found", graph_name)})))
        })?;
        (handle.graph.clone(), handle.neural_network.clone())
    };

    let title = doc.title.clone();

    // Build ExtractionConfig from current settings at runtime
    let extract_config = {
        let s = state.settings.lock().unwrap();
        ExtractionConfig::from_llm_config(&s.llm)
    };

    let task_id = state.task_manager.submit_document_extraction(
        extract_config,
        id,
        content,
        title,
        graph,
        neural,
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
    let mut g = handle.graph.lock().unwrap();
    let vertex = g.get_vertex_mut(id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "vertex not found"})))
    })?;
    let props: std::collections::HashMap<String, crate::graph::PropertyValue> = req
        .properties.into_iter().map(|(k, v)| (k, json_to_property(&v))).collect();
    if let Some(name) = req.name {
        vertex.name = name;
    }
    if let Some(keywords) = req.keywords {
        vertex.keywords = keywords;
    }
    if !req.labels.is_empty() {
        vertex.labels = req.labels;
    }
    vertex.properties = props;
    let new_keywords: Vec<String> = {
        let v = &vertex;
        let mut kw = v.labels.clone();
        kw.push(v.name.clone());
        for kw_val in &v.keywords {
            kw.push(kw_val.clone());
        }
        kw
    };
    drop(g);
    // Graph WAL
    if let Ok(mut wal) = handle.redolog_wal.lock() {
        let g = handle.graph.lock().unwrap();
        if let Some(v) = g.get_vertex(id) {
            let _ = wal.append_update_vertex(id, &v.labels, &v.properties);
        }
        drop(g);
        drop(wal);
    }
    // Sync neuron keywords
    if let Ok(mut nn) = handle.neural_network.lock() {
        use crate::neuron::neuron::EntityType;
        let nid = nn.all_neurons().find_map(|n| {
            if matches!(n.entity_type, Some(EntityType::Vertex(v)) if v == id) {
                Some(n.id)
            } else {
                None
            }
        });
        if let Some(nid) = nid {
            if let Some(neuron) = nn.get_neuron_mut(nid) {
                neuron.keywords = new_keywords;
                nn.mark_dirty();
            }
            if let Ok(mut wal) = handle.redolog_wal.lock() {
                if let Some(neuron) = nn.get_neuron(nid) {
                    let _ = wal.append_update_neuron(neuron);
                }
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
    let mut g = handle.graph.lock().unwrap();
    let edge = g.get_edge_mut(id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "edge not found"})))
    })?;
    let props: std::collections::HashMap<String, crate::graph::PropertyValue> = req
        .properties.into_iter().map(|(k, v)| (k, json_to_property(&v))).collect();
    if let Some(ref label) = req.label {
        edge.label = label.clone();
    }
    edge.properties = props;
    drop(g);
    // Graph WAL
    if let Ok(mut wal) = handle.redolog_wal.lock() {
        let g = handle.graph.lock().unwrap();
        if let Some(e) = g.get_edge(id) {
            let _ = wal.append_update_edge(id, &e.label, &e.properties);
        }
        drop(g);
        drop(wal);
    }
    // Sync neuron label
    if let Ok(mut nn) = handle.neural_network.lock() {
        use crate::neuron::neuron::EntityType;
        let nid = nn.all_neurons().find_map(|n| {
            if matches!(n.entity_type, Some(EntityType::Edge(e)) if e == id) {
                Some(n.id)
            } else {
                None
            }
        });
        if let Some(nid) = nid {
            if let Some(neuron) = nn.get_neuron_mut(nid) {
                if let Some(ref label) = req.label {
                    neuron.keywords = vec![label.clone()];
                    nn.mark_dirty();
                }
            }
            if let Ok(mut wal) = handle.redolog_wal.lock() {
                if let Some(neuron) = nn.get_neuron(nid) {
                    let _ = wal.append_update_neuron(neuron);
                }
            }
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
        let g = handle.graph.lock().unwrap();
        let mut nn = handle.neural_network.lock().unwrap();
        nn.reindex_edges(&g)
    };

    Ok(Json(serde_json::json!({
        "success": true,
        "graph": graph_name,
        "new_edge_neurons": count,
    })))
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
