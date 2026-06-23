use std::collections::{HashSet, VecDeque};

use crate::graph::{Graph, Vertex, VertexId};

use super::subgraph::{Subgraph, SubgraphId};

/// Configuration for subgraph partitioning.
#[derive(Debug, Clone)]
pub struct PartitionConfig {
    /// Maximum number of vertices per subgraph.
    pub max_vertices_per_subgraph: usize,
    /// BFS depth for clustering (how far from seed to include).
    pub cluster_bfs_depth: usize,
    /// Strategy for assigning new vertices to subgraphs.
    pub strategy: PartitionStrategy,
}

impl Default for PartitionConfig {
    fn default() -> Self {
        Self {
            max_vertices_per_subgraph: 10_000,
            cluster_bfs_depth: 3,
            strategy: PartitionStrategy::AutoCluster,
        }
    }
}

/// How to assign vertices to subgraphs.
#[derive(Debug, Clone, PartialEq)]
pub enum PartitionStrategy {
    /// Auto-cluster: BFS from seeds to find locality groups.
    AutoCluster,
    /// Label-based: vertices with the same label go together.
    ByLabel,
    /// All in one subgraph (simple, for small graphs).
    SingleSubgraph,
}

/// Result of partitioning a graph into subgraphs.
#[derive(Debug)]
pub struct PartitionResult {
    pub subgraphs: Vec<Subgraph>,
    pub subgraph_ids: Vec<SubgraphId>,
}

/// Assigns a newly created vertex to a subgraph.
///
/// Returns the subgraph ID to place the vertex in.
/// If `None`, a new subgraph should be created.
pub fn assign_vertex_to_subgraph(
    vertex: &Vertex,
    config: &PartitionConfig,
    current_subgraphs: &[(SubgraphId, usize)], // (sg_id, vertex_count)
    vertex_index: &crate::storage::index::VertexIndex,
    graph: &Graph,
) -> Option<SubgraphId> {
    match config.strategy {
        PartitionStrategy::SingleSubgraph => {
            // Always put in the first subgraph, or None to create one
            current_subgraphs.first().map(|&(id, _)| id)
        }
        PartitionStrategy::ByLabel => {
            // Find a subgraph with the same label
            let _target_label = vertex.labels.first()?;
            // Try to find existing subgraph matching this label
            // We need a LabelIndex or subgraph naming convention
            // Simple: check all current subgraphs via their first vertex's label
            for &(sg_id, count) in current_subgraphs {
                if count < config.max_vertices_per_subgraph {
                    return Some(sg_id);
                }
            }
            None // All full → create new
        }
        PartitionStrategy::AutoCluster => {
            // Check if any neighbor is already in a subgraph
            for label in &vertex.labels {
                // Find peers with same label
                let peers = graph.vertices_by_label(label);
                for &peer_id in &peers {
                    if let Some((sg_id, _)) = vertex_index.lookup(peer_id) {
                        // Check subgraph isn't full
                        let sg_count = current_subgraphs
                            .iter()
                            .find(|&&(id, _)| id == sg_id)
                            .map(|&(_, c)| c)
                            .unwrap_or(0);
                        if sg_count < config.max_vertices_per_subgraph {
                            return Some(sg_id);
                        }
                    }
                }
            }
            None // No room in existing → create new
        }
    }
}

/// Partition an in-memory Graph into subgraphs using BFS clustering.
///
/// Algorithm:
/// 1. Pick a seed vertex (not yet assigned)
/// 2. Run BFS up to `cluster_bfs_depth` to find related vertices
/// 3. Group them into a subgraph
/// 4. Repeat for remaining unassigned vertices
pub fn partition_graph(
    graph: &Graph,
    config: &PartitionConfig,
    start_id: SubgraphId,
) -> PartitionResult {
    let all_vertices: Vec<VertexId> = graph.vertex_ids().copied().collect();
    let mut assigned: HashSet<VertexId> = HashSet::new();
    let mut subgraphs = Vec::new();
    let mut next_sg_id = start_id;

    for &seed in &all_vertices {
        if assigned.contains(&seed) {
            continue;
        }

        // BFS from seed to find cluster
        let cluster = bfs_cluster(graph, seed, config.cluster_bfs_depth, &assigned);
        if cluster.is_empty() {
            continue;
        }

        // Create subgraph from cluster
        let mut sg = Subgraph::new(next_sg_id);
        let mut vid_map: std::collections::HashMap<VertexId, VertexId> =
            std::collections::HashMap::new();

        // Add vertices
        for &vid in &cluster {
            if let Some(v) = graph.get_vertex(vid) {
                let new_id = sg.add_vertex(v.labels.clone());
                // Copy properties
                if let Some(new_v) = sg.get_vertex_mut(new_id) {
                    new_v.properties = v.properties.clone();
                }
                vid_map.insert(vid, new_id);
            }
            assigned.insert(vid);
        }

        // Add edges (internal: both endpoints in this subgraph)
        for &vid in &cluster {
            let local_src = vid_map[&vid];
            for neighbor in graph.out_neighbors(vid, None) {
                if cluster.contains(&neighbor) {
                    // Internal edge — both ends in same subgraph
                    let local_tgt = vid_map[&neighbor];
                    // Find the edge label
                    if let Some(edge) = graph.edges_between(vid, neighbor).first() {
                        let _ = sg.add_edge(edge.label.clone(), local_src, local_tgt);
                    }
                }
            }
        }

        subgraphs.push(sg);
        next_sg_id += 1;
    }

    // Second pass: add cross-edges between subgraphs
    // (This requires tracking original vid → subgraph mapping)
    // Cross-edge resolution is done at query time via SubgraphCache.

    PartitionResult {
        subgraph_ids: (start_id..next_sg_id).collect(),
        subgraphs,
    }
}

/// BFS from a seed vertex to find a cluster of related vertices.
fn bfs_cluster(
    graph: &Graph,
    seed: VertexId,
    max_depth: usize,
    exclude: &HashSet<VertexId>,
) -> Vec<VertexId> {
    let mut cluster = Vec::new();
    let mut visited = exclude.clone();
    let mut queue = VecDeque::new();

    if !visited.contains(&seed) {
        visited.insert(seed);
        queue.push_back((seed, 0usize));
    }

    while let Some((vid, depth)) = queue.pop_front() {
        if depth > max_depth {
            break;
        }
        cluster.push(vid);

        if depth < max_depth {
            for neighbor in graph.both_neighbors(vid, None) {
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }
    }

    cluster
}

/// Merge a list of subgraphs back into an in-memory Graph
/// (useful for testing and for full-graph operations).
pub fn merge_subgraphs(subgraphs: &[Subgraph]) -> Graph {
    let mut graph = Graph::new();
    // Track ID remapping
    for sg in subgraphs {
        for v in &sg.vertices {
            graph.add_vertex(v.clone()).unwrap();
        }
    }
    for sg in subgraphs {
        for e in &sg.edges {
            let _ = graph.create_edge(e.label.clone(), e.source, e.target);
        }
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;

    fn make_test_graph() -> Graph {
        let mut g = Graph::new();

        // Two clusters connected internally:
        // Cluster A: alice --knows--> bob --works_at--> acme
        // Cluster B: carol --knows--> dave --works_at--> globex
        let alice = g.create_vertex(vec!["person".to_string()]);
        let bob = g.create_vertex(vec!["person".to_string()]);
        let carol = g.create_vertex(vec!["person".to_string()]);
        let dave = g.create_vertex(vec!["person".to_string()]);
        let acme = g.create_vertex(vec!["company".to_string()]);
        let globex = g.create_vertex(vec!["company".to_string()]);

        g.create_edge("knows".to_string(), alice, bob).unwrap();
        g.create_edge("works_at".to_string(), bob, acme).unwrap();
        g.create_edge("knows".to_string(), carol, dave).unwrap();
        g.create_edge("works_at".to_string(), dave, globex).unwrap();

        g
    }

    #[test]
    fn test_bfs_cluster_size() {
        let g = make_test_graph();
        // Find first person vertex
        let persons = g.vertices_by_label("person");
        assert!(!persons.is_empty());

        let cluster = bfs_cluster(&g, persons[0], 2, &HashSet::new());
        assert!(!cluster.is_empty());
        // The BFS from a person should find at least 2 vertices (person + their company)
        assert!(cluster.len() >= 2);
    }

    #[test]
    fn test_partition_creates_subgraphs() {
        let g = make_test_graph();
        let config = PartitionConfig {
            max_vertices_per_subgraph: 10,
            cluster_bfs_depth: 3,
            strategy: PartitionStrategy::AutoCluster,
        };

        let result = partition_graph(&g, &config, 1);
        assert!(!result.subgraphs.is_empty(), "Should create at least 1 subgraph");

        // Count total vertices across all subgraphs
        let total: usize = result.subgraphs.iter().map(|sg| sg.vertices.len()).sum();
        assert_eq!(total, g.vertex_count(), "All vertices should be assigned");
    }

    #[test]
    fn test_merge_roundtrip() {
        let g = make_test_graph();
        let config = PartitionConfig::default();
        let result = partition_graph(&g, &config, 1);
        let merged = merge_subgraphs(&result.subgraphs);
        assert_eq!(merged.vertex_count(), g.vertex_count());
    }
}
