use std::path::{Path, PathBuf};

use crate::graph::Graph;
use crate::storage::disk_graph::DiskGraph;

use super::StoreError;

// ─── Legacy: Whole-Graph Save/Load (for in-memory Graph) ───────

/// Save the entire in-memory graph to disk as a single binary blob.
pub fn save_graph(graph: &Graph, path: impl AsRef<Path>) -> Result<(), StoreError> {
    let encoded = bincode::serialize(graph).map_err(|e| StoreError::Serialize {
        source: e,
        description: "graph".to_string(),
    })?;
    std::fs::write(path.as_ref(), &encoded).map_err(|e| StoreError::Io {
        source: e,
        description: format!("writing graph to {}", path.as_ref().display()),
    })?;
    Ok(())
}

/// Load the entire in-memory graph from a binary blob on disk.
pub fn load_graph(path: impl AsRef<Path>) -> Result<Graph, StoreError> {
    let data = std::fs::read(path.as_ref()).map_err(|e| StoreError::Io {
        source: e,
        description: format!("reading graph from {}", path.as_ref().display()),
    })?;
    let graph: Graph = bincode::deserialize(&data).map_err(|e| StoreError::Deserialize {
        source: e,
        description: "graph".to_string(),
    })?;
    Ok(graph)
}

// ─── New: Subgraph-based Save/Load (for DiskGraph) ─────────────

/// Initialize a DiskGraph at the given data directory.
pub fn open_disk_graph(data_dir: impl Into<PathBuf>) -> Result<DiskGraph, StoreError> {
    DiskGraph::open(data_dir).map_err(|e| StoreError::Io {
        source: e,
        description: "opening disk graph".to_string(),
    })
}

/// Force a checkpoint on a DiskGraph (flush dirty subgraphs + log checkpoint).
pub fn checkpoint(graph: &mut DiskGraph) -> Result<(), StoreError> {
    graph.checkpoint().map_err(|e| StoreError::Io {
        source: e,
        description: "checkpoint".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Vertex;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_graph_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.bin");

        let mut graph = Graph::new();
        let v1 = graph.create_vertex(vec!["person".to_string()]);
        let v2 = graph.create_vertex(vec!["company".to_string()]);
        graph.create_edge("works_at".to_string(), v1, v2).unwrap();

        save_graph(&graph, &path).unwrap();
        let loaded = load_graph(&path).unwrap();
        assert_eq!(loaded.vertex_count(), graph.vertex_count());
        assert_eq!(loaded.edge_count(), graph.edge_count());
    }

    #[test]
    fn test_disk_graph_open() {
        let dir = tempdir().unwrap();
        let mut graph = open_disk_graph(dir.path()).unwrap();
        graph.add_vertex(vec!["person".to_string()]).unwrap();
        checkpoint(&mut graph).unwrap();
        assert_eq!(graph.vertex_count(), 1);
    }
}
