use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::vertex::{PropertyValue, VertexId};

/// Unique identifier for a graph edge.
pub type EdgeId = u64;

/// A directed edge connecting two vertices in the knowledge graph.
///
/// Each edge has a unique ID, a label describing the relationship type,
/// a source vertex, a target vertex, and optional properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub label: String,
    pub source: VertexId,
    pub target: VertexId,
    pub properties: HashMap<String, PropertyValue>,
}

impl Edge {
    pub fn new(id: EdgeId, label: String, source: VertexId, target: VertexId) -> Self {
        Self {
            id,
            label,
            source,
            target,
            properties: HashMap::new(),
        }
    }

    pub fn with_properties(mut self, props: HashMap<String, PropertyValue>) -> Self {
        self.properties = props;
        self
    }

    pub fn has_label(&self, label: &str) -> bool {
        self.label == label
    }

    pub fn get_property(&self, key: &str) -> Option<&PropertyValue> {
        self.properties.get(key)
    }
}
