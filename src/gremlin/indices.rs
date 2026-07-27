//! REST handlers for custom vertex/edge property index management.
//!
//! Routes:
//!   POST   /indices/vertex/properties              — register a property key
//!   DELETE /indices/vertex/properties               — unregister multiple keys
//!   GET    /indices/vertex/properties               — list registered keys
//!   GET    /indices/vertex/properties/:key           — query by key+value
//!   POST   /indices/vertex/properties/query         — batch query
//!   DELETE /indices/vertex/properties/:key           — unregister a single key
//!   (same for /indices/edge/properties/*)

use axum::{extract::{Path, Query, State}, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::gremlin::AppState;
use crate::graph::graph::Graph;
use crate::storage::memory_index::MetaPointer;
use crate::storage::types::StorageResult;

// ── Request/Response types ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RegisterKeyBody {
    pub key: String,
}

#[derive(Deserialize)]
pub struct UnregisterKeysBody {
    pub keys: Vec<String>,
}

#[derive(Deserialize)]
pub struct PropertyQuery {
    pub key: String,
    pub value: String,
}

#[derive(Deserialize)]
pub struct ValueQuery {
    pub value: String,
}

#[derive(Deserialize)]
pub struct BatchQueryBody {
    pub queries: Vec<PropertyQuery>,
}

#[derive(Serialize)]
pub struct KeyResponse {
    pub status: String,
    pub key: String,
    #[serde(rename = "type")]
    pub entity_type: String,
    pub created: Option<bool>,
    pub deleted: Option<bool>,
}

#[derive(Serialize)]
pub struct KeysListResponse {
    pub status: String,
    pub keys: Vec<String>,
}

#[derive(Serialize)]
pub struct IndexQueryResponse {
    pub status: String,
    pub key: String,
    pub value: String,
    pub count: usize,
    pub data: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct BatchQueryResponse {
    pub status: String,
    pub results: Vec<serde_json::Value>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn get_graph(state: &AppState) -> Arc<Graph> {
    let name = state.gm.get_default_name();
    state.gm.get(&name).unwrap()
}

fn prop_val_to_json(pv: &crate::storage::types::PropertyValue) -> serde_json::Value {
    match pv {
        crate::storage::types::PropertyValue::String(s) => serde_json::Value::String(s.clone()),
        crate::storage::types::PropertyValue::Integer(i) => serde_json::json!(i),
        crate::storage::types::PropertyValue::Float(f) => serde_json::json!(f),
        crate::storage::types::PropertyValue::Boolean(b) => serde_json::json!(b),
        crate::storage::types::PropertyValue::List(l) => {
            serde_json::Value::Array(l.iter().map(prop_val_to_json).collect())
        }
        crate::storage::types::PropertyValue::Null => serde_json::Value::Null,
    }
}

/// Read a vertex payload from a MetaPointer and return a JSON object.
fn vertex_to_json(
    graph: &Graph,
    vid: u32,
    ptr: MetaPointer,
) -> Option<serde_json::Value> {
    if let Ok(Some(payload)) = crate::graph::crud::get_vertex(graph, vid) {
        let props: serde_json::Map<String, serde_json::Value> = payload.properties.iter()
            .map(|(k, v)| (k.clone(), prop_val_to_json(v)))
            .collect();
        Some(serde_json::json!({
            "id": vid,
            "name": payload.name,
            "labels": payload.labels,
            "properties": props,
        }))
    } else {
        None
    }
}

/// Read an edge payload from a MetaPointer and return a JSON object.
fn edge_to_json(
    graph: &Graph,
    eid: u32,
    ptr: MetaPointer,
) -> Option<serde_json::Value> {
    if let Ok(Some(payload)) = crate::graph::crud::get_edge(graph, eid) {
        let props: serde_json::Map<String, serde_json::Value> = payload.properties.iter()
            .map(|(k, v)| (k.clone(), prop_val_to_json(v)))
            .collect();
        Some(serde_json::json!({
            "id": eid,
            "name": payload.name,
            "source": payload.source,
            "target": payload.target,
            "labels": payload.labels,
            "strength": payload.strength,
            "properties": props,
        }))
    } else {
        None
    }
}

// ── Vertex property index handlers ─────────────────────────────────────────

pub async fn create_vertex_property_index(
    State(state): State<AppState>,
    Json(body): Json<RegisterKeyBody>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    let already = mi.has_vertex_property(&body.key);
    if !already {
        mi.register_vertex_property(&body.key);
    }
    Json(serde_json::json!(KeyResponse {
        status: "ok".into(),
        key: body.key,
        entity_type: "vertex".into(),
        created: Some(!already),
        deleted: None,
    }))
}

pub async fn delete_vertex_property_index(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    let deleted = mi.unregister_vertex_property(&key);
    Json(serde_json::json!(KeyResponse {
        status: "ok".into(),
        key,
        entity_type: "vertex".into(),
        created: None,
        deleted: Some(deleted),
    }))
}

pub async fn delete_vertex_property_indices(
    State(state): State<AppState>,
    Json(body): Json<UnregisterKeysBody>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    let mut deleted = Vec::new();
    for k in &body.keys {
        if mi.unregister_vertex_property(k) {
            deleted.push(k.clone());
        }
    }
    Json(serde_json::json!({
        "status": "ok",
        "deleted": deleted,
    }))
}

pub async fn list_vertex_property_indices(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
    Json(serde_json::json!(KeysListResponse {
        status: "ok".into(),
        keys: mi.list_vertex_property_keys(),
    }))
}

pub async fn query_vertex_property_index(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Query(params): Query<ValueQuery>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let pairs: Vec<(u32, MetaPointer)> = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        let ptrs = mi.query_vertex_property(&key, &params.value);
        match ptrs {
            Some(ptrs) => mi.vertex_id.iter()
                .filter(|(_, &p)| ptrs.contains(&p))
                .map(|(&vid, &p)| (vid, p))
                .collect(),
            None => Vec::new(),
        }
    };
    let mut data = Vec::with_capacity(pairs.len());
    for (vid, p) in &pairs {
        if let Some(json) = vertex_to_json(&graph, *vid, *p) {
            data.push(json);
        }
    }
    Json(serde_json::json!(IndexQueryResponse {
                status: "ok".into(),
                key,
                value: params.value,
                count: data.len(),
                data,
            }))
    }

pub async fn query_vertex_property_indices(
    State(state): State<AppState>,
    Json(body): Json<BatchQueryBody>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let mut results = Vec::new();
    for q in &body.queries {
        let pairs: Vec<(u32, MetaPointer)> = {
            let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
            let ptrs = mi.query_vertex_property(&q.key, &q.value);
            match ptrs {
                Some(ptrs) => mi.vertex_id.iter()
                    .filter(|(_, &p)| ptrs.contains(&p))
                    .map(|(&vid, &p)| (vid, p))
                    .collect(),
                None => Vec::new(),
            }
        };
        if pairs.is_empty() {
            results.push(serde_json::json!({
                "key": q.key, "value": q.value, "count": 0, "data": [],
            }));
            continue;
        }
        let mut data = Vec::with_capacity(pairs.len());
        for (vid, p) in &pairs {
            if let Some(json) = vertex_to_json(&graph, *vid, *p) {
                data.push(json);
            }
        }
        results.push(serde_json::json!({
            "key": q.key, "value": q.value, "count": data.len(), "data": data,
        }));
    }
    Json(serde_json::json!(BatchQueryResponse {
        status: "ok".into(),
        results,
    }))
}

// ── Edge property index handlers ───────────────────────────────────────────

pub async fn create_edge_property_index(
    State(state): State<AppState>,
    Json(body): Json<RegisterKeyBody>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    let already = mi.has_edge_property(&body.key);
    if !already {
        mi.register_edge_property(&body.key);
    }
    Json(serde_json::json!(KeyResponse {
        status: "ok".into(),
        key: body.key,
        entity_type: "edge".into(),
        created: Some(!already),
        deleted: None,
    }))
}

pub async fn delete_edge_property_index(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    let deleted = mi.unregister_edge_property(&key);
    Json(serde_json::json!(KeyResponse {
        status: "ok".into(),
        key,
        entity_type: "edge".into(),
        created: None,
        deleted: Some(deleted),
    }))
}

pub async fn delete_edge_property_indices(
    State(state): State<AppState>,
    Json(body): Json<UnregisterKeysBody>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    let mut deleted = Vec::new();
    for k in &body.keys {
        if mi.unregister_edge_property(k) {
            deleted.push(k.clone());
        }
    }
    Json(serde_json::json!({
        "status": "ok",
        "deleted": deleted,
    }))
}

pub async fn list_edge_property_indices(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
    Json(serde_json::json!(KeysListResponse {
        status: "ok".into(),
        keys: mi.list_edge_property_keys(),
    }))
}

pub async fn query_edge_property_index(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Query(params): Query<ValueQuery>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let pairs: Vec<(u32, MetaPointer)> = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        let ptrs = mi.query_edge_property(&key, &params.value);
        match ptrs {
            Some(ptrs) => mi.edge_id.iter()
                .filter(|(_, &p)| ptrs.contains(&p))
                .map(|(&eid, &p)| (eid, p))
                .collect(),
            None => Vec::new(),
        }
    };
    let mut data = Vec::with_capacity(pairs.len());
    for (eid, p) in &pairs {
        if let Some(json) = edge_to_json(&graph, *eid, *p) {
            data.push(json);
        }
    }
    Json(serde_json::json!(IndexQueryResponse {
                status: "ok".into(),
                key,
                value: params.value,
                count: data.len(),
                data,
            }))
    }

pub async fn query_edge_property_indices(
    State(state): State<AppState>,
    Json(body): Json<BatchQueryBody>,
) -> Json<serde_json::Value> {
    let graph = get_graph(&state);
    let mut results = Vec::new();
    for q in &body.queries {
        let pairs: Vec<(u32, MetaPointer)> = {
            let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
            let ptrs = mi.query_edge_property(&q.key, &q.value);
            match ptrs {
                Some(ptrs) => mi.edge_id.iter()
                    .filter(|(_, &p)| ptrs.contains(&p))
                    .map(|(&eid, &p)| (eid, p))
                    .collect(),
                None => Vec::new(),
            }
        };
        if pairs.is_empty() {
            results.push(serde_json::json!({
                "key": q.key, "value": q.value, "count": 0, "data": [],
            }));
            continue;
        }
        let mut data = Vec::with_capacity(pairs.len());
        for (eid, p) in &pairs {
            if let Some(json) = edge_to_json(&graph, *eid, *p) {
                data.push(json);
            }
        }
        results.push(serde_json::json!({
            "key": q.key, "value": q.value, "count": data.len(), "data": data,
        }));
    }
    Json(serde_json::json!(BatchQueryResponse {
        status: "ok".into(),
        results,
    }))
}
