use std::collections::{HashSet, VecDeque};

use super::graph::Graph;
use super::vertex::VertexId;

/// A step in a traversal path — records the current vertex, depth, and the
/// edge that led to it (None for the starting vertices).
#[derive(Debug, Clone)]
pub struct TraversalStep {
    pub vertex: VertexId,
    pub depth: usize,
    pub edge_id: Option<super::edge::EdgeId>,
}

// ─── Breadth-First Search ─────────────────────────────────────────

/// Lazy BFS iterator over a graph.
pub struct Bfs<'a> {
    graph: &'a Graph,
    frontier: VecDeque<TraversalStep>,
    visited: HashSet<VertexId>,
    edge_label: Option<&'a str>,
    max_depth: usize,
}

impl<'a> Bfs<'a> {
    pub fn new(graph: &'a Graph, start: VertexId) -> Self {
        let mut frontier = VecDeque::new();
        frontier.push_back(TraversalStep {
            vertex: start,
            depth: 0,
            edge_id: None,
        });
        let mut visited = HashSet::new();
        visited.insert(start);
        Self {
            graph,
            frontier,
            visited,
            edge_label: None,
            max_depth: usize::MAX,
        }
    }

    /// Start from multiple root vertices.
    pub fn from_many(graph: &'a Graph, starts: Vec<VertexId>) -> Self {
        let mut frontier = VecDeque::new();
        let mut visited = HashSet::new();
        for v in starts {
            if visited.insert(v) {
                frontier.push_back(TraversalStep {
                    vertex: v,
                    depth: 0,
                    edge_id: None,
                });
            }
        }
        Self {
            graph,
            frontier,
            visited,
            edge_label: None,
            max_depth: usize::MAX,
        }
    }

    /// Filter edges by label.
    pub fn with_edge_label(mut self, label: &'a str) -> Self {
        self.edge_label = Some(label);
        self
    }

    /// Limit traversal depth.
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }
}

impl<'a> Iterator for Bfs<'a> {
    type Item = TraversalStep;

    fn next(&mut self) -> Option<Self::Item> {
        let step = self.frontier.pop_front()?;
        if step.depth < self.max_depth {
            for neighbor in self
                .graph
                .out_neighbors(step.vertex, self.edge_label)
            {
                if self.visited.insert(neighbor) {
                    // Find the edge id — grab the first matching one
                    let edges = self.graph.outgoing_edges(step.vertex);
                    let eid = edges.iter().find_map(|eid| {
                        let e = self.graph.get_edge(*eid)?;
                        if e.target == neighbor {
                            if let Some(label) = self.edge_label {
                                if e.label != label {
                                    return None;
                                }
                            }
                            Some(*eid)
                        } else {
                            None
                        }
                    });
                    self.frontier.push_back(TraversalStep {
                        vertex: neighbor,
                        depth: step.depth + 1,
                        edge_id: eid,
                    });
                }
            }
        }
        Some(step)
    }
}

// ─── Depth-First Search ───────────────────────────────────────────

/// Lazy DFS iterator over a graph.
pub struct Dfs<'a> {
    graph: &'a Graph,
    stack: Vec<TraversalStep>,
    visited: HashSet<VertexId>,
    edge_label: Option<&'a str>,
    max_depth: usize,
}

impl<'a> Dfs<'a> {
    pub fn new(graph: &'a Graph, start: VertexId) -> Self {
        let mut visited = HashSet::new();
        visited.insert(start);
        Self {
            graph,
            stack: vec![TraversalStep {
                vertex: start,
                depth: 0,
                edge_id: None,
            }],
            visited,
            edge_label: None,
            max_depth: usize::MAX,
        }
    }

    pub fn from_many(graph: &'a Graph, starts: Vec<VertexId>) -> Self {
        let mut visited = HashSet::new();
        let mut stack = Vec::new();
        for v in starts {
            if visited.insert(v) {
                stack.push(TraversalStep {
                    vertex: v,
                    depth: 0,
                    edge_id: None,
                });
            }
        }
        Self {
            graph,
            stack,
            visited,
            edge_label: None,
            max_depth: usize::MAX,
        }
    }

    pub fn with_edge_label(mut self, label: &'a str) -> Self {
        self.edge_label = Some(label);
        self
    }

    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }
}

impl<'a> Iterator for Dfs<'a> {
    type Item = TraversalStep;

    fn next(&mut self) -> Option<Self::Item> {
        let step = self.stack.pop()?;
        if step.depth < self.max_depth {
            for neighbor in self
                .graph
                .out_neighbors(step.vertex, self.edge_label)
            {
                if self.visited.insert(neighbor) {
                    let edges = self.graph.outgoing_edges(step.vertex);
                    let eid = edges.iter().find_map(|eid| {
                        let e = self.graph.get_edge(*eid)?;
                        if e.target == neighbor {
                            if let Some(label) = self.edge_label {
                                if e.label != label {
                                    return None;
                                }
                            }
                            Some(*eid)
                        } else {
                            None
                        }
                    });
                    self.stack.push(TraversalStep {
                        vertex: neighbor,
                        depth: step.depth + 1,
                        edge_id: eid,
                    });
                }
            }
        }
        Some(step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;

    fn setup_graph() -> Graph {
        let mut g = Graph::new();
        let a = g.create_vertex(vec!["node".to_string()]); // 1
        let b = g.create_vertex(vec!["node".to_string()]); // 2
        let c = g.create_vertex(vec!["node".to_string()]); // 3
        let d = g.create_vertex(vec!["node".to_string()]); // 4

        g.create_edge("connects".to_string(), a, b).unwrap();
        g.create_edge("connects".to_string(), b, c).unwrap();
        g.create_edge("connects".to_string(), c, d).unwrap();
        g.create_edge("connects".to_string(), a, c).unwrap();
        g
    }

    #[test]
    fn test_bfs_basic() {
        let g = setup_graph();
        let results: Vec<_> = Bfs::new(&g, 1).collect();
        // BFS from 1 should reach: 1 (depth 0), then 2 and 3 (depth 1), then 4 (depth 2)
        assert_eq!(results.len(), 4, "BFS should visit all 4 nodes");
        // First result is the start node
        assert_eq!(results[0].vertex, 1);
        assert_eq!(results[0].depth, 0);
    }

    #[test]
    fn test_bfs_depth_limit() {
        let g = setup_graph();
        let results: Vec<_> = Bfs::new(&g, 1).with_max_depth(1).collect();
        // Depth 1: node 1 (depth 0), nodes 2 and 3 (depth 1)
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_dfs_basic() {
        let g = setup_graph();
        let results: Vec<_> = Dfs::new(&g, 1).collect();
        assert_eq!(results.len(), 4, "DFS should visit all 4 nodes");
        assert_eq!(results[0].vertex, 1);
    }

    #[test]
    fn test_bfs_from_many() {
        let mut g = Graph::new();
        let a = g.create_vertex(vec!["x".to_string()]);
        let b = g.create_vertex(vec!["x".to_string()]);
        let c = g.create_vertex(vec!["x".to_string()]);
        g.create_edge("e".to_string(), a, b).unwrap();
        g.create_edge("e".to_string(), b, c).unwrap();

        // Start from both a and b
        let results: Vec<_> = Bfs::from_many(&g, vec![a, b]).collect();
        // a(1) at depth 0, b already visited as start, c reached from b
        assert_eq!(results.len(), 3);
    }
}
