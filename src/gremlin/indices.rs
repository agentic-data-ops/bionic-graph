//! REST handlers for custom vertex/edge property index management.
//!
//! Routes:
//!   POST   /indices/vertex/properties              — register + auto-scan
//!   DELETE /indices/vertex/properties               — unregister multiple keys
//!   GET    /indices/vertex/properties               — list with per-key stats
//!   GET    /indices/vertex/properties/:key           — show value statistics
//!   DELETE /indices/vertex/properties/:key           — unregister a single key
//!   (same for /indices/edge/properties/*)

use axum::{extract::{Path, State}, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::gremlin::AppState;
use crate::graph::graph::Graph;
use crate::storage::memory_index::MetaPointer;
use crate::storage::types::PropertyValue;

// ── Request types ──────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
pub struct RegisterKeyBody {
    pub key: String,
}

#[derive(Deserialize, Serialize)]
pub struct UnregisterKeysBody {
    pub keys: Vec<String>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn get_graph(state: &AppState, graph_name: Option<&str>) -> Arc<Graph> {
    let name = match graph_name {
        Some(n) => n.to_string(),
        None => state.gm.get_default_name(),
    };
    state.gm.get(&name).unwrap()
}

fn prop_val_str(pv: &PropertyValue) -> String {
    match pv {
        PropertyValue::String(s) => s.clone(),
        PropertyValue::Integer(i) => i.to_string(),
        PropertyValue::Float(f) => format!("{:.2}", f),
        PropertyValue::Boolean(b) => b.to_string(),
        PropertyValue::List(_) | PropertyValue::Null => String::new(),
    }
}

/// Scan all vertices and populate index for a given property key.
fn scan_vertex_property(graph: &Graph, key: &str) {
    let pairs: Vec<(u32, MetaPointer)> = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.vertex_id.iter().map(|(&vid, &p)| (vid, p)).collect()
    };
    for (vid, ptr) in &pairs {
        if let Ok(Some(payload)) = crate::graph::crud::get_vertex(graph, *vid) {
            if let Some(val) = payload.properties.get(key) {
                let s = prop_val_str(val);
                if !s.is_empty() {
                    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                    mi.insert_vertex_property(key, &s, *ptr);
                }
            }
        }
    }
}

fn scan_edge_property(graph: &Graph, key: &str) {
    let pairs: Vec<(u32, MetaPointer)> = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.edge_id.iter().map(|(&eid, &p)| (eid, p)).collect()
    };
    for (eid, ptr) in &pairs {
        if let Ok(Some(payload)) = crate::graph::crud::get_edge(graph, *eid) {
            if let Some(val) = payload.properties.get(key) {
                let s = prop_val_str(val);
                if !s.is_empty() {
                    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                    mi.insert_edge_property(key, &s, *ptr);
                }
            }
        }
    }
}

// ── Vertex property index handlers ─────────────────────────────────────────

pub async fn create_vertex_property_index(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<RegisterKeyBody>,
) -> Json<serde_json::Value> {
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let gateway = state.cluster_gateway();

    // Worker → Master forwarding via ClusterGateway
    let req = crate::cluster::request::ClusterRequest::new("POST", "/indices/vertex/properties")
        .with_body(&body_str)
        .with_opt_graph(graph_name);
    match gateway.forward::<serde_json::Value>(&req).await {
        Ok(Some(val)) => return Json(val),
        Ok(None) => {}
        Err(_) => return Json(serde_json::json!({"status": "error"})),
    }

    let graph = get_graph(&state, graph_name);
    let already = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.has_vertex_property(&body.key)
    };
    if !already {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.register_vertex_property(&body.key);
        drop(mi);
        scan_vertex_property(&graph, &body.key);
    }
    // Persist the updated index keys to config.json.
    let _ = graph.persist_indices_config();
    gateway.broadcast(&req);
    Json(serde_json::json!({
        "status": "ok", "key": body.key, "type": "vertex", "created": !already,
    }))
}

pub async fn show_vertex_property_index(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Json<serde_json::Value> {
    let graph_name = None;
    let graph = get_graph(&state, graph_name);
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
    match mi.vertex_properties.get(&key) {
        Some(data) => {
            let total: usize = data.values().map(|v| v.len()).sum();
            Json(serde_json::json!({
                "status": "ok", "key": key, "type": "vertex",
                "total_entities": total,
            }))
        }
        None => Json(serde_json::json!({
            "status": "ok", "key": key, "type": "vertex",
            "total_entities": 0, "values": [],
        })),
    }
}

pub async fn list_vertex_property_indices(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let graph_name = None;
    let graph = get_graph(&state, graph_name);
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
    let mut indices = Vec::new();
    for key in mi.list_vertex_property_keys() {
        if let Some(data) = mi.vertex_properties.get(&key) {
            let total: usize = data.values().map(|v| v.len()).sum();
            indices.push(serde_json::json!({
                "key": key, "total_entities": total,
            }));
        }
    }
    Json(serde_json::json!({"status": "ok", "indices": indices}))
}

pub async fn delete_vertex_property_index(
    State(state): State<AppState>,
    Path(key): Path<String>,
    headers: axum::http::HeaderMap,
) -> Json<serde_json::Value> {
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let gateway = state.cluster_gateway();

    // Worker → Master forwarding via ClusterGateway
    let req = crate::cluster::request::ClusterRequest::new("DELETE", &format!("/indices/vertex/properties/{}", key))
        .with_opt_graph(graph_name);
    match gateway.forward::<serde_json::Value>(&req).await {
        Ok(Some(val)) => return Json(val),
        Ok(None) => {}
        Err(_) => return Json(serde_json::json!({"status": "error"})),
    }
    let graph = get_graph(&state, graph_name);
    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    let deleted = mi.unregister_vertex_property(&key);
    drop(mi);
    // Persist the updated index keys to config.json.
    let _ = graph.persist_indices_config();
    gateway.broadcast(&req);
    Json(serde_json::json!({"status": "ok", "key": key, "deleted": deleted}))
}

pub async fn delete_vertex_property_indices(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<UnregisterKeysBody>,
) -> Json<serde_json::Value> {
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let gateway = state.cluster_gateway();

    // Worker → Master forwarding via ClusterGateway
    let req = crate::cluster::request::ClusterRequest::new("DELETE", "/indices/vertex/properties")
        .with_body(&body_str)
        .with_opt_graph(graph_name);
    match gateway.forward::<serde_json::Value>(&req).await {
        Ok(Some(val)) => return Json(val),
        Ok(None) => {}
        Err(_) => return Json(serde_json::json!({"status": "error"})),
    }
    let graph = get_graph(&state, graph_name);
    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    let mut deleted = Vec::new();
    for k in &body.keys {
        if mi.unregister_vertex_property(k) {
            deleted.push(k.clone());
        }
    }
    drop(mi);
    // Persist the updated index keys to config.json.
    let _ = graph.persist_indices_config();
    gateway.broadcast(&req);
    Json(serde_json::json!({"status": "ok", "deleted": deleted}))
}

// ── Edge property index handlers ───────────────────────────────────────────

pub async fn create_edge_property_index(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<RegisterKeyBody>,
) -> Json<serde_json::Value> {
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let gateway = state.cluster_gateway();

    // Worker → Master forwarding via ClusterGateway
    let req = crate::cluster::request::ClusterRequest::new("POST", "/indices/edge/properties")
        .with_body(&body_str)
        .with_opt_graph(graph_name);
    match gateway.forward::<serde_json::Value>(&req).await {
        Ok(Some(val)) => return Json(val),
        Ok(None) => {}
        Err(_) => return Json(serde_json::json!({"status": "error"})),
    }

    let graph = get_graph(&state, graph_name);
    let already = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.has_edge_property(&body.key)
    };
    if !already {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.register_edge_property(&body.key);
        drop(mi);
        scan_edge_property(&graph, &body.key);
    }
    // Persist the updated index keys to config.json.
    let _ = graph.persist_indices_config();
    gateway.broadcast(&req);
    Json(serde_json::json!({
        "status": "ok", "key": body.key, "type": "edge", "created": !already,
    }))
}

pub async fn show_edge_property_index(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Json<serde_json::Value> {
    let graph_name = None;
    let graph = get_graph(&state, graph_name);
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
    match mi.edge_properties.get(&key) {
        Some(data) => {
            let total: usize = data.values().map(|v| v.len()).sum();
            Json(serde_json::json!({
                "status": "ok", "key": key, "type": "edge",
                "total_entities": total,
            }))
        }
        None => Json(serde_json::json!({
            "status": "ok", "key": key, "type": "edge",
            "total_entities": 0, "values": [],
        })),
    }
}

pub async fn list_edge_property_indices(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let graph_name = None;
    let graph = get_graph(&state, graph_name);
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
    let mut indices = Vec::new();
    for key in mi.list_edge_property_keys() {
        if let Some(data) = mi.edge_properties.get(&key) {
            let total: usize = data.values().map(|v| v.len()).sum();
            indices.push(serde_json::json!({
                "key": key, "total_entities": total,
            }));
        }
    }
    Json(serde_json::json!({"status": "ok", "indices": indices}))
}

pub async fn delete_edge_property_index(
    State(state): State<AppState>,
    Path(key): Path<String>,
    headers: axum::http::HeaderMap,
) -> Json<serde_json::Value> {
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let gateway = state.cluster_gateway();

    // Worker → Master forwarding via ClusterGateway
    let req = crate::cluster::request::ClusterRequest::new("DELETE", &format!("/indices/edge/properties/{}", key))
        .with_opt_graph(graph_name);
    match gateway.forward::<serde_json::Value>(&req).await {
        Ok(Some(val)) => return Json(val),
        Ok(None) => {}
        Err(_) => return Json(serde_json::json!({"status": "error"})),
    }
    let graph = get_graph(&state, graph_name);
    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    let deleted = mi.unregister_edge_property(&key);
    drop(mi);
    // Persist the updated index keys to config.json.
    let _ = graph.persist_indices_config();
    gateway.broadcast(&req);
    Json(serde_json::json!({"status": "ok", "key": key, "deleted": deleted}))
}

pub async fn delete_edge_property_indices(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<UnregisterKeysBody>,
) -> Json<serde_json::Value> {
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let gateway = state.cluster_gateway();

    // Worker → Master forwarding via ClusterGateway
    let req = crate::cluster::request::ClusterRequest::new("DELETE", "/indices/edge/properties")
        .with_body(&body_str)
        .with_opt_graph(graph_name);
    match gateway.forward::<serde_json::Value>(&req).await {
        Ok(Some(val)) => return Json(val),
        Ok(None) => {}
        Err(_) => return Json(serde_json::json!({"status": "error"})),
    }
    let graph = get_graph(&state, graph_name);
    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    let mut deleted = Vec::new();
    for k in &body.keys {
        if mi.unregister_edge_property(k) {
            deleted.push(k.clone());
        }
    }
    drop(mi);
    // Persist the updated index keys to config.json.
    let _ = graph.persist_indices_config();
    gateway.broadcast(&req);
    Json(serde_json::json!({"status": "ok", "deleted": deleted}))
}
