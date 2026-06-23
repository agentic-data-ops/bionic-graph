use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::vertex::{PropertyValue, VertexId, VersionRecord};

/// Unique identifier for a graph edge.
pub type EdgeId = u64;

/// A directed edge connecting two vertices in the knowledge graph, with MVCC support.
///
/// Each edge carries a version number, update timestamp, soft-delete flag,
/// and a history of previous property states for time-travel queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub label: String,
    pub source: VertexId,
    pub target: VertexId,
    pub properties: HashMap<String, PropertyValue>,

    // ─── Version fields ──────────────────────────────────────────
    pub _version: u64,
    pub _updated_at: i64,
    pub _is_deleted: bool,
    pub _history: Vec<VersionRecord>,
}

impl Edge {
    pub fn new(id: EdgeId, label: String, source: VertexId, target: VertexId) -> Self {
        Self {
            id,
            label,
            source,
            target,
            properties: HashMap::new(),
            _version: 1,
            _updated_at: super::vertex::now_micros(),
            _is_deleted: false,
            _history: Vec::new(),
        }
    }

    pub fn with_properties(mut self, props: HashMap<String, PropertyValue>) -> Self {
        self.properties = props;
        self
    }

    /// Update properties — bumps version, saves snapshot if `record_history` is true.
    pub fn update_properties(&mut self, new_props: HashMap<String, PropertyValue>, record_history: bool) {
        if record_history {
            let now = super::vertex::now_micros();
            self._history.push(VersionRecord {
                version: self._version,
                updated_at: self._updated_at,
                name: String::new(),
                keywords: Vec::new(),
                labels: vec![self.label.clone()],
                properties: self.properties.clone(),
            });
            self._version += 1;
            self._updated_at = now;
        }
        self.properties = new_props;
    }

    /// Soft-delete this edge.
    pub fn soft_delete(&mut self, record_history: bool) {
        if !self._is_deleted {
            if record_history {
                let now = super::vertex::now_micros();
                self._history.push(VersionRecord {
                    version: self._version,
                    updated_at: self._updated_at,
                    name: String::new(),
                    keywords: Vec::new(),
                    labels: vec![self.label.clone()],
                    properties: self.properties.clone(),
                });
                self._version += 1;
                self._updated_at = now;
            }
            self._is_deleted = true;
        }
    }

    /// Compact history: remove records before `before_timestamp` and return them.
    pub fn compact(&mut self, before_timestamp: i64) -> Vec<VersionRecord> {
        let split = self._history.iter().position(|r| r.updated_at > before_timestamp)
            .unwrap_or(self._history.len());
        self._history.drain(..split).collect()
    }

    /// Compact history: keep only the last `max_count` records.
    pub fn compact_max(&mut self, max_count: usize) -> Vec<VersionRecord> {
        if self._history.len() <= max_count {
            return Vec::new();
        }
        let remove_end = self._history.len() - max_count;
        self._history.drain(..remove_end).collect()
    }

    /// Get snapshot at a point in time.
    pub fn at_time(&self, timestamp_us: i64) -> Option<Self> {
        if self._updated_at > timestamp_us {
            for record in self._history.iter().rev() {
                if record.updated_at <= timestamp_us {
                    let label = record.labels.first().cloned().unwrap_or_default();
                    return Some(Self {
                        id: self.id,
                        label,
                        source: self.source,
                        target: self.target,
                        properties: record.properties.clone(),
                        _version: record.version,
                        _updated_at: record.updated_at,
                        _is_deleted: false,
                        _history: Vec::new(),
                    });
                }
            }
            return None;
        }
        if self._is_deleted && self._updated_at <= timestamp_us {
            for record in self._history.iter().rev() {
                if record.updated_at <= timestamp_us {
                    let label = record.labels.first().cloned().unwrap_or_default();
                    return Some(Self {
                        id: self.id,
                        label,
                        source: self.source,
                        target: self.target,
                        properties: record.properties.clone(),
                        _version: record.version,
                        _updated_at: record.updated_at,
                        _is_deleted: false,
                        _history: Vec::new(),
                    });
                }
            }
            return None;
        }
        Some(self.clone())
    }

    pub fn has_label(&self, label: &str) -> bool {
        self.label == label
    }

    pub fn get_property(&self, key: &str) -> Option<&PropertyValue> {
        self.properties.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_version() {
        let e = Edge::new(1, "knows".to_string(), 1, 2);
        assert_eq!(e._version, 1);
    }

    #[test]
    fn test_edge_update() {
        let mut e = Edge::new(1, "knows".to_string(), 1, 2);
        let mut props = HashMap::new();
        props.insert("since".to_string(), PropertyValue::Integer(2020));
        e.update_properties(props, true);
        assert_eq!(e._version, 2);
        assert_eq!(e._history.len(), 1);
    }

    #[test]
    fn test_edge_soft_delete() {
        let mut e = Edge::new(1, "knows".to_string(), 1, 2);
        e.soft_delete(true);
        assert!(e._is_deleted);
        assert_eq!(e._version, 2);
    }
}
