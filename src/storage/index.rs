use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::graph::VertexId;

use super::subgraph::SubgraphId;

/// Metadata about a subgraph file on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubgraphMeta {
    pub id: SubgraphId,
    pub file_path: PathBuf,
    pub vertex_count: u32,
    pub edge_count: u32,
    pub cross_edge_count: u32,
    pub size_bytes: u64,
    pub checksum: u32,
}

/// Maps vertex IDs to their containing subgraph.
///
/// Memory: ~16 bytes per vertex. At 10M vertices ≈ 160 MB.
/// This is the one index that must fit in RAM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VertexIndex {
    /// vertex_id → (subgraph_id, offset_within_subgraph_vertices)
    /// offset is the index in Subgraph.vertices[] for fast direct access.
    inner: HashMap<VertexId, (SubgraphId, u32)>,
}

impl VertexIndex {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Register a vertex in a subgraph.
    pub fn insert(&mut self, vertex_id: VertexId, subgraph_id: SubgraphId, offset: u32) {
        self.inner.insert(vertex_id, (subgraph_id, offset));
    }

    /// Look up which subgraph a vertex belongs to.
    pub fn lookup(&self, vertex_id: VertexId) -> Option<(SubgraphId, u32)> {
        self.inner.get(&vertex_id).copied()
    }

    /// Remove a vertex from the index.
    pub fn remove(&mut self, vertex_id: VertexId) {
        self.inner.remove(&vertex_id);
    }

    /// Number of indexed vertices.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = (&VertexId, &(SubgraphId, u32))> {
        self.inner.iter()
    }

    /// Serialize to bytes for disk persistence.
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("VertexIndex serialization failed")
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        bincode::deserialize(data).ok()
    }
}

impl Default for VertexIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Index of all subgraphs: id → metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubgraphIndex {
    inner: HashMap<SubgraphId, SubgraphMeta>,
}

impl SubgraphIndex {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn insert(&mut self, meta: SubgraphMeta) {
        self.inner.insert(meta.id, meta);
    }

    pub fn get(&self, id: SubgraphId) -> Option<&SubgraphMeta> {
        self.inner.get(&id)
    }

    pub fn get_mut(&mut self, id: SubgraphId) -> Option<&mut SubgraphMeta> {
        self.inner.get_mut(&id)
    }

    pub fn remove(&mut self, id: SubgraphId) {
        self.inner.remove(&id);
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&SubgraphId, &SubgraphMeta)> {
        self.inner.iter()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("SubgraphIndex serialization failed")
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        bincode::deserialize(data).ok()
    }
}

impl Default for SubgraphIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps labels to lists of vertex IDs — for fast `hasLabel` lookups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelIndex {
    inner: HashMap<String, Vec<VertexId>>,
}

impl LabelIndex {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Add a vertex to a label group.
    pub fn add(&mut self, label: impl Into<String>, vertex_id: VertexId) {
        self.inner.entry(label.into()).or_default().push(vertex_id);
    }

    /// Remove a vertex from a label group.
    pub fn remove(&mut self, label: &str, vertex_id: VertexId) {
        if let Some(ids) = self.inner.get_mut(label) {
            ids.retain(|&id| id != vertex_id);
        }
    }

    /// Get all vertices with a given label.
    pub fn get(&self, label: &str) -> Vec<VertexId> {
        self.inner.get(label).cloned().unwrap_or_default()
    }

    /// Number of unique labels.
    pub fn label_count(&self) -> usize {
        self.inner.len()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("LabelIndex serialization failed")
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        bincode::deserialize(data).ok()
    }
}

impl Default for LabelIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Top-level index bundle — saved/loaded as one unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexBundle {
    pub vertex_index: VertexIndex,
    pub subgraph_index: SubgraphIndex,
    pub label_index: LabelIndex,
    /// Monotonic ID counters (global, not per-subgraph).
    pub global_next_vertex_id: VertexId,
    pub global_next_edge_id: u64,
}

impl IndexBundle {
    pub fn new() -> Self {
        Self {
            vertex_index: VertexIndex::new(),
            subgraph_index: SubgraphIndex::new(),
            label_index: LabelIndex::new(),
            global_next_vertex_id: 1,
            global_next_edge_id: 1,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("IndexBundle serialization failed")
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        bincode::deserialize(data).ok()
    }
}

impl Default for IndexBundle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vertex_index() {
        let mut idx = VertexIndex::new();
        idx.insert(1, 0, 0);
        idx.insert(2, 0, 1);
        idx.insert(100, 3, 5);

        assert_eq!(idx.lookup(1), Some((0, 0)));
        assert_eq!(idx.lookup(100), Some((3, 5)));
        assert_eq!(idx.lookup(999), None);

        idx.remove(1);
        assert_eq!(idx.lookup(1), None);
        assert_eq!(idx.len(), 2);
    }

    #[test]
    fn test_subgraph_index() {
        let mut idx = SubgraphIndex::new();
        idx.insert(SubgraphMeta {
            id: 1,
            file_path: PathBuf::from("data/subgraph/00000001.bin"),
            vertex_count: 100,
            edge_count: 200,
            cross_edge_count: 5,
            size_bytes: 4096,
            checksum: 0xDEADBEEF,
        });

        let meta = idx.get(1).unwrap();
        assert_eq!(meta.vertex_count, 100);
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn test_label_index() {
        let mut idx = LabelIndex::new();
        idx.add("person", 1);
        idx.add("person", 2);
        idx.add("company", 3);

        assert_eq!(idx.get("person").len(), 2);
        assert_eq!(idx.get("company"), vec![3]);
        assert_eq!(idx.get("animal").len(), 0);

        idx.remove("person", 1);
        assert_eq!(idx.get("person"), vec![2]);
    }

    #[test]
    fn test_index_bundle_roundtrip() {
        let mut bundle = IndexBundle::new();
        bundle.vertex_index.insert(1, 0, 0);
        bundle.vertex_index.insert(2, 0, 1);
        bundle.label_index.add("person", 1);
        bundle.label_index.add("person", 2);

        let bytes = bundle.to_bytes();
        let loaded = IndexBundle::from_bytes(&bytes).unwrap();
        assert_eq!(loaded.vertex_index.len(), 2);
        assert_eq!(loaded.label_index.get("person").len(), 2);
        assert_eq!(loaded.label_index.label_count(), 1);
    }
}
