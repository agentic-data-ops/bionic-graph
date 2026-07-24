//! Batch data import — upsert vertices by name, upsert edges by (source, target, name).
//!
//! Provides a single function [`batch_import`] that accepts lists of entities and
//! relations (where edges reference vertices by string name rather than numeric ID)
//! and performs upsert logic matching the extraction pipeline.

use std::collections::HashMap;
use std::sync::Arc;

use crate::graph::crud;
use crate::graph::graph::Graph;
use crate::graph::locked;
use crate::storage::types::PropertyValue;
use crate::storage::types::StorageResult;

/// A batch import item describing a vertex.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct BatchEntity {
    pub name: String,
    pub labels: Vec<String>,
    pub keywords: Vec<String>,
    pub properties: HashMap<String, serde_json::Value>,
}

impl Default for BatchEntity {
    fn default() -> Self {
        Self {
            name: String::new(),
            labels: vec!["entity".to_string()],
            keywords: vec![],
            properties: HashMap::new(),
        }
    }
}

/// A batch import item describing an edge, with source/target as vertex names.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct BatchRelation {
    pub source: String,
    pub target: String,
    pub name: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default = "default_strength")]
    pub strength: f32,
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
}

fn default_strength() -> f32 { 1.0 }

/// A batch delete item for an edge, identified by source/target names and edge name.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct BatchDeleteEdge {
    pub source: String,
    pub target: String,
    pub name: String,
}

/// Result of a batch delete operation.
#[derive(Clone, Debug, serde::Serialize)]
pub struct BatchDeleteResult {
    pub vertices_deleted: usize,
    pub edges_deleted: usize,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct BatchImportResult {
    pub vertices_created: usize,
    pub vertices_updated: usize,
    pub vertices_skipped: usize,
    pub edges_created: usize,
    pub edges_updated: usize,
    pub edges_skipped: usize,
}

/// Upsert a single vertex by name: create if absent, update if present.
fn upsert_vertex(
    graph: &Arc<Graph>,
    name: &str,
    labels: &[String],
    keywords: &[String],
    properties: &HashMap<String, serde_json::Value>,
    name_to_vid: &mut HashMap<String, u32>,
    _source_doc_id: &str,
) -> StorageResult<()> {
    let props: HashMap<String, PropertyValue> = properties.iter()
        .map(|(k, v)| (k.clone(), json_to_property_value(v)))
        .collect();

    if let Some(&existing_vid) = name_to_vid.get(name) {
        // Update existing vertex
        locked::update_vertex_locked(
            graph, existing_vid, Some(name), Some(labels), Some(keywords), Some(&props), true,
        )?;
    } else {
        // Create new vertex
        let vid = locked::create_vertex_locked(graph, name, labels, keywords, &props)?;
        name_to_vid.insert(name.to_string(), vid);
    }
    Ok(())
}

/// Upsert a single edge by (source_name, target_name, edge_name).
fn upsert_edge(
    graph: &Arc<Graph>,
    src_name: &str,
    tgt_name: &str,
    rel_name: &str,
    labels: &[String],
    keywords: &[String],
    strength: f32,
    properties: &HashMap<String, serde_json::Value>,
    name_to_vid: &HashMap<String, u32>,
    edge_key_to_eid: &mut HashMap<(String, String, String), u32>,
    source_doc_id: &str,
) -> StorageResult<()> {
    let Some(&src_vid) = name_to_vid.get(src_name) else { return Ok(()) };
    let Some(&tgt_vid) = name_to_vid.get(tgt_name) else { return Ok(()) };

    let mut props: HashMap<String, PropertyValue> = properties.iter()
        .map(|(k, v)| (k.clone(), json_to_property_value(v)))
        .collect();
    props.insert("_source_doc_id".to_string(), PropertyValue::String(source_doc_id.to_string()));

    let key = (src_name.to_string(), tgt_name.to_string(), rel_name.to_string());
    if let Some(&existing_eid) = edge_key_to_eid.get(&key) {
        // Update existing edge
        locked::update_edge_locked(
            graph, existing_eid, Some(rel_name), Some(labels), Some(keywords),
            Some(strength), Some(&props), true,
        )?;
    } else {
        // Create new edge
        let eid = locked::create_edge_locked(
            graph, src_vid, tgt_vid, rel_name, labels, keywords, strength, &props,
        )?;
        edge_key_to_eid.insert(key, eid);
    }
    Ok(())
}

/// Build a name→vid map from the graph's current vertex data.
///
/// Uses the `vertex_names` B-tree (built at startup from data file scan)
/// to avoid reading the full vertex data payload for each vertex.
pub fn build_name_to_vid(graph: &Arc<Graph>) -> HashMap<String, u32> {
    let mem = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
    mem.vertex_names.iter().map(|(k, v)| (k.clone(), *v)).collect()
}

/// Build an edge lookup keyed by (src_name, tgt_name, edge_name).
pub fn build_edge_lookup(
    graph: &Arc<Graph>,
    name_to_vid: &HashMap<String, u32>,
) -> HashMap<(String, String, String), u32> {
    let mut map = HashMap::new();
    let eids: Vec<u32> = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.edges.keys().copied().collect()
    };
    // Build reverse vid→name map for efficient lookup
    let vid_to_name: HashMap<u32, &str> = name_to_vid.iter().map(|(n, &v)| (v, n.as_str())).collect();
    for eid in eids {
        if let Ok(Some(payload)) = crud::get_edge(graph, eid) {
            if let (Some(&src_name), Some(&tgt_name)) =
                (vid_to_name.get(&payload.source), vid_to_name.get(&payload.target))
            {
                map.insert(
                    (src_name.to_string(), tgt_name.to_string(), payload.name),
                    eid,
                );
            }
        }
    }
    map
}

/// Batch import entities and relations into a graph.
///
/// - Vertices are upserted by `name`.
/// - Edges are upserted by `(source_name, target_name, name)`.
/// - `source_doc_id` is optional; pass `""` to skip the `_source_doc_id` property.
///
/// Returns counts of created/updated vertices and edges.
pub fn batch_import(
    graph: &Arc<Graph>,
    entities: &[BatchEntity],
    relations: &[BatchRelation],
    source_doc_id: &str,
    update_existing: bool,
) -> BatchImportResult {
    let mut name_to_vid: HashMap<String, u32>;
    let mut edge_key_to_eid: HashMap<(String, String, String), u32>;

    if update_existing {
        // Upsert mode: build lookup maps from existing graph data
        name_to_vid = build_name_to_vid(graph);
        edge_key_to_eid = build_edge_lookup(graph, &name_to_vid);
    } else {
        // Append mode: always create new vertices, but still load existing
        // name→vid mapping so edges can reference vertices created in prior
        // batches or separate batch_load calls.
        name_to_vid = build_name_to_vid(graph);
        edge_key_to_eid = HashMap::new();
    }

    let mut result = BatchImportResult {
        vertices_created: 0,
        vertices_updated: 0,
        vertices_skipped: 0,
        edges_created: 0,
        edges_updated: 0,
        edges_skipped: 0,
    };

    // Import vertices
    for entity in entities {
        if update_existing {
            let existed = name_to_vid.contains_key(&entity.name);
            if existed {
                match upsert_vertex(
                    graph, &entity.name, &entity.labels, &entity.keywords,
                    &entity.properties, &mut name_to_vid, source_doc_id,
                ) {
                    Ok(()) => { result.vertices_updated += 1; }
                    Err(e) => log::warn!("Failed to update vertex '{}': {}", entity.name, e),
                }
            } else {
                match upsert_vertex(
                    graph, &entity.name, &entity.labels, &entity.keywords,
                    &entity.properties, &mut name_to_vid, source_doc_id,
                ) {
                    Ok(()) => { result.vertices_created += 1; }
                    Err(e) => log::warn!("Failed to create vertex '{}': {}", entity.name, e),
                }
            }
        } else {
            // Append mode: create a new vertex unconditionally.
            let props: HashMap<String, PropertyValue> = entity.properties.iter()
                .map(|(k, v)| (k.clone(), json_to_property_value(v)))
                .collect();
            match locked::create_vertex_locked(
                graph, &entity.name, &entity.labels, &entity.keywords, &props,
            ) {
                Ok(vid) => { name_to_vid.insert(entity.name.clone(), vid); result.vertices_created += 1; }
                Err(e) => log::warn!("Failed to create vertex '{}': {}", entity.name, e),
            }
        }
    }

    // Import edges
    for rel in relations {
        if update_existing {
            let key = (rel.source.clone(), rel.target.clone(), rel.name.clone());
            let existed = edge_key_to_eid.contains_key(&key);
            if existed {
                if let Err(e) = upsert_edge(
                    graph, &rel.source, &rel.target, &rel.name,
                    &rel.labels, &rel.keywords, rel.strength, &rel.properties,
                    &name_to_vid, &mut edge_key_to_eid, source_doc_id,
                ) {
                    log::warn!("Failed to update edge '{}->{}': {}", rel.source, rel.name, e);
                    continue;
                }
                result.edges_updated += 1;
            } else {
                if let Err(e) = upsert_edge(
                    graph, &rel.source, &rel.target, &rel.name,
                    &rel.labels, &rel.keywords, rel.strength, &rel.properties,
                    &name_to_vid, &mut edge_key_to_eid, source_doc_id,
                ) {
                    log::warn!("Failed to create edge '{}->{}': {}", rel.source, rel.name, e);
                    continue;
                }
                result.edges_created += 1;
            }
        } else {
            // Append mode: create edge unconditionally using batch-local name→vid mapping.
            if let (Some(&src_vid), Some(&tgt_vid)) =
                (name_to_vid.get(&rel.source), name_to_vid.get(&rel.target))
            {
                let mut props: HashMap<String, PropertyValue> = rel.properties.iter()
                    .map(|(k, v)| (k.clone(), json_to_property_value(v)))
                    .collect();
                props.insert("_source_doc_id".to_string(), PropertyValue::String(source_doc_id.to_string()));
                match locked::create_edge_locked(
                    graph, src_vid, tgt_vid, &rel.name,
                    &rel.labels, &rel.keywords, rel.strength, &props,
                ) {
                    Ok(_) => { result.edges_created += 1; }
                    Err(e) => log::warn!("Failed to create edge '{}->{}': {}", rel.source, rel.name, e),
                }
            } else {
                log::warn!("Skipping edge '{}': source '{}' or target '{}' not found in batch",
                    rel.name, rel.source, rel.target);
            }
        }
    }

    result
}

/// Batch delete vertices and edges from a graph.
///
/// - `vertex_names`: vertices to delete by name. All edges connected to these
///   vertices (both incoming and outgoing) are deleted first.
/// - `edges`: specific edges to delete by (source_name, target_name, name).
///
/// Edges listed in `edges` are deleted first, then all edges connected to
/// deleted vertices, then the vertices themselves.
pub fn batch_delete(
    graph: &Arc<Graph>,
    vertex_names: &[String],
    edges: &[BatchDeleteEdge],
) -> BatchDeleteResult {
    let name_to_vid = build_name_to_vid(graph);
    let mut result = BatchDeleteResult {
        vertices_deleted: 0,
        edges_deleted: 0,
    };

    // Phase 1: Delete specified edges by (source_name, target_name, name)
    for edge in edges {
        let Some(&src_vid) = name_to_vid.get(&edge.source) else { continue };
        let Some(&tgt_vid) = name_to_vid.get(&edge.target) else { continue };
        // Find the edge in the adjacency index
        let eid = {
            let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
            mi.adjacency.out_edges(src_vid).iter()
                .find(|(_, t, _)| *t == tgt_vid)
                .map(|(e, _, _)| *e)
        };
        if let Some(eid) = eid {
            if crate::graph::locked::hard_delete_edge_locked(graph, eid).is_ok() {
                result.edges_deleted += 1;
            }
        }
    }

    // Phase 2: Collect all vertex IDs to delete, and all edges connected to them
    let mut vids_to_delete: Vec<u32> = Vec::new();
    let mut edge_ids_to_delete: Vec<u32> = Vec::new();
    for name in vertex_names {
        if let Some(&vid) = name_to_vid.get(name) {
            vids_to_delete.push(vid);
            // Collect all edges from adjacency index (both outgoing and incoming)
            let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
            for (eid, _, _) in mi.adjacency.out_edges(vid) {
                edge_ids_to_delete.push(*eid);
            }
            for (eid, _, _) in mi.adjacency.in_edges(vid) {
                edge_ids_to_delete.push(*eid);
            }
        }
    }
    // Dedup edge IDs
    edge_ids_to_delete.sort();
    edge_ids_to_delete.dedup();

    // Phase 3: Delete collected edges
    for eid in &edge_ids_to_delete {
        if crate::graph::locked::hard_delete_edge_locked(graph, *eid).is_ok() {
            result.edges_deleted += 1;
        }
    }

    // Phase 4: Delete vertices
    for vid in &vids_to_delete {
        if crate::graph::locked::hard_delete_vertex_locked(graph, *vid).is_ok() {
            result.vertices_deleted += 1;
        }
    }

    result
}

/// Convert a serde_json::Value to a PropertyValue.
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
