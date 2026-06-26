use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::graph::graph::GraphError;
use crate::graph::{Edge, Vertex, VertexId, PropertyValue};

use super::index::{IndexBundle, LabelIndex, SubgraphIndex, VertexIndex};
use super::partition::PartitionConfig;
use super::redo_log::{
    AddCrossEdgePayload, AddEdgePayload, AddVertexPayload, RedoLog, RedoOperation,
    RemoveEdgePayload, RemoveVertexPayload,
};
use super::subgraph::{Subgraph, SubgraphId};
use super::subgraph_cache::SubgraphCache;

/// A disk-backed knowledge graph that loads/stores data in subgraphs.
///
/// Uses:
/// - **SubgraphCache** — LRU cache of subgraph data blocks
/// - **VertexIndex / SubgraphIndex / LabelIndex** — in-memory indices (常驻)
/// - **RedoLog** — WAL for crash recovery
///
/// All mutation goes: RedoLog → cache → (eventually) disk via checkpoint.
pub struct DiskGraph {
    pub cache: SubgraphCache,
    pub vertex_index: VertexIndex,
    pub subgraph_index: SubgraphIndex,
    pub label_index: LabelIndex,
    pub redo_log: RedoLog,
    pub partition_config: PartitionConfig,
    pub time_travel_enabled: bool,
    data_dir: PathBuf,

    /// Global ID counters.
    next_vertex_id: AtomicU64,
    next_edge_id: AtomicU64,

    /// edge_id → containing subgraph_id (built during checkpoint).
    edge_index: HashMap<u64, SubgraphId>,
}

impl DiskGraph {
    /// Open (or create) a disk-backed graph at the given data directory.
    pub fn open(data_dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let data_dir: PathBuf = data_dir.into();

        // Ensure directory structure
        std::fs::create_dir_all(data_dir.join("subgraph"))?;

        // Load or initialize index bundle
        let index_path = data_dir.join("index.bundle");
        let bundle = if index_path.exists() {
            let bytes = std::fs::read(&index_path)?;
            IndexBundle::from_bytes(&bytes).unwrap_or_default()
        } else {
            IndexBundle::new()
        };

        // Open redo log
        let redo_log = RedoLog::open(data_dir.join("redo.log"))?;

        // Create cache
        let cache = SubgraphCache::new(&data_dir);

        let mut graph = Self {
            cache,
            vertex_index: bundle.vertex_index,
            subgraph_index: bundle.subgraph_index,
            label_index: bundle.label_index,
            redo_log,
            partition_config: PartitionConfig::default(),
            time_travel_enabled: false,
            data_dir,
            next_vertex_id: AtomicU64::new(bundle.global_next_vertex_id),
            next_edge_id: AtomicU64::new(bundle.global_next_edge_id),
            edge_index: HashMap::new(),
        };

        // Recover from WAL
        graph.recover_from_wal()?;

        Ok(graph)
    }

    /// Recover from the write-ahead log after a crash.
    fn recover_from_wal(&mut self) -> std::io::Result<()> {
        let entries = self.redo_log.recover()?;
        if entries.is_empty() {
            return Ok(());
        }

        log::info!("Recovering {} entries from redo log", entries.len());

        for entry in &entries {
            match entry.entry_type {
                super::redo_log::ENTRY_ADD_VERTEX => {
                    if let Ok(payload) = bincode::deserialize::<AddVertexPayload>(&entry.data) {
                        self.replay_add_vertex(&payload);
                    }
                }
                super::redo_log::ENTRY_ADD_EDGE => {
                    if let Ok(payload) = bincode::deserialize::<AddEdgePayload>(&entry.data) {
                        self.replay_add_edge(&payload);
                    }
                }
                super::redo_log::ENTRY_REMOVE_VERTEX => {
                    if let Ok(payload) = bincode::deserialize::<RemoveVertexPayload>(&entry.data) {
                        self.replay_remove_vertex(&payload);
                    }
                }
                super::redo_log::ENTRY_REMOVE_EDGE => {
                    if let Ok(payload) = bincode::deserialize::<RemoveEdgePayload>(&entry.data) {
                        self.replay_remove_edge(&payload);
                    }
                }
                _ => {} // Skip checkpoints and unknown entries
            }
        }

        // Flush recovered data to disk
        self.checkpoint()?;

        // Clean up old log files
        self.redo_log.clean_old_logs()?;

        log::info!("Recovery complete");
        Ok(())
    }

    // ─── Vertex Operations ─────────────────────────────────────

    /// Add a vertex, logging to WAL first.
    pub fn add_vertex(&mut self, labels: Vec<String>) -> Result<VertexId, GraphError> {
        let vid = self.next_vertex_id.fetch_add(1, Ordering::SeqCst);

        // 1. Assign to a subgraph
        let sg_id = self.find_or_create_subgraph(&labels);

        // 2. Log to WAL
        self.redo_log
            .append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: sg_id,
                vertex_id: vid,
                labels: labels.clone(),
            }))
            .map_err(|e| {
                log::error!("WAL write failed: {}", e);
                GraphError::VertexNotFound(vid) // best approximation
            })?;

        // 3. Apply to cache
        if let Some(sg) = self.cache.get_mut(sg_id, &self.subgraph_index) {
            let new_id = sg.add_vertex(labels.clone());
            // The subgraph's internal ID may differ; track mapping
            self.vertex_index.insert(vid, sg_id, (sg.vertices.len() - 1) as u32);
            // Ensure we use the global ID
            if let Some(v) = sg.get_vertex_mut(new_id) {
                v.id = vid;
            }
        } else {
            // Create the subgraph if it doesn't exist yet
            let mut sg = Subgraph::new(sg_id);
            let _ = sg.add_vertex(labels.clone());
            self.vertex_index.insert(vid, sg_id, 0);
            self.cache.insert(sg);
        }

        // 4. Update label index
        for label in &labels {
            self.label_index.add(label.clone(), vid);
        }

        Ok(vid)
    }

    /// Add a vertex with properties.
    pub fn add_vertex_with_props(
        &mut self,
        labels: Vec<String>,
        properties: std::collections::HashMap<String, PropertyValue>,
    ) -> Result<VertexId, GraphError> {
        let vid = self.add_vertex(labels)?;
        if let Some(v) = self.get_vertex_mut(vid) {
            v.properties = properties;
        }
        Ok(vid)
    }

    /// Get a vertex by ID.
    pub fn get_vertex(&mut self, id: VertexId) -> Option<Vertex> {
        let (sg_id, offset) = self.vertex_index.lookup(id)?;
        let sg = self.cache.get_mut(sg_id, &self.subgraph_index)?;
        sg.vertices.get(offset as usize).cloned()
    }

    /// Get a mutable reference to a vertex (for updating properties).
    pub fn get_vertex_mut(&mut self, id: VertexId) -> Option<&mut Vertex> {
        let (sg_id, offset) = self.vertex_index.lookup(id)?;
        let sg = self.cache.get_mut(sg_id, &self.subgraph_index)?;
        sg.vertices.get_mut(offset as usize)
    }

    /// Remove a vertex and all its incident edges.
    pub fn remove_vertex(&mut self, id: VertexId) -> Result<(), GraphError> {
        let (sg_id, _offset) = self
            .vertex_index
            .lookup(id)
            .ok_or(GraphError::VertexNotFound(id))?;

        // Log to WAL
        self.redo_log
            .append(&RedoOperation::RemoveVertex(RemoveVertexPayload {
                subgraph_id: sg_id,
                vertex_id: id,
            }))
            .map_err(|_| GraphError::VertexNotFound(id))?;

        // Remove all edges referencing this vertex
        self.remove_incident_edges(id, sg_id);

        // Apply to cache
        if let Some(sg) = self.cache.get_mut(sg_id, &self.subgraph_index) {
            sg.remove_vertex(id, true);
        }

        self.vertex_index.remove(id);
        // Note: label index cleanup is best-effort
        Ok(())
    }

    // ─── Edge Operations ───────────────────────────────────────

    /// Add an edge between two vertices.
    pub fn add_edge(
        &mut self,
        label: String,
        source: VertexId,
        target: VertexId,
    ) -> Result<u64, GraphError> {
        if self.vertex_index.lookup(source).is_none() {
            return Err(GraphError::VertexNotFound(source));
        }
        if self.vertex_index.lookup(target).is_none() {
            return Err(GraphError::VertexNotFound(target));
        }

        let eid = self.next_edge_id.fetch_add(1, Ordering::SeqCst);
        let (src_sg, _) = self.vertex_index.lookup(source).unwrap();
        let (tgt_sg, _) = self.vertex_index.lookup(target).unwrap();

        if src_sg == tgt_sg {
            // Internal edge — both ends in same subgraph
            self.redo_log
                .append(&RedoOperation::AddEdge(AddEdgePayload {
                    subgraph_id: src_sg,
                    edge_id: eid,
                    label: label.clone(),
                    source,
                    target,
                }))
                .map_err(|_| GraphError::EdgeNotFound(eid))?;

            self.edge_index.insert(eid, src_sg);
            if let Some(sg) = self.cache.get_mut(src_sg, &self.subgraph_index) {
                let _ = sg.add_edge(label, source, target);
            }
        } else {
            // Cross-subgraph edge
            self.redo_log
                .append(&RedoOperation::AddCrossEdge(AddCrossEdgePayload {
                    subgraph_id: src_sg,
                    edge_id: eid,
                    label: label.clone(),
                    source,
                    target_subgraph: tgt_sg,
                    target_vertex: target,
                }))
                .map_err(|_| GraphError::EdgeNotFound(eid))?;

            self.edge_index.insert(eid, src_sg);
            if let Some(sg) = self.cache.get_mut(src_sg, &self.subgraph_index) {
                sg.add_cross_edge(eid, label, source, tgt_sg, target);
            }
        }

        Ok(eid)
    }

    // ─── Traversal ─────────────────────────────────────────────

    /// Outgoing neighbors of a vertex (same-subgraph + cross-edges).
    pub fn out_neighbors(&mut self, vertex_id: VertexId, edge_label: Option<&str>) -> Vec<VertexId> {
        let (sg_id, _) = match self.vertex_index.lookup(vertex_id) {
            Some(x) => x,
            None => return vec![],
        };
        let sg = match self.cache.get_mut(sg_id, &self.subgraph_index) {
            Some(sg) => sg,
            None => return vec![],
        };

        let mut result: Vec<VertexId> = sg
            .outgoing_edges(vertex_id)
            .iter()
            .filter(|e| edge_label.map_or(true, |l| e.label == *l))
            .map(|e| e.target)
            .collect();
        result.extend(
            sg.outgoing_cross_edges(vertex_id)
                .iter()
                .filter(|e| edge_label.map_or(true, |l| e.edge_label == *l))
                .map(|e| e.target_vertex),
        );
        result
    }

    /// Incoming neighbors of a vertex (same-subgraph only, for now).
    pub fn in_neighbors(&mut self, vertex_id: VertexId, edge_label: Option<&str>) -> Vec<VertexId> {
        let (sg_id, _) = match self.vertex_index.lookup(vertex_id) {
            Some(x) => x,
            None => return vec![],
        };
        let sg = match self.cache.get_mut(sg_id, &self.subgraph_index) {
            Some(sg) => sg,
            None => return vec![],
        };

        sg.incoming_edges(vertex_id)
            .iter()
            .filter(|e| edge_label.map_or(true, |l| e.label == *l))
            .map(|e| e.source)
            .collect()
    }

    /// Both incoming and outgoing neighbors.
    pub fn both_neighbors(&mut self, vertex_id: VertexId, edge_label: Option<&str>) -> Vec<VertexId> {
        let mut neighbors = self.out_neighbors(vertex_id, edge_label);
        neighbors.extend(self.in_neighbors(vertex_id, edge_label));
        neighbors
    }

    // ─── Subgraph Management ───────────────────────────────────

    /// Find a subgraph to place a new vertex in, creating one if needed.
    fn find_or_create_subgraph(&mut self, _labels: &[String]) -> SubgraphId {
        let current: Vec<(SubgraphId, usize)> = self
            .subgraph_index
            .iter()
            .map(|(&id, meta)| (id, meta.vertex_count as usize))
            .collect();

        // Try to find a subgraph with room
        for &(sg_id, count) in &current {
            if count < self.partition_config.max_vertices_per_subgraph {
                return sg_id;
            }
        }

        // All full or none exist — create new
        self.create_new_subgraph()
    }

    /// Create a new empty subgraph.
    fn create_new_subgraph(&mut self) -> SubgraphId {
        let id = (self.subgraph_index.len() as u64) + 1;
        let sg = Subgraph::new(id);
        self.cache.insert(sg);
        self.subgraph_index
            .insert(super::index::SubgraphMeta {
                id,
                file_path: subgraph_file_path(&self.data_dir, id),
                vertex_count: 0,
                edge_count: 0,
                cross_edge_count: 0,
                size_bytes: 0,
                checksum: 0,
            });
        id
    }

    /// Remove incident edges for a vertex being deleted.
    fn remove_incident_edges(&mut self, vertex_id: VertexId, sg_id: SubgraphId) {
        // Best-effort: remove edges referencing this vertex from its subgraph
        if let Some(sg) = self.cache.get_mut(sg_id, &self.subgraph_index) {
            sg.edges.retain(|e| e.source != vertex_id && e.target != vertex_id);
            sg.cross_edges.retain(|e| e.source_vertex != vertex_id);
        }
    }

    // ─── Replay (for WAL recovery) ─────────────────────────────

    fn replay_add_vertex(&mut self, payload: &AddVertexPayload) {
        let sg_id = payload.subgraph_id;
        if let Some(sg) = self.cache.get_mut(sg_id, &self.subgraph_index) {
            let new_id = sg.add_vertex(payload.labels.clone());
            self.vertex_index
                .insert(payload.vertex_id, sg_id, (sg.vertices.len() - 1) as u32);
            if let Some(v) = sg.get_vertex_mut(new_id) {
                v.id = payload.vertex_id;
            }
        }
    }

    fn replay_add_edge(&mut self, payload: &AddEdgePayload) {
        if let Some(sg) = self.cache.get_mut(payload.subgraph_id, &self.subgraph_index) {
            let _ = sg.add_edge(payload.label.clone(), payload.source, payload.target);
        }
    }

    fn replay_remove_vertex(&mut self, payload: &RemoveVertexPayload) {
        if let Some(sg) = self.cache.get_mut(payload.subgraph_id, &self.subgraph_index) {
            sg.remove_vertex(payload.vertex_id, true);
        }
        self.vertex_index.remove(payload.vertex_id);
    }

    fn replay_remove_edge(&mut self, payload: &RemoveEdgePayload) {
        if let Some(sg) = self.cache.get_mut(payload.subgraph_id, &self.subgraph_index) {
            sg.edges.retain(|e| e.id != payload.edge_id);
            sg.cross_edges.retain(|e| e.edge_id != payload.edge_id);
        }
    }

    // ─── Checkpoint ────────────────────────────────────────────

    /// Flush dirty subgraphs to disk and write a checkpoint to the WAL.
    pub fn checkpoint(&mut self) -> std::io::Result<()> {
        let written = self.cache.flush_all();
        if written > 0 {
            log::debug!("Checkpoint: flushed {} dirty subgraphs", written);
        }
        // Update subgraph metadata
        for id in self.cache.cached_ids() {
            if let Some(sg) = self.cache.get_mut(id, &self.subgraph_index) {
                if let Some(meta) = self.subgraph_index.get_mut(id) {
                    meta.vertex_count = sg.vertices.len() as u32;
                    meta.edge_count = sg.edges.len() as u32;
                    meta.cross_edge_count = sg.cross_edges.len() as u32;
                }
            }
        }
        // Save index bundle
        self.save_index_bundle()?;
        // Rebuild edge index
        self.rebuild_edge_index();
        // Write checkpoint marker to WAL
        self.redo_log.checkpoint()?;
        Ok(())
    }

    /// Save the index bundle to disk.
    fn save_index_bundle(&self) -> std::io::Result<()> {
        let bundle = IndexBundle {
            vertex_index: self.vertex_index.clone(),
            subgraph_index: self.subgraph_index.clone(),
            label_index: self.label_index.clone(),
            global_next_vertex_id: self.next_vertex_id.load(Ordering::SeqCst),
            global_next_edge_id: self.next_edge_id.load(Ordering::SeqCst),
        };
        let bytes = bundle.to_bytes();
        std::fs::write(self.data_dir.join("index.bundle"), &bytes)?;
        Ok(())
    }

    // ─── Stats ─────────────────────────────────────────────────

    pub fn vertex_count(&self) -> usize {
        self.vertex_index.len()
    }

    pub fn edge_count(&self) -> usize {
        // Approximate: sum of all subgraph edge counts
        self.subgraph_index
            .iter()
            .map(|(_, m)| m.edge_count as usize)
            .sum()
    }

    pub fn subgraph_count(&self) -> usize {
        self.subgraph_index.len()
    }

    pub fn cache_stats(&self) -> &super::subgraph_cache::CacheStats {
        self.cache.stats()
    }

    // ─── Extended Read API (for Gremlin compatibility) ────────

    /// Get an edge by ID. Scans subgraphs (O(subgraphs)) — caches result in edge_index.
    pub fn get_edge(&mut self, id: u64) -> Option<Edge> {
        // Fast path: check index
        if let Some(&sg_id) = self.edge_index.get(&id) {
            if let Some(sg) = self.cache.get_mut(sg_id, &self.subgraph_index) {
                if let Some(e) = sg.edges.iter().find(|e| e.id == id) {
                    return Some(e.clone());
                }
                // Also check cross_edges
                if let Some(ce) = sg.cross_edges.iter().find(|e| e.edge_id == id) {
                    let mut edge = Edge::new(ce.edge_id, ce.edge_label.clone(), ce.source_vertex, ce.target_vertex);
                    edge.properties = ce.properties.clone();
                    return Some(edge);
                }
            }
        }
        // Slow path: scan all subgraphs
        let sg_ids: Vec<SubgraphId> = self.subgraph_index.iter().map(|(&id, _)| id).collect();
        for sg_id in sg_ids {
            if let Some(sg) = self.cache.get_mut(sg_id, &self.subgraph_index) {
                if let Some(e) = sg.edges.iter().find(|e| e.id == id) {
                    self.edge_index.insert(id, sg_id);
                    return Some(e.clone());
                }
                if let Some(ce) = sg.cross_edges.iter().find(|e| e.edge_id == id) {
                    self.edge_index.insert(id, sg_id);
                    let mut edge = Edge::new(ce.edge_id, ce.edge_label.clone(), ce.source_vertex, ce.target_vertex);
                    edge.properties = ce.properties.clone();
                    return Some(edge);
                }
            }
        }
        None
    }

    /// Get an edge by ID, including deleted (same as get_edge for now).
    pub fn get_edge_including_deleted(&mut self, id: u64) -> Option<Edge> {
        self.get_edge(id)
    }

    /// Get a vertex including deleted (checks _is_deleted flag).
    pub fn get_vertex_including_deleted(&mut self, id: VertexId) -> Option<Vertex> {
        self.get_vertex(id)
    }

    /// Remove an edge by ID.
    pub fn remove_edge(&mut self, id: u64) -> Result<(), GraphError> {
        // Find which subgraph
        let sg_id = if let Some(&sg_id) = self.edge_index.get(&id) {
            sg_id
        } else {
            return Err(GraphError::EdgeNotFound(id));
        };
        // Log to WAL
        self.redo_log
            .append(&RedoOperation::RemoveEdge(RemoveEdgePayload {
                subgraph_id: sg_id,
                edge_id: id,
            }))
            .map_err(|_| GraphError::EdgeNotFound(id))?;
        // Remove from cache
        if let Some(sg) = self.cache.get_mut(sg_id, &self.subgraph_index) {
            sg.edges.retain(|e| e.id != id);
            sg.cross_edges.retain(|e| e.edge_id != id);
        }
        self.edge_index.remove(&id);
        Ok(())
    }

    /// Soft-delete an edge (mark as deleted in-place).
    pub fn soft_delete_edge(&mut self, id: u64, _force: bool) -> Result<(), GraphError> {
        // Mark the edge's _is_deleted flag
        if let Some(e) = self.get_edge(id) {
            let _ = self.update_edge(id, Some(&e.label), e.properties);
        }
        Ok(())
    }

    /// Add an edge with properties.
    pub fn add_edge_with_props(
        &mut self,
        label: String,
        source: VertexId,
        target: VertexId,
        properties: HashMap<String, PropertyValue>,
    ) -> Result<u64, GraphError> {
        let eid = self.add_edge(label, source, target)?;
        self.update_edge(eid, None, properties);
        Ok(eid)
    }

    /// Update an edge's label and properties in-place.
    pub fn update_edge(&mut self, id: u64, label: Option<&str>, properties: HashMap<String, PropertyValue>) -> bool {
        if let Some(&sg_id) = self.edge_index.get(&id) {
            if let Some(sg) = self.cache.get_mut(sg_id, &self.subgraph_index) {
                if let Some(e) = sg.edges.iter_mut().find(|e| e.id == id) {
                    if let Some(l) = label { e.label = l.to_string(); }
                    if !properties.is_empty() { e.properties = properties; }
                    return true;
                }
            }
        }
        false
    }

    /// All vertex IDs (from vertex_index, always in memory).
    pub fn vertex_ids(&self) -> Vec<VertexId> {
        self.vertex_index.iter().map(|(&id, _)| id).collect()
    }

    /// All edge IDs (loads all subgraphs).
    pub fn edge_ids(&mut self) -> Vec<u64> {
        let mut ids = Vec::new();
        let sg_ids: Vec<SubgraphId> = self.subgraph_index.iter().map(|(&id, _)| id).collect();
        for sg_id in sg_ids {
            if let Some(sg) = self.cache.get_mut(sg_id, &self.subgraph_index) {
                for e in &sg.edges {
                    ids.push(e.id);
                }
                for ce in &sg.cross_edges {
                    ids.push(ce.edge_id);
                }
            }
        }
        ids
    }

    /// All edges in the graph.
    pub fn all_edges(&mut self) -> Vec<Edge> {
        let mut edges = Vec::new();
        let sg_ids: Vec<SubgraphId> = self.subgraph_index.iter().map(|(&id, _)| id).collect();
        for sg_id in sg_ids {
            if let Some(sg) = self.cache.get_mut(sg_id, &self.subgraph_index) {
                for e in &sg.edges {
                    edges.push(e.clone());
                }
                for ce in &sg.cross_edges {
                    let mut edge = Edge::new(ce.edge_id, ce.edge_label.clone(), ce.source_vertex, ce.target_vertex);
                    edge.properties = ce.properties.clone();
                    edges.push(edge);
                }
            }
        }
        edges
    }

    /// Outgoing edges from a vertex.
    pub fn outgoing_edges(&mut self, vertex_id: VertexId, edge_label: Option<&str>) -> Vec<Edge> {
        let (sg_id, _) = match self.vertex_index.lookup(vertex_id) {
            Some(x) => x,
            None => return vec![],
        };
        let sg = match self.cache.get_mut(sg_id, &self.subgraph_index) {
            Some(sg) => sg,
            None => return vec![],
        };
        let result: Vec<Edge> = sg
            .outgoing_edges(vertex_id)
            .iter()
            .filter(|e| edge_label.map_or(true, |l| e.label == *l))
            .map(|e| {
                let mut edge = Edge::new(e.id, e.label.clone(), e.source, e.target);
                edge.properties = e.properties.clone();
                edge
            })
            .collect();
        result
    }

    /// Incoming edges to a vertex (same-subgraph only).
    pub fn incoming_edges(&mut self, vertex_id: VertexId, edge_label: Option<&str>) -> Vec<Edge> {
        let (sg_id, _) = match self.vertex_index.lookup(vertex_id) {
            Some(x) => x,
            None => return vec![],
        };
        let sg = match self.cache.get_mut(sg_id, &self.subgraph_index) {
            Some(sg) => sg,
            None => return vec![],
        };
        sg.incoming_edges(vertex_id)
            .iter()
            .filter(|e| edge_label.map_or(true, |l| e.label == *l))
            .map(|e| {
                let mut edge = Edge::new(e.id, e.label.clone(), e.source, e.target);
                edge.properties = e.properties.clone();
                edge
            })
            .collect()
    }

    /// Build edge index from current subgraph data.
    pub fn rebuild_edge_index(&mut self) {
        self.edge_index.clear();
        let sg_ids: Vec<SubgraphId> = self.subgraph_index.iter().map(|(&id, _)| id).collect();
        for sg_id in sg_ids {
            if let Some(sg) = self.cache.get(sg_id, &self.subgraph_index) {
                for e in &sg.edges {
                    self.edge_index.insert(e.id, sg_id);
                }
                for ce in &sg.cross_edges {
                    self.edge_index.insert(ce.edge_id, sg_id);
                }
            }
        }
    }
}

/// Build the file path for a subgraph.
fn subgraph_file_path(data_dir: &Path, id: SubgraphId) -> PathBuf {
    let filename = format!("{:08x}.bin", id);
    data_dir.join("subgraph").join(filename)
}



#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_disk_graph() -> (DiskGraph, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let graph = DiskGraph::open(dir.path()).unwrap();
        (graph, dir)
    }

    #[test]
    fn test_open_empty() {
        let (_graph, _dir) = make_disk_graph();
    }

    #[test]
    fn test_add_and_get_vertex() {
        let (mut graph, _dir) = make_disk_graph();
        let vid = graph.add_vertex(vec!["person".to_string()]).unwrap();
        let v = graph.get_vertex(vid).unwrap();
        assert_eq!(v.labels, vec!["person"]);
    }

    #[test]
    fn test_add_edge() {
        let (mut graph, _dir) = make_disk_graph();
        let v1 = graph.add_vertex(vec!["person".to_string()]).unwrap();
        let v2 = graph.add_vertex(vec!["company".to_string()]).unwrap();
        let eid = graph.add_edge("works_at".to_string(), v1, v2).unwrap();
        assert!(eid > 0);

        let neighbors = graph.out_neighbors(v1, None);
        assert_eq!(neighbors, vec![v2]);
    }

    #[test]
    fn test_out_neighbors_with_filter() {
        let (mut graph, _dir) = make_disk_graph();
        let v1 = graph.add_vertex(vec!["person".to_string()]).unwrap();
        let v2 = graph.add_vertex(vec!["company".to_string()]).unwrap();
        let v3 = graph.add_vertex(vec!["person".to_string()]).unwrap();
        graph.add_edge("works_at".to_string(), v1, v2).unwrap();
        graph.add_edge("knows".to_string(), v1, v3).unwrap();

        let works = graph.out_neighbors(v1, Some("works_at"));
        assert_eq!(works, vec![v2]);

        let knows = graph.out_neighbors(v1, Some("knows"));
        assert_eq!(knows, vec![v3]);

        let all = graph.out_neighbors(v1, None);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_remove_vertex() {
        let (mut graph, _dir) = make_disk_graph();
        let v1 = graph.add_vertex(vec!["person".to_string()]).unwrap();
        let v2 = graph.add_vertex(vec!["company".to_string()]).unwrap();
        graph.add_edge("works_at".to_string(), v1, v2).unwrap();
        assert_eq!(graph.vertex_count(), 2);
        graph.remove_vertex(v1).unwrap();
        assert_eq!(graph.vertex_count(), 1);
    }

    #[test]
    fn test_checkpoint_and_recover() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let vid;
        {
            let mut graph = DiskGraph::open(&path).unwrap();
            vid = graph.add_vertex(vec!["person".to_string()]).unwrap();
            graph.add_vertex(vec!["company".to_string()]).unwrap();
            graph.checkpoint().unwrap();
        }

        // Re-open — should recover
        {
            let mut graph = DiskGraph::open(&path).unwrap();
            assert_eq!(graph.vertex_count(), 2);
            assert!(graph.get_vertex(vid).is_some());
        }
    }

    #[test]
    fn test_remove_vertex_after_checkpoint() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let vid;
        {
            let mut graph = DiskGraph::open(&path).unwrap();
            vid = graph.add_vertex(vec!["person".to_string()]).unwrap();
            graph.checkpoint().unwrap();
            graph.remove_vertex(vid).unwrap();
        }

        {
            // WAL should replay the removal
            let mut graph = DiskGraph::open(&path).unwrap();
            assert_eq!(graph.vertex_count(), 0);
        }
    }
}
