use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use uuid::Uuid;

use crate::graph_manager::GraphManager;

use crate::extract::{ExtractionConfig, ExtractionTaskManager};

use super::query::{GremlinQuery, QueryResponse, TraversalResult};
use super::steps::{execute_query, execute_query_with_llm};

// ─── Search Task Manager ─────────────────────────────────────────

/// Status of an async search task.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SearchTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

/// A step in the search pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct SearchTaskStep {
    pub name: String,
    pub status: String, // "pending" | "running" | "done" | "failed"
}

/// An async semantic search task.
#[derive(Debug, Clone, Serialize)]
pub struct SearchTask {
    pub task_id: String,
    pub status: SearchTaskStatus,
    pub steps: Vec<SearchTaskStep>,
    pub current_step: usize,
    pub results: Option<QueryResponse>,
    pub error: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

/// Manages async search tasks.
#[derive(Clone)]
pub struct SearchTaskManager {
    tasks: Arc<Mutex<HashMap<String, SearchTask>>>,
}

impl SearchTaskManager {
    pub fn new() -> Self {
        Self { tasks: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn create_task(&self) -> String {
        let task_id = Uuid::new_v4().to_string();
        let task = SearchTask {
            task_id: task_id.clone(),
            status: SearchTaskStatus::Pending,
            steps: vec![
                SearchTaskStep { name: "Extracting keywords from query".to_string(), status: "pending".to_string() },
                SearchTaskStep { name: "Searching knowledge graph".to_string(), status: "pending".to_string() },
                SearchTaskStep { name: "Filtering results by relevance".to_string(), status: "pending".to_string() },
            ],
            current_step: 0,
            results: None,
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
        };
        self.tasks.lock().unwrap().insert(task_id.clone(), task);
        task_id
    }

    pub fn set_step(&self, task_id: &str, step_idx: usize, status: &str) {
        if let Some(task) = self.tasks.lock().unwrap().get_mut(task_id) {
            if let Some(step) = task.steps.get_mut(step_idx) {
                step.status = status.to_string();
            }
            task.current_step = step_idx;
            task.status = SearchTaskStatus::Running;
        }
    }

    pub fn complete(&self, task_id: &str, results: QueryResponse) {
        if let Some(task) = self.tasks.lock().unwrap().get_mut(task_id) {
            task.status = SearchTaskStatus::Completed;
            task.results = Some(results);
            task.completed_at = Some(chrono::Utc::now().to_rfc3339());
            if let Some(step) = task.steps.last_mut() {
                step.status = "done".to_string();
            }
        }
    }

    pub fn fail(&self, task_id: &str, error: String) {
        if let Some(task) = self.tasks.lock().unwrap().get_mut(task_id) {
            task.status = SearchTaskStatus::Failed;
            task.error = Some(error);
            task.completed_at = Some(chrono::Utc::now().to_rfc3339());
        }
    }

    pub fn get_task(&self, task_id: &str) -> Option<SearchTask> {
        self.tasks.lock().unwrap().get(task_id).cloned()
    }
}

impl Default for SearchTaskManager {
    fn default() -> Self { Self::new() }
}

// ─── AppState ────────────────────────────────────────────────────

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub graph_manager: Arc<Mutex<GraphManager>>,
    pub extraction_config: Option<ExtractionConfig>,
    pub task_manager: ExtractionTaskManager,
    pub search_task_manager: SearchTaskManager,
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
        .route("/search/semantic", post(semantic_search_handler))
        .route("/search/task/:task_id", get(search_task_handler))

        // Document extraction (async task API)
        .route("/extract", post(extract_handler_async))
        .route("/extract/task/:task_id", get(extract_task_handler))
        .route("/extract/tasks", get(extract_tasks_handler))

        // Compaction
        .route("/compact", post(compact_handler))

        // Re-index edges into neural network
        .route("/reindex", post(reindex_handler))

        // UI — redirect / → /ui/
        .route("/", get(|| async { axum::response::Redirect::to("/ui/") }))
        .nest_service("/ui", ServeDir::new("src/ui/dist"))

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
    let (g, n, c) = {
        let gm = state.graph_manager.lock().unwrap();
        match gm.get(&graph_name) {
            Some(handle) => (handle.graph.clone(), handle.neural_network.clone(), state.extraction_config.clone()),
            None => return Json(QueryResponse {
                success: false, data: vec![], error: Some(format!("Graph '{}' not found", graph_name)),
                ticks_used: None, neurons_fired: None,
            }),
        }
    };
    // Run query on a blocking thread to avoid block_on within tokio runtime
    let result = tokio::task::spawn_blocking(move || {
        execute_query_with_llm(&g, &n, &query, c.as_ref())
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

// ─── Extraction (Async Task API) ──────────────────────────────

/// POST /extract — Submit a markdown document for async extraction.
/// Returns immediately with a task_id. Poll GET /extract/task/{id} for progress.
async fn extract_handler_async(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let config = state.extraction_config.as_ref().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "Extraction not configured. Set BGRAPH_LLM_API_KEY."})))
    })?;
    let graph_name = resolve_graph_name(&headers);

    let content = if let Ok(json) = serde_json::from_str::<Value>(&body) {
        json.get("content").or_else(|| json.get("markdown")).and_then(|v| v.as_str()).unwrap_or(&body).to_string()
    } else {
        body
    };
    if content.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Empty content"}))));
    }

    // Get graph handle and clone the Arc for the async call
    let (handle, source_name) = {
        let gm = state.graph_manager.lock().unwrap();
        let h = gm.get(&graph_name).cloned().ok_or_else(|| {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("Graph '{}' not found", graph_name)})))
        })?;
        let source = format!("{}.md", graph_name);
        (h, source)
    };

    // Submit as async background task
    let task_id = state.task_manager.submit_extraction(
        config.clone(),
        content,
        source_name,
        handle.graph,
        handle.neural_network,
        graph_name.clone(),
    );

    Ok(Json(serde_json::json!({
        "task_id": task_id,
        "status": "pending"
    })))
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

// ─── Extraction Task Handlers ─────────────────────────────────────

/// GET /extract/task/{task_id} — Get the status and results of an extraction task.
async fn extract_task_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.task_manager.get_task(&task_id) {
        Some(task) => Ok(Json(serde_json::json!(task))),
        None => Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Task not found"})))),
    }
}

/// GET /extract/tasks — List all extraction tasks (newest first).
async fn extract_tasks_handler(
    State(state): State<AppState>,
) -> Json<Value> {
    let tasks = state.task_manager.list_tasks();
    Json(serde_json::json!({
        "tasks": tasks,
        "count": tasks.len()
    }))
}

// ─── Async Semantic Search ────────────────────────────────────────

/// POST /search/semantic — Submit a semantic search as an async task.
async fn semantic_search_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_name = resolve_graph_name(&headers);
    let query = body.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if query.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Empty query"}))));
    }

    // Extract graph handles
    let (graph, neural, config) = {
        let gm = state.graph_manager.lock().unwrap();
        match gm.get(&graph_name) {
            Some(handle) => (handle.graph.clone(), handle.neural_network.clone(), state.extraction_config.clone()),
            None => return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Graph not found"})))),
        }
    };

    let task_id = state.search_task_manager.create_task();
    let task_id_clone = task_id.clone();
    let mgr = state.search_task_manager.clone();

    tokio::spawn(async move {
        let tid = task_id_clone;

        // Step 1: Extract keywords
        mgr.set_step(&tid, 0, "running");
        let keywords = if let Some(ref config) = config {
            let system_prompt = "Extract 3-5 key search keywords from this query. Keep names in their original language (e.g. Chinese names stay in Chinese). Return ONLY a JSON array of strings, no other text.";
            let user_msg = format!("Query: {}", query);
            match crate::extract::llm_client::chat_completion_with_retry(config, system_prompt, &user_msg).await {
                Ok(result) => {
                    let trimmed = result.content.trim();
                    if trimmed.starts_with('[') {
                        serde_json::from_str::<Vec<String>>(trimmed).unwrap_or_else(|_| {
                            query.split_whitespace().map(|s| s.to_string()).collect()
                        })
                    } else {
                        query.split_whitespace().map(|s| s.to_string()).collect()
                    }
                }
                Err(_) => query.split_whitespace().map(|s| s.to_string()).collect(),
            }
        } else {
            query.split_whitespace().map(|s| s.to_string()).collect()
        };
        mgr.set_step(&tid, 0, "done");

        // Step 2: Neural search
        mgr.set_step(&tid, 1, "running");
        let query_str = keywords.join(" ");
        let (ranked_vertices, ranked_edges, _fired, _hot, _ticks) = {
            let mut nn = neural.lock().unwrap();
            nn.search(&query_str)
        };
        let mut results: Vec<TraversalResult> = ranked_vertices.into_iter().take(100)
            .map(|(vid, _)| TraversalResult::VertexResult(super::query::VertexResult {
                element_type: "vertex".to_string(), id: vid,
                labels: Vec::new(), properties: std::collections::HashMap::new(),
            })).collect();
        {
            let g = graph.lock().unwrap();
            for (eid, _) in ranked_edges {
                if let Some(e) = g.get_edge(eid) {
                    results.push(crate::gremlin::steps::edge_to_result(e));
                }
            }
        }
        // Fill vertex details
        let results = match super::steps::fill_vertex_details(&graph.lock().unwrap(), results) {
            Ok(r) => r,
            Err(e) => { mgr.fail(&tid, e); return; }
        };
        mgr.set_step(&tid, 1, "done");

        // Step 3: Semantic filter (async — limit to first 30 results)
        mgr.set_step(&tid, 2, "running");
        let filter_input: Vec<_> = results.into_iter().take(30).collect();
        let final_results = if let Some(ref config) = config {
            // Build a summary of the search results
            let mut summary = String::new();
            for (i, r) in filter_input.iter().enumerate() {
                match r {
                    TraversalResult::VertexResult(v) => {
                        let name = v.properties.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let labels = v.labels.join(", ");
                        summary.push_str(&format!("{}. V:{} [{}]\n", i + 1, name, labels));
                    }
                    TraversalResult::EdgeResult(e) => {
                        summary.push_str(&format!("{}. EDGE:{} ({})\n", i + 1, e.label, e.id));
                    }
                    _ => {}
                }
            }
            let system_prompt = "You are a semantic relevance filter. Given a user query and a list of search results, return ONLY the indices (comma-separated, 1-based) of results that are semantically relevant to the query. If none are relevant, return \"NONE\". No other text.";
            let user_msg = format!("Query: {}\n\nSearch results:\n{}", query, summary);
            match crate::extract::llm_client::chat_completion_with_retry(config, system_prompt, &user_msg).await {
                Ok(reply) => {
                    let text = reply.content.trim();
                    if text == "NONE" {
                        Vec::new()
                    } else {
                        let indices: Vec<usize> = text.split(',')
                            .filter_map(|s| s.trim().parse::<usize>().ok())
                            .filter(|&i| i > 0 && i <= filter_input.len())
                            .map(|i| i - 1)
                            .collect();
                        if indices.is_empty() {
                            filter_input
                        } else {
                            indices.into_iter().map(|i| filter_input[i].clone()).collect()
                        }
                    }
                }
                Err(_) => filter_input,
            }
        } else {
            filter_input
        };
        mgr.set_step(&tid, 2, "done");

        // Complete
        let response = QueryResponse {
            success: true,
            data: final_results,
            error: None,
            ticks_used: Some(0),
            neurons_fired: Some(vec![]),
        };
        mgr.complete(&tid, response);
    });

    Ok(Json(serde_json::json!({"task_id": task_id, "status": "pending"})))
}

/// GET /search/task/{task_id} — Poll the status and results of an async search task.
async fn search_task_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.search_task_manager.get_task(&task_id) {
        Some(task) => Ok(Json(serde_json::json!(task))),
        None => Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Task not found"})))),
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
