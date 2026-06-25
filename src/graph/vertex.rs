use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a graph vertex.
pub type VertexId = u64;

// ─── PropertyValue (used by both Vertex and Edge) ─────────────────

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

// ─── VersionRecord ───────────────────────────────────────────────

/// A snapshot of a vertex or edge at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionRecord {
    pub version: u64,
    pub updated_at: i64,
    pub name: String,
    pub keywords: Vec<String>,
    pub document: String,
    pub labels: Vec<String>,
    pub properties: HashMap<String, PropertyValue>,
}

// ─── Vertex ──────────────────────────────────────────────────────

/// A vertex (node) in the knowledge graph, with MVCC support.
///
/// Each vertex carries a version number, update timestamp, soft-delete flag,
/// and a history of previous property/label states for time-travel queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vertex {
    pub id: VertexId,
    pub name: String,
    pub keywords: Vec<String>,
    pub document: String,
    pub labels: Vec<String>,
    pub properties: HashMap<String, PropertyValue>,

    // ─── Version fields ──────────────────────────────────────────
    /// Monotonic version number — increments on every update.
    pub _version: u64,
    /// Unix timestamp (microseconds) of the last modification.
    pub _updated_at: i64,
    /// Soft-delete flag — when true the vertex is considered deleted.
    pub _is_deleted: bool,
    /// Snapshot history for time-travel. _history[0] = oldest.
    pub _history: Vec<VersionRecord>,
}

impl Vertex {
    pub fn new(id: VertexId, labels: Vec<String>) -> Self {
        Self {
            id,
            name: String::new(),
            keywords: Vec::new(),
            document: String::new(),
            labels,
            properties: HashMap::new(),
            _version: 1,
            _updated_at: now_micros(),
            _is_deleted: false,
            _history: Vec::new(),
        }
    }

    pub fn named(id: VertexId, labels: Vec<String>, name: String) -> Self {
        let mut v = Self::new(id, labels);
        v.name = name;
        v
    }

    /// Create a vertex from a historical snapshot at a given point in time.
    pub fn from_history(id: VertexId, record: &VersionRecord) -> Self {
        Self {
            id,
            name: record.name.clone(),
            keywords: record.keywords.clone(),
            document: record.document.clone(),
            labels: record.labels.clone(),
            properties: record.properties.clone(),
            _version: record.version,
            _updated_at: record.updated_at,
            _is_deleted: false,
            _history: Vec::new(),
        }
    }

    /// Update properties — bumps version, saves snapshot to history.
    /// If `record_history` is false, directly overwrites without versioning.
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn set_keywords(&mut self, keywords: Vec<String>) {
        self.keywords = keywords;
    }

    pub fn update_properties(&mut self, new_props: HashMap<String, PropertyValue>, record_history: bool) {
        if record_history {
            let now = now_micros();
            self._history.push(VersionRecord {
                version: self._version,
                updated_at: self._updated_at,
                name: self.name.clone(),
                keywords: self.keywords.clone(),
                document: self.document.clone(),
                labels: self.labels.clone(),
                properties: self.properties.clone(),
            });
            self._version += 1;
            self._updated_at = now;
        }
        self.properties = new_props;
    }

    /// Update labels — bumps version, saves snapshot to history.
    pub fn update_labels(&mut self, new_labels: Vec<String>, record_history: bool) {
        if record_history {
            let now = now_micros();
            self._history.push(VersionRecord {
                version: self._version,
                updated_at: self._updated_at,
                name: self.name.clone(),
                keywords: self.keywords.clone(),
                document: self.document.clone(),
                labels: self.labels.clone(),
                properties: self.properties.clone(),
            });
            self._version += 1;
            self._updated_at = now;
        }
        self.labels = new_labels;
    }

    /// Soft-delete this vertex. If `record_history` is false, just sets the flag.
    pub fn soft_delete(&mut self, record_history: bool) {
        if !self._is_deleted {
            if record_history {
                let now = now_micros();
                self._history.push(VersionRecord {
                    version: self._version,
                    updated_at: self._updated_at,
                    name: self.name.clone(),
                    keywords: self.keywords.clone(),
                document: self.document.clone(),
                    labels: self.labels.clone(),
                    properties: self.properties.clone(),
                });
                self._version += 1;
                self._updated_at = now;
            }
            self._is_deleted = true;
        }
    }

    /// Restore a soft-deleted vertex.
    pub fn restore(&mut self, record_history: bool) {
        if self._is_deleted {
            if record_history {
                let now = now_micros();
                self._history.push(VersionRecord {
                    version: self._version,
                    updated_at: self._updated_at,
                    name: self.name.clone(),
                    keywords: self.keywords.clone(),
                document: self.document.clone(),
                    labels: self.labels.clone(),
                    properties: self.properties.clone(),
                });
                self._version += 1;
                self._updated_at = now;
            }
            self._is_deleted = false;
        }
    }

    /// Compact history: remove records before `before_timestamp` and return them.
    /// Returns the removed records for offloading to version log.
    pub fn compact(&mut self, before_timestamp: i64) -> Vec<VersionRecord> {
        let split = self._history.iter().position(|r| r.updated_at > before_timestamp)
            .unwrap_or(self._history.len());
        let removed: Vec<VersionRecord> = self._history.drain(..split).collect();
        removed
    }

    /// Compact history: keep only the last `max_count` records, return the rest.
    pub fn compact_max(&mut self, max_count: usize) -> Vec<VersionRecord> {
        if self._history.len() <= max_count {
            return Vec::new();
        }
        let remove_end = self._history.len() - max_count;
        let removed: Vec<VersionRecord> = self._history.drain(..remove_end).collect();
        removed
    }

    /// Get a snapshot of this vertex as it existed at `timestamp_us`.
    /// Returns `None` if the vertex didn't exist yet or was already deleted.
    pub fn at_time(&self, timestamp_us: i64) -> Option<Self> {
        if self._updated_at > timestamp_us {
            for record in self._history.iter().rev() {
                if record.updated_at <= timestamp_us {
                    return Some(Vertex::from_history(self.id, record));
                }
            }
            return None;
        }
        if self._is_deleted && self._updated_at <= timestamp_us {
            for record in self._history.iter().rev() {
                if record.updated_at <= timestamp_us {
                    return Some(Vertex::from_history(self.id, record));
                }
            }
            return None;
        }
        Some(self.clone())
    }

    pub fn has_label(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l == label)
    }

    pub fn get_property(&self, key: &str) -> Option<&PropertyValue> {
        self.properties.get(key)
    }
}

/// Returns current Unix time in microseconds.
pub fn now_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_starts_at_one() {
        let v = Vertex::new(1, vec!["person".to_string()]);
        assert_eq!(v._version, 1);
    }

    #[test]
    fn test_update_increments_version() {
        let mut v = Vertex::new(1, vec!["person".to_string()]);
        let mut props = HashMap::new();
        props.insert("name".to_string(), PropertyValue::String("Alice".to_string()));
        v.update_properties(props, true);
        assert_eq!(v._version, 2);
        assert_eq!(v._history.len(), 1);
    }

    #[test]
    fn test_soft_delete() {
        let mut v = Vertex::new(1, vec!["person".to_string()]);
        assert!(!v._is_deleted);
        v.soft_delete(true);
        assert!(v._is_deleted);
        assert_eq!(v._version, 2);
    }

    #[test]
    fn test_restore() {
        let mut v = Vertex::new(1, vec!["person".to_string()]);
        v.soft_delete(true);
        assert!(v._is_deleted);
        v.restore(true);
        assert!(!v._is_deleted);
        assert_eq!(v._version, 3);
    }

    #[test]
    fn test_at_time_returns_history() {
        let mut v = Vertex::new(1, vec!["person".to_string()]);
        let mut props_v1 = HashMap::new();
        props_v1.insert("name".to_string(), PropertyValue::String("Alice".to_string()));
        v.update_properties(props_v1, true);

        let t2 = now_micros();
        let mut props_v2 = HashMap::new();
        props_v2.insert("name".to_string(), PropertyValue::String("Alicia".to_string()));
        v.update_properties(props_v2, true);

        if let Some(snap) = v.at_time(t2) {
            let name = snap.properties.get("name").unwrap();
            assert_eq!(*name, PropertyValue::String("Alice".to_string()));
        } else {
            panic!("Should have returned historical version");
        }
    }
}
