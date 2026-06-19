use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::graph::{Edge, PropertyValue, Vertex, VertexId};

/// Unique identifier for a subgraph.
pub type SubgraphId = u64;

/// Maximum size of a single subgraph's serialized data (before compression, if any).
/// When a subgraph exceeds this, the partitioner should split it.
pub const MAX_SUBGRAPH_BYTES: u64 = 64 * 1024 * 1024; // 64 MB

/// Magic bytes at the start of every subgraph file: b"TGSUB"
pub const SUBGRAPH_MAGIC: [u8; 5] = [0x54, 0x47, 0x53, 0x55, 0x42];
pub const SUBGRAPH_VERSION: u32 = 1;

/// A reference to an edge that crosses subgraph boundaries.
///
/// The edge's source vertex is in this subgraph, but the target vertex
/// lives in a different subgraph. This record lets us follow the edge
/// without loading the target's subgraph until needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossEdgeRef {
    pub edge_id: u64,
    pub edge_label: String,
    pub source_vertex: VertexId,
    pub target_subgraph: SubgraphId,
    pub target_vertex: VertexId,
    pub properties: HashMap<String, PropertyValue>,
}

/// A single subgraph — the unit of disk I/O and caching.
///
/// Each subgraph holds a cluster of related vertices and the edges between them.
/// Edges whose target is in another subgraph are stored as `cross_edges` for
/// lazy cross-subgraph traversal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subgraph {
    pub id: SubgraphId,
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    /// Edges that point to vertices in other subgraphs.
    pub cross_edges: Vec<CrossEdgeRef>,
    /// Monotonic counters for generating new IDs within this subgraph.
    pub next_vertex_id: VertexId,
    pub next_edge_id: u64,
}

impl Subgraph {
    pub fn new(id: SubgraphId) -> Self {
        Self {
            id,
            vertices: Vec::new(),
            edges: Vec::new(),
            cross_edges: Vec::new(),
            next_vertex_id: id * 1_000_000 + 1,
            next_edge_id: id * 1_000_000 + 1,
        }
    }

    /// Add a vertex, assigning it an auto-generated ID.
    pub fn add_vertex(&mut self, labels: Vec<String>) -> VertexId {
        let id = self.next_vertex_id;
        self.next_vertex_id += 1;
        self.vertices.push(Vertex::new(id, labels));
        id
    }

    /// Check if a vertex exists in this subgraph.
    pub fn has_vertex(&self, id: VertexId) -> bool {
        self.vertices.iter().any(|v| v.id == id)
    }

    /// Get a vertex by ID.
    pub fn get_vertex(&self, id: VertexId) -> Option<&Vertex> {
        self.vertices.iter().find(|v| v.id == id)
    }

    /// Get a mutable reference to a vertex.
    pub fn get_vertex_mut(&mut self, id: VertexId) -> Option<&mut Vertex> {
        self.vertices.iter_mut().find(|v| v.id == id)
    }

    /// Remove a vertex by ID (also removes its edges).
    pub fn remove_vertex(&mut self, id: VertexId) {
        self.vertices.retain(|v| v.id != id);
        self.edges.retain(|e| e.source != id && e.target != id);
        self.cross_edges.retain(|e| e.source_vertex != id);
    }

    /// Add an edge between two vertices that are both in this subgraph.
    pub fn add_edge(&mut self, label: String, source: VertexId, target: VertexId) -> Result<u64, String> {
        if !self.has_vertex(source) {
            return Err(format!("source vertex {} not in subgraph {}", source, self.id));
        }
        if !self.has_vertex(target) {
            return Err(format!("target vertex {} not in subgraph {}", target, self.id));
        }
        let id = self.next_edge_id;
        self.next_edge_id += 1;
        self.edges.push(Edge::new(id, label, source, target));
        Ok(id)
    }

    /// Add a cross-subgraph edge reference.
    pub fn add_cross_edge(
        &mut self,
        edge_id: u64,
        label: String,
        source: VertexId,
        target_sg: SubgraphId,
        target_vid: VertexId,
    ) {
        self.cross_edges.push(CrossEdgeRef {
            edge_id,
            edge_label: label,
            source_vertex: source,
            target_subgraph: target_sg,
            target_vertex: target_vid,
            properties: HashMap::new(),
        });
    }

    /// Get outgoing (internal) edges from a vertex.
    pub fn outgoing_edges(&self, vertex_id: VertexId) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.source == vertex_id).collect()
    }

    /// Get incoming (internal) edges to a vertex.
    pub fn incoming_edges(&self, vertex_id: VertexId) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.target == vertex_id).collect()
    }

    /// Get outgoing cross-edges from a vertex.
    pub fn outgoing_cross_edges(&self, vertex_id: VertexId) -> Vec<&CrossEdgeRef> {
        self.cross_edges.iter().filter(|e| e.source_vertex == vertex_id).collect()
    }

    /// Total number of items in this subgraph.
    pub fn item_count(&self) -> usize {
        self.vertices.len() + self.edges.len() + self.cross_edges.len()
    }

    /// Calculate the estimated serialized size.
    pub fn estimated_size(&self) -> u64 {
        // Rough estimate: each vertex ~200 bytes, each edge ~100 bytes
        (self.vertices.len() as u64 * 200)
            + (self.edges.len() as u64 * 100)
            + (self.cross_edges.len() as u64 * 64)
    }

    // ─── Serialization ────────────────────────────────────────

    /// Magic bytes + version + subgraph_id + crc32 + bincode payload.
    pub fn to_bytes(&self) -> Vec<u8> {
        let payload = bincode::serialize(self).expect("Subgraph serialization failed");
        let checksum = crc32fast::hash(&payload);

        let mut buf = Vec::with_capacity(5 + 4 + 8 + 4 + payload.len());
        buf.extend_from_slice(&SUBGRAPH_MAGIC);
        buf.extend_from_slice(&SUBGRAPH_VERSION.to_le_bytes());
        buf.extend_from_slice(&self.id.to_le_bytes());
        buf.extend_from_slice(&checksum.to_le_bytes());
        buf.extend_from_slice(&payload);
        buf
    }

    /// Deserialize from bytes. Returns None if magic/version/checksum mismatch.
    pub fn from_bytes(data: &[u8]) -> Option<(Self, u32)> {
        if data.len() < 5 + 4 + 8 + 4 {
            // Magic(5) + version(4) + id(8) + crc32(4) = 21 bytes header minimum
            return None;
        }
        let mut off = 0;
        if &data[off..off + 5] != SUBGRAPH_MAGIC {
            return None;
        }
        off += 5;
        let version = u32::from_le_bytes(data[off..off + 4].try_into().ok()?);
        off += 4;
        let _sg_id = u64::from_le_bytes(data[off..off + 8].try_into().ok()?);
        off += 8;
        let stored_crc = u32::from_le_bytes(data[off..off + 4].try_into().ok()?);
        off += 4;

        let payload = &data[off..];
        let actual_crc = crc32fast::hash(payload);
        if actual_crc != stored_crc {
            return None;
        }

        let subgraph: Subgraph = bincode::deserialize(payload).ok()?;
        Some((subgraph, version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subgraph_new() {
        let sg = Subgraph::new(1);
        assert_eq!(sg.id, 1);
        assert!(sg.vertices.is_empty());
        assert!(sg.edges.is_empty());
    }

    #[test]
    fn test_add_vertex_and_edge() {
        let mut sg = Subgraph::new(1);
        let v1 = sg.add_vertex(vec!["person".to_string()]);
        let v2 = sg.add_vertex(vec!["company".to_string()]);
        assert_eq!(sg.vertices.len(), 2);

        let eid = sg.add_edge("works_at".to_string(), v1, v2).unwrap();
        assert_eq!(sg.edges.len(), 1);
        assert_eq!(sg.outgoing_edges(v1).len(), 1);
        assert_eq!(sg.incoming_edges(v2).len(), 1);
    }

    #[test]
    fn test_cross_edge() {
        let mut sg = Subgraph::new(1);
        let v1 = sg.add_vertex(vec!["person".to_string()]);
        sg.add_cross_edge(100, "knows".to_string(), v1, 2, 999);
        assert_eq!(sg.cross_edges.len(), 1);
        assert_eq!(sg.outgoing_cross_edges(v1).len(), 1);
    }

    #[test]
    fn test_remove_vertex_cascades() {
        let mut sg = Subgraph::new(1);
        let v1 = sg.add_vertex(vec!["a".to_string()]);
        let v2 = sg.add_vertex(vec!["b".to_string()]);
        sg.add_edge("rel".to_string(), v1, v2).unwrap();
        sg.add_cross_edge(50, "x".to_string(), v1, 2, 999);
        sg.remove_vertex(v1);
        assert_eq!(sg.vertices.len(), 1);
        assert_eq!(sg.edges.len(), 0);
        assert_eq!(sg.cross_edges.len(), 0);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut sg = Subgraph::new(42);
        let v1 = sg.add_vertex(vec!["person".to_string()]);
        let v2 = sg.add_vertex(vec!["company".to_string()]);
        sg.add_edge("works_at".to_string(), v1, v2).unwrap();

        let bytes = sg.to_bytes();
        let (loaded, version) = Subgraph::from_bytes(&bytes).unwrap();
        assert_eq!(loaded.id, 42);
        assert_eq!(version, 1);
        assert_eq!(loaded.vertices.len(), 2);
        assert_eq!(loaded.edges.len(), 1);
        assert!(loaded.has_vertex(v1));
        assert!(loaded.has_vertex(v2));
    }

    #[test]
    fn test_serialization_corrupted() {
        let sg = Subgraph::new(1);
        let mut bytes = sg.to_bytes();
        // Corrupt a byte in the payload
        let last = bytes.len() - 1;
        bytes[last] = bytes[last].wrapping_add(1);
        assert!(Subgraph::from_bytes(&bytes).is_none());
    }
}
