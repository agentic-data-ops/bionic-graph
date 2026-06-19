use serde::{Deserialize, Serialize};

/// A single step in a Gremlin traversal pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "step")]
pub enum TraversalStep {
    /// Start traversal by finding vertices via the neural index.
    /// Custom extension to Gremlin.
    #[serde(rename = "neuralSearch")]
    NeuralSearch {
        keywords: Vec<String>,
    },

    /// Start with all vertices, or specific vertices by ID.
    #[serde(rename = "V")]
    V {
        #[serde(default)]
        ids: Vec<u64>,
    },

    /// Start with all edges, or specific edges by ID.
    #[serde(rename = "E")]
    E {
        #[serde(default)]
        ids: Vec<u64>,
    },

    /// Filter vertices/edges by property key-value pair.
    #[serde(rename = "has")]
    Has {
        key: String,
        value: serde_json::Value,
    },

    /// Filter by label.
    #[serde(rename = "hasLabel")]
    HasLabel {
        labels: Vec<String>,
    },

    /// Traverse outgoing edges. If label is Some, filter by edge label.
    /// depth: traverse N levels (default 1). Uses BFS internally.
    #[serde(rename = "out")]
    Out {
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        depth: Option<usize>,
    },

    /// Traverse incoming edges. depth: traverse N levels (default 1).
    #[serde(rename = "in")]
    In {
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        depth: Option<usize>,
    },

    /// Traverse both incoming and outgoing edges. depth: traverse N levels (default 1).
    #[serde(rename = "both")]
    Both {
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        depth: Option<usize>,
    },

    /// Get values of a specific property.
    #[serde(rename = "values")]
    Values {
        key: String,
    },

    /// Limit the number of results.
    #[serde(rename = "limit")]
    Limit {
        count: usize,
    },

    /// Count the results (returns a single number).
    #[serde(rename = "count")]
    Count,

    /// Deduplicate results by vertex/edge ID.
    #[serde(rename = "dedup")]
    Dedup,

    /// Filter by substring match on a property (case-insensitive).
    /// Example: {"step": "hasText", "key": "name", "pattern": "Ali"}
    #[serde(rename = "hasText")]
    HasText {
        key: String,
        pattern: String,
    },

    /// Repeat a sub-pipeline N times.
    /// Example: {"step": "repeat", "times": 3, "steps": [
    ///   {"step": "out", "label": "knows"}
    /// ]}
    #[serde(rename = "repeat")]
    Repeat {
        times: usize,
        steps: Vec<TraversalStep>,
    },
}

/// A full Gremlin query consisting of a pipeline of traversal steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GremlinQuery {
    pub steps: Vec<TraversalStep>,
}

impl GremlinQuery {
    pub fn new(steps: Vec<TraversalStep>) -> Self {
        Self { steps }
    }
}

/// An element in the traversal result stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TraversalResult {
    VertexResult(VertexResult),
    EdgeResult(EdgeResult),
    ValueResult(serde_json::Value),
    CountResult(u64),
}

/// Vertex data returned by the Gremlin API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VertexResult {
    #[serde(rename = "type")]
    pub element_type: String,
    pub id: u64,
    pub labels: Vec<String>,
    pub properties: std::collections::HashMap<String, serde_json::Value>,
}

/// Edge data returned by the Gremlin API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeResult {
    #[serde(rename = "type")]
    pub element_type: String,
    pub id: u64,
    pub label: String,
    pub source: u64,
    pub target: u64,
    pub properties: std::collections::HashMap<String, serde_json::Value>,
}

/// Response wrapper for the Gremlin query endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub success: bool,
    pub data: Vec<TraversalResult>,
    pub error: Option<String>,
    pub ticks_used: Option<usize>,
    pub neurons_fired: Option<Vec<u64>>,
}
