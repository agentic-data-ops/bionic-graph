//! Lock-aware wrappers for CRUD operations.
//!
//! These functions acquire the appropriate locks before delegating to the
//! inner CRUD implementation, following the strict lock ordering:
//!
//! ```text
//! metadata → block → vertex → edge
//! ```

use std::sync::Arc;

use crate::graph::crud;
use crate::graph::graph::Graph;
use crate::storage::types::{EdgeId, EdgePayload, PropertyValue, StorageResult, VertexId, VertexPayload};
use std::collections::HashMap;

// ── Vertex operations ───────────────────────────────────────────────────────

pub fn create_vertex_locked(
    graph: &Arc<Graph>,
    name: &str,
    labels: &[String],
    keywords: &[String],
    properties: &HashMap<String, PropertyValue>,
) -> StorageResult<VertexId> {
    let _meta = graph.locks.read_metadata();
    let vid = crud::create_vertex(graph, name, labels, keywords, properties);
    drop(_meta);
    vid
}

/// Create a vertex with a specific ID (locked). Used during cluster replay.
pub fn create_vertex_with_id_locked(
    graph: &Arc<Graph>,
    vid: u32,
    name: &str,
    labels: &[String],
    keywords: &[String],
    properties: &HashMap<String, PropertyValue>,
) -> StorageResult<VertexId> {
    let _meta = graph.locks.read_metadata();
    let result = crud::create_vertex_with_id(graph, vid, name, labels, keywords, properties);
    drop(_meta);
    result
}

pub fn get_vertex_locked(
    graph: &Arc<Graph>,
    vertex_id: VertexId,
) -> StorageResult<Option<VertexPayload>> {
    let _vlock = graph.locks.read_vertex(vertex_id);
    let result = crud::get_vertex(graph, vertex_id);
    drop(_vlock);
    result
}

pub fn update_vertex_locked(
    graph: &Arc<Graph>,
    vertex_id: VertexId,
    name: Option<&str>,
    labels: Option<&[String]>,
    keywords: Option<&[String]>,
    properties: Option<&HashMap<String, PropertyValue>>,
    record_history: bool,
) -> StorageResult<()> {
    let _meta = graph.locks.read_metadata();
    let _vlock = graph.locks.write_vertex(vertex_id);
    let result = crud::update_vertex(graph, vertex_id, name, labels, keywords, properties, record_history);
    drop(_vlock);
    drop(_meta);
    result
}

pub fn soft_delete_vertex_locked(
    graph: &Arc<Graph>,
    vertex_id: VertexId,
) -> StorageResult<()> {
    let _meta = graph.locks.read_metadata();
    let _vlock = graph.locks.write_vertex(vertex_id);
    let result = crud::soft_delete_vertex(graph, vertex_id);
    drop(_vlock);
    drop(_meta);
    result
}

pub fn hard_delete_vertex_locked(
    graph: &Arc<Graph>,
    vertex_id: VertexId,
) -> StorageResult<()> {
    let _meta = graph.locks.read_metadata();
    let _vlock = graph.locks.write_vertex(vertex_id);
    let result = crud::hard_delete_vertex(graph, vertex_id);
    drop(_vlock);
    drop(_meta);
    result
}

// ── Edge operations ─────────────────────────────────────────────────────────

pub fn create_edge_locked(
    graph: &Arc<Graph>,
    source: VertexId,
    target: VertexId,
    name: &str,
    labels: &[String],
    keywords: &[String],
    strength: f32,
    properties: &HashMap<String, PropertyValue>,
) -> StorageResult<EdgeId> {
    let _meta = graph.locks.read_metadata();
    let result = crud::create_edge(graph, source, target, name, labels, keywords, strength, properties);
    drop(_meta);
    result
}

/// Create an edge with a specific ID (locked). Used during cluster replay.
pub fn create_edge_with_id_locked(
    graph: &Arc<Graph>,
    eid: u32,
    source: VertexId,
    target: VertexId,
    name: &str,
    labels: &[String],
    keywords: &[String],
    strength: f32,
    properties: &HashMap<String, PropertyValue>,
) -> StorageResult<EdgeId> {
    let _meta = graph.locks.read_metadata();
    let result = crud::create_edge_with_id(graph, eid, source, target, name, labels, keywords, strength, properties);
    drop(_meta);
    result
}

pub fn get_edge_locked(
    graph: &Arc<Graph>,
    edge_id: EdgeId,
) -> StorageResult<Option<EdgePayload>> {
    let _elock = graph.locks.read_edge(edge_id);
    let result = crud::get_edge(graph, edge_id);
    drop(_elock);
    result
}

pub fn update_edge_locked(
    graph: &Arc<Graph>,
    edge_id: EdgeId,
    name: Option<&str>,
    labels: Option<&[String]>,
    keywords: Option<&[String]>,
    strength: Option<f32>,
    properties: Option<&HashMap<String, PropertyValue>>,
    record_history: bool,
) -> StorageResult<()> {
    let _meta = graph.locks.read_metadata();
    let _elock = graph.locks.write_edge(edge_id);
    let result = crud::update_edge(graph, edge_id, name, labels, keywords, strength, properties, record_history);
    drop(_elock);
    drop(_meta);
    result
}

pub fn soft_delete_edge_locked(
    graph: &Arc<Graph>,
    edge_id: EdgeId,
) -> StorageResult<()> {
    let _meta = graph.locks.read_metadata();
    let _elock = graph.locks.write_edge(edge_id);
    let result = crud::soft_delete_edge(graph, edge_id);
    drop(_elock);
    drop(_meta);
    result
}

pub fn hard_delete_edge_locked(
    graph: &Arc<Graph>,
    edge_id: EdgeId,
) -> StorageResult<()> {
    let _meta = graph.locks.read_metadata();
    let _elock = graph.locks.write_edge(edge_id);
    let result = crud::hard_delete_edge(graph, edge_id);
    drop(_elock);
    drop(_meta);
    result
}

// ── Direct &Graph variants (WAL replay — no Arc needed) ─────────────────

pub fn create_vertex_locked_direct(
    graph: &Graph,
    name: &str,
    labels: &[String],
    keywords: &[String],
    properties: &HashMap<String, PropertyValue>,
) -> StorageResult<VertexId> {
    crud::create_vertex(graph, name, labels, keywords, properties)
}

pub fn update_vertex_locked_direct(
    graph: &Graph,
    vertex_id: VertexId,
    name: Option<&str>,
    labels: Option<&[String]>,
    keywords: Option<&[String]>,
    properties: Option<&HashMap<String, PropertyValue>>,
    append_history: bool,
) -> StorageResult<()> {
    crud::update_vertex(graph, vertex_id, name, labels, keywords, properties, append_history)
}

pub fn hard_delete_vertex_locked_direct(
    graph: &Graph,
    vertex_id: VertexId,
) -> StorageResult<()> {
    crud::hard_delete_vertex(graph, vertex_id)
}

pub fn create_edge_locked_direct(
    graph: &Graph,
    source: u32,
    target: u32,
    name: &str,
    labels: &[String],
    keywords: &[String],
    strength: f32,
    properties: &HashMap<String, PropertyValue>,
) -> StorageResult<EdgeId> {
    crud::create_edge(graph, source, target, name, labels, keywords, strength, properties)
}

pub fn update_edge_locked_direct(
    graph: &Graph,
    edge_id: EdgeId,
    name: Option<&str>,
    labels: Option<&[String]>,
    keywords: Option<&[String]>,
    strength: Option<f32>,
    properties: Option<&HashMap<String, PropertyValue>>,
    append_history: bool,
) -> StorageResult<()> {
    crud::update_edge(graph, edge_id, name, labels, keywords, strength, properties, append_history)
}

pub fn hard_delete_edge_locked_direct(
    graph: &Graph,
    edge_id: EdgeId,
) -> StorageResult<()> {
    crud::hard_delete_edge(graph, edge_id)
}
