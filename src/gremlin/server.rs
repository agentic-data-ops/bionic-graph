use std::collections::HashMap;
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
use uuid::Uuid;

use crate::graph_manager::GraphManager;

use crate::documents::DocumentManager;

use super::query::{GremlinQuery, QueryResponse, TraversalResult};
use super::steps::{execute_query, execute_query_with_llm};

// ─── AppState ────────────────────────────────────────────────────

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub graph_manager: Arc<Mutex<GraphManager>>,
    pub document_manager: DocumentManager,
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

        // Vertex management
        .route("/vertices/:id", delete(delete_vertex_handler))

        // Document management
        .route("/documents", get(list_documents_handler))
        .route("/documents", post(add_document_handler))
        .route("/documents/:id", get(get_document_handler))
        .route("/documents/:id", put(update_document_handler))
        .route("/documents/:id", delete(delete_document_handler))
        .route("/documents/:id/content", get(get_document_content_handler))

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
        v.properties = props;
    }
    // Auto-create a neuron for this vertex
    let nn_label = labels.first().cloned().unwrap_or_else(|| "entity".to_string());
    let neuron = crate::neuron::neuron::Neuron::for_vertex(
        (handle.neural_network.lock().unwrap().neuron_count() as u64) + 1,
        &nn_label, id,
    ).with_keywords(labels);
    handle.neural_network.lock().unwrap().add_neuron(neuron);
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
    if let Some(e) = g.get_edge_mut(id) {
        e.properties = req.properties.into_iter().map(|(k, v)| (k, json_to_property(&v))).collect();
    }
    drop(g);
    // Auto-create a neuron for this edge
    let mut nn = handle.neural_network.lock().unwrap();
    let nid = (nn.neuron_count() as u64) + 1;
    let neuron = crate::neuron::neuron::Neuron::for_edge(nid, &label, id)
        .with_keywords(vec![label.clone()]);
    nn.add_neuron(neuron);
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
    nn.add_neuron(neuron);
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
    handle.neural_network.lock().unwrap().link_vertex(neuron_id, vertex_id);
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
        g.remove_edge(eid);
    }
    // Remove the vertex
    g.remove_vertex(id, true);
    Ok(Json(serde_json::json!({"success": true, "deleted": id})))
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
    let doc = state.document_manager.add(&id, &req.title, &req.content, &req.tags);
    Json(serde_json::json!(doc))
}

#[derive(Deserialize)]
struct UpdateDocumentRequest {
    title: String,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// PUT /documents/{id} — Update document.
async fn update_document_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateDocumentRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.document_manager.update(&id, &req.title, &req.content, &req.tags) {
        Some(doc) => Ok(Json(serde_json::json!(doc))),
        None => Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document not found"})))),
    }
}

/// DELETE /documents/{id} — Delete a document.
async fn delete_document_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if state.document_manager.delete(&id) {
        Ok(Json(serde_json::json!({"success": true})))
    } else {
        Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Document not found"}))))
    }
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
