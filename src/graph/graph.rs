use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::edge::{Edge, EdgeId};
use super::vertex::{Vertex, VertexId};

/// Error types for graph operations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GraphError {
    #[error("vertex {0} not found")]
    VertexNotFound(VertexId),
    #[error("edge {0} not found")]
    EdgeNotFound(EdgeId),
    #[error("vertex {0} already exists")]
    VertexAlreadyExists(VertexId),
}

/// The core knowledge graph — an in-memory directed graph stored as
/// dual adjacency lists (forward + backward) for O(1) traversals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Graph {
    vertices: HashMap<VertexId, Vertex>,
    edges: HashMap<EdgeId, Edge>,

    /// Outgoing edges: source vertex → list of outgoing edge IDs
    forward: HashMap<VertexId, Vec<EdgeId>>,
    /// Incoming edges: target vertex → list of incoming edge IDs
    backward: HashMap<VertexId, Vec<EdgeId>>,

    /// Maps vertex label → set of vertex IDs (for label-based lookups)
    vertex_labels: HashMap<String, HashSet<VertexId>>,

    next_vertex_id: VertexId,
    next_edge_id: EdgeId,
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

impl Graph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            vertices: HashMap::new(),
            edges: HashMap::new(),
            forward: HashMap::new(),
            backward: HashMap::new(),
            vertex_labels: HashMap::new(),
            next_vertex_id: 1,
            next_edge_id: 1,
        }
    }

    // ─── Vertex Operations ─────────────────────────────────────────

    /// Add a vertex to the graph. Returns error if ID already exists.
    pub fn add_vertex(&mut self, vertex: Vertex) -> Result<VertexId, GraphError> {
        let id = vertex.id;
        if self.vertices.contains_key(&id) {
            return Err(GraphError::VertexAlreadyExists(id));
        }
        for label in &vertex.labels {
            self.vertex_labels
                .entry(label.clone())
                .or_default()
                .insert(id);
        }
        self.vertices.insert(id, vertex);
        Ok(id)
    }

    /// Create a new vertex with auto-assigned ID.
    pub fn create_vertex(&mut self, labels: Vec<String>) -> VertexId {
        let id = self.next_vertex_id;
        self.next_vertex_id += 1;
        let vertex = Vertex::new(id, labels.clone());
        for label in labels {
            self.vertex_labels
                .entry(label)
                .or_default()
                .insert(id);
        }
        self.vertices.insert(id, vertex);
        id
    }

    /// Remove a vertex and all its incident edges.
    pub fn remove_vertex(&mut self, id: VertexId) -> Result<(), GraphError> {
        let vertex = self
            .vertices
            .remove(&id)
            .ok_or(GraphError::VertexNotFound(id))?;

        // Remove label index entries
        for label in &vertex.labels {
            if let Some(set) = self.vertex_labels.get_mut(label) {
                set.remove(&id);
            }
        }

        // Remove all incident edges
        let out_edges = self.forward.remove(&id).unwrap_or_default();
        let in_edges = self.backward.remove(&id).unwrap_or_default();

        for eid in out_edges.iter().chain(in_edges.iter()) {
            if let Some(edge) = self.edges.remove(eid) {
                // Remove edge from the other end's adjacency lists
                if let Some(fwd) = self.forward.get_mut(&edge.source) {
                    fwd.retain(|e| e != eid);
                }
                if let Some(bwd) = self.backward.get_mut(&edge.target) {
                    bwd.retain(|e| e != eid);
                }
            }
        }

        Ok(())
    }

    /// Get a vertex by ID.
    pub fn get_vertex(&self, id: VertexId) -> Option<&Vertex> {
        self.vertices.get(&id)
    }

    /// Get a mutable reference to a vertex.
    pub fn get_vertex_mut(&mut self, id: VertexId) -> Option<&mut Vertex> {
        self.vertices.get_mut(&id)
    }

    /// Return all vertex IDs in the graph.
    pub fn vertex_ids(&self) -> impl Iterator<Item = &VertexId> {
        self.vertices.keys()
    }

    /// Return the total number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Find all vertices with a given label.
    pub fn vertices_by_label(&self, label: &str) -> Vec<VertexId> {
        self.vertex_labels
            .get(label)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    // ─── Edge Operations ───────────────────────────────────────────

    /// Add an edge. Automatically updates adjacency lists.
    pub fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, GraphError> {
        let id = edge.id;
        let src = edge.source;
        let tgt = edge.target;

        if !self.vertices.contains_key(&src) {
            return Err(GraphError::VertexNotFound(src));
        }
        if !self.vertices.contains_key(&tgt) {
            return Err(GraphError::VertexNotFound(tgt));
        }

        self.forward.entry(src).or_default().push(id);
        self.backward.entry(tgt).or_default().push(id);
        self.edges.insert(id, edge);
        Ok(id)
    }

    /// Create an edge with auto-assigned ID.
    pub fn create_edge(
        &mut self,
        label: String,
        source: VertexId,
        target: VertexId,
    ) -> Result<EdgeId, GraphError> {
        if !self.vertices.contains_key(&source) {
            return Err(GraphError::VertexNotFound(source));
        }
        if !self.vertices.contains_key(&target) {
            return Err(GraphError::VertexNotFound(target));
        }

        let id = self.next_edge_id;
        self.next_edge_id += 1;
        let edge = Edge::new(id, label, source, target);
        self.forward.entry(source).or_default().push(id);
        self.backward.entry(target).or_default().push(id);
        self.edges.insert(id, edge);
        Ok(id)
    }

    /// Remove an edge by ID.
    pub fn remove_edge(&mut self, id: EdgeId) -> Result<(), GraphError> {
        let edge = self.edges.remove(&id).ok_or(GraphError::EdgeNotFound(id))?;

        if let Some(fwd) = self.forward.get_mut(&edge.source) {
            fwd.retain(|e| *e != id);
        }
        if let Some(bwd) = self.backward.get_mut(&edge.target) {
            bwd.retain(|e| *e != id);
        }

        Ok(())
    }

    /// Get an edge by ID.
    pub fn get_edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(&id)
    }

    /// Get a mutable reference to an edge by ID.
    pub fn get_edge_mut(&mut self, id: EdgeId) -> Option<&mut Edge> {
        self.edges.get_mut(&id)
    }

    /// Get all edges.
    pub fn all_edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.values()
    }

    /// Return the total number of edges.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    // ─── Traversal Helpers ─────────────────────────────────────────

    /// Get outgoing edge IDs from a vertex.
    pub fn outgoing_edges(&self, vertex_id: VertexId) -> Vec<EdgeId> {
        self.forward
            .get(&vertex_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get incoming edge IDs to a vertex.
    pub fn incoming_edges(&self, vertex_id: VertexId) -> Vec<EdgeId> {
        self.backward
            .get(&vertex_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get both incoming and outgoing edge IDs for a vertex.
    pub fn incident_edges(&self, vertex_id: VertexId) -> Vec<EdgeId> {
        let mut edges = self.outgoing_edges(vertex_id);
        edges.extend(self.incoming_edges(vertex_id));
        edges
    }

    /// Get neighbor vertex IDs reachable via outgoing edges (with optional label filter).
    pub fn out_neighbors(&self, vertex_id: VertexId, edge_label: Option<&str>) -> Vec<VertexId> {
        self.outgoing_edges(vertex_id)
            .iter()
            .filter_map(|eid| {
                let edge = self.edges.get(eid)?;
                if let Some(label) = edge_label {
                    if edge.label != label {
                        return None;
                    }
                }
                Some(edge.target)
            })
            .collect()
    }

    /// Get neighbor vertex IDs reachable via incoming edges (with optional label filter).
    pub fn in_neighbors(&self, vertex_id: VertexId, edge_label: Option<&str>) -> Vec<VertexId> {
        self.incoming_edges(vertex_id)
            .iter()
            .filter_map(|eid| {
                let edge = self.edges.get(eid)?;
                if let Some(label) = edge_label {
                    if edge.label != label {
                        return None;
                    }
                }
                Some(edge.source)
            })
            .collect()
    }

    /// Get all neighbor vertex IDs (both in and out) with optional edge label filter.
    pub fn both_neighbors(&self, vertex_id: VertexId, edge_label: Option<&str>) -> Vec<VertexId> {
        let mut neighbors = self.out_neighbors(vertex_id, edge_label);
        neighbors.extend(self.in_neighbors(vertex_id, edge_label));
        neighbors
    }

    /// Find all edges between two vertices.
    pub fn edges_between(&self, src: VertexId, tgt: VertexId) -> Vec<&Edge> {
        self.outgoing_edges(src)
            .iter()
            .filter_map(|eid| {
                let edge = self.edges.get(eid)?;
                if edge.target == tgt { Some(edge) } else { None }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_get_vertex() {
        let mut g = Graph::new();
        let v = Vertex::new(1, vec!["person".to_string()]);
        g.add_vertex(v).unwrap();
        assert_eq!(g.vertex_count(), 1);
        assert!(g.get_vertex(1).is_some());
        assert!(g.get_vertex(99).is_none());
    }

    #[test]
    fn test_auto_id_vertex() {
        let mut g = Graph::new();
        let id1 = g.create_vertex(vec!["person".to_string()]);
        let id2 = g.create_vertex(vec!["company".to_string()]);
        assert_ne!(id1, id2);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_add_duplicate_vertex_fails() {
        let mut g = Graph::new();
        let v = Vertex::new(1, vec!["person".to_string()]);
        g.add_vertex(v).unwrap();
        let v2 = Vertex::new(1, vec!["person".to_string()]);
        assert_eq!(
            g.add_vertex(v2),
            Err(GraphError::VertexAlreadyExists(1))
        );
    }

    #[test]
    fn test_create_edge() {
        let mut g = Graph::new();
        let v1 = g.create_vertex(vec!["person".to_string()]);
        let v2 = g.create_vertex(vec!["company".to_string()]);
        let eid = g
            .create_edge("works_at".to_string(), v1, v2)
            .unwrap();
        assert_eq!(g.edge_count(), 1);
        assert!(g.get_edge(eid).is_some());
    }

    #[test]
    fn test_edge_to_nonexistent_vertex_fails() {
        let mut g = Graph::new();
        let v1 = g.create_vertex(vec!["person".to_string()]);
        let result = g.create_edge("knows".to_string(), v1, 999);
        assert_eq!(result, Err(GraphError::VertexNotFound(999)));
    }

    #[test]
    fn test_out_neighbors() {
        let mut g = Graph::new();
        let alice = g.create_vertex(vec!["person".to_string()]);
        let bob = g.create_vertex(vec!["person".to_string()]);
        let acme = g.create_vertex(vec!["company".to_string()]);
        g.create_edge("works_at".to_string(), alice, acme).unwrap();
        g.create_edge("knows".to_string(), alice, bob).unwrap();

        let all_out = g.out_neighbors(alice, None);
        assert_eq!(all_out.len(), 2);

        let filtered = g.out_neighbors(alice, Some("works_at"));
        assert_eq!(filtered, vec![acme]);
    }

    #[test]
    fn test_in_neighbors() {
        let mut g = Graph::new();
        let alice = g.create_vertex(vec!["person".to_string()]);
        let bob = g.create_vertex(vec!["person".to_string()]);
        g.create_edge("knows".to_string(), alice, bob).unwrap();
        g.create_edge("knows".to_string(), bob, alice).unwrap();

        let in_n = g.in_neighbors(alice, None);
        assert_eq!(in_n, vec![bob]);
    }

    #[test]
    fn test_remove_vertex_cascades() {
        let mut g = Graph::new();
        let v1 = g.create_vertex(vec!["a".to_string()]);
        let v2 = g.create_vertex(vec!["b".to_string()]);
        g.create_edge("rel".to_string(), v1, v2).unwrap();
        assert_eq!(g.edge_count(), 1);
        g.remove_vertex(v1).unwrap();
        assert_eq!(g.vertex_count(), 1);
        assert_eq!(g.edge_count(), 0);
    }

    #[test]
    fn test_vertices_by_label() {
        let mut g = Graph::new();
        g.create_vertex(vec!["person".to_string()]);
        g.create_vertex(vec!["person".to_string()]);
        g.create_vertex(vec!["company".to_string()]);
        assert_eq!(g.vertices_by_label("person").len(), 2);
        assert_eq!(g.vertices_by_label("company").len(), 1);
        assert_eq!(g.vertices_by_label("animal").len(), 0);
    }

    #[test]
    fn test_both_neighbors() {
        let mut g = Graph::new();
        let a = g.create_vertex(vec!["node".to_string()]);
        let b = g.create_vertex(vec!["node".to_string()]);
        let c = g.create_vertex(vec!["node".to_string()]);
        g.create_edge("a_to_b".to_string(), a, b).unwrap();
        g.create_edge("c_to_a".to_string(), c, a).unwrap();
        let both = g.both_neighbors(a, None);
        assert_eq!(both.len(), 2);
        assert!(both.contains(&b));
        assert!(both.contains(&c));
    }
}
