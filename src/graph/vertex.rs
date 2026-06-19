use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a graph vertex.
pub type VertexId = u64;

/// A single value stored as a vertex or edge property.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PropertyValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    List(Vec<PropertyValue>),
    Null,
}

impl From<&str> for PropertyValue {
    fn from(s: &str) -> Self {
        PropertyValue::String(s.to_string())
    }
}

impl From<String> for PropertyValue {
    fn from(s: String) -> Self {
        PropertyValue::String(s)
    }
}

impl From<i64> for PropertyValue {
    fn from(n: i64) -> Self {
        PropertyValue::Integer(n)
    }
}

impl From<f64> for PropertyValue {
    fn from(n: f64) -> Self {
        PropertyValue::Float(n)
    }
}

impl From<bool> for PropertyValue {
    fn from(b: bool) -> Self {
        PropertyValue::Boolean(b)
    }
}

/// A vertex (node) in the knowledge graph.
///
/// Each vertex has a unique ID, one or more labels (typically one for the entity type),
/// and an arbitrary set of key-value properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vertex {
    pub id: VertexId,
    pub labels: Vec<String>,
    pub properties: HashMap<String, PropertyValue>,
}

impl Vertex {
    pub fn new(id: VertexId, labels: Vec<String>) -> Self {
        Self {
            id,
            labels,
            properties: HashMap::new(),
        }
    }

    pub fn with_properties(mut self, props: HashMap<String, PropertyValue>) -> Self {
        self.properties = props;
        self
    }

    pub fn has_label(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l == label)
    }

    pub fn get_property(&self, key: &str) -> Option<&PropertyValue> {
        self.properties.get(key)
    }
}
