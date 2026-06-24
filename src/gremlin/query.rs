use serde::{Deserialize, Serialize};

/// A single step in a Gremlin traversal pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "step")]
pub enum TraversalStep {
    /// Start traversal by finding vertices via the neural index.
    /// Custom extension to Gremlin.
    #[serde(rename = "search")]
    Search {
        keywords: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
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

    /// Filter where property does NOT match.
    #[serde(rename = "hasNot")]
    HasNot {
        key: String,
        value: serde_json::Value,
    },

    /// Filter where property key exists.
    #[serde(rename = "hasKey")]
    HasKey {
        key: String,
    },

    /// Filter where any property has this value.
    #[serde(rename = "hasValue")]
    HasValue {
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

    /// Traverse outgoing edges (returns EdgeResult, not VertexResult).
    #[serde(rename = "outE")]
    OutE {
        #[serde(default)]
        label: Option<String>,
    },

    /// Traverse incoming edges (returns EdgeResult).
    #[serde(rename = "inE")]
    InE {
        #[serde(default)]
        label: Option<String>,
    },

    /// Traverse both-direction edges (returns EdgeResult).
    #[serde(rename = "bothE")]
    BothE {
        #[serde(default)]
        label: Option<String>,
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

    /// Set a query time point for time-travel (affects subsequent steps).
    /// `at` can be a Unix timestamp in microseconds (integer) or an
    /// ISO 8601 string like "2024-06-10T12:00:00Z".
    ///
    /// Example: {"step": "timeTravel", "at": 1718000000000}
    #[serde(rename = "timeTravel")]
    TimeTravel {
        at: serde_json::Value,
    },

    /// Compact old history records to version log files.
    /// `before` specifies the cutoff timestamp — records older than this
    /// are moved from Vertex._history into .vlog files.
    ///
    /// Example: {"step": "compact", "before": 1718000000000}
    #[serde(rename = "compact")]
    Compact {
        before: serde_json::Value,
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
    pub name: String,
    pub keywords: Vec<String>,
    pub document: String,
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
    pub document: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(step: TraversalStep, expected_json: &str) {
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json, serde_json::from_str::<serde_json::Value>(expected_json).unwrap(),
            "JSON serialization mismatch");
        let deserialized: TraversalStep = serde_json::from_str(expected_json).unwrap();
        match (&step, &deserialized) {
            (a, b) => {
                let a_json = serde_json::to_value(a).unwrap();
                let b_json = serde_json::to_value(b).unwrap();
                assert_eq!(a_json, b_json, "Roundtrip mismatch");
            }
        }
    }

    #[test]
    fn test_v_step() {
        roundtrip(
            TraversalStep::V { ids: vec![1, 2, 3] },
            r#"{"step":"V","ids":[1,2,3]}"#,
        );
    }

    #[test]
    fn test_v_step_empty() {
        let json = r#"{"step":"V"}"#;
        let step: TraversalStep = serde_json::from_str(json).unwrap();
        match step {
            TraversalStep::V { ids } => assert!(ids.is_empty()),
            _ => panic!("Expected V"),
        }
    }

    #[test]
    fn test_e_step() {
        roundtrip(
            TraversalStep::E { ids: vec![10, 20] },
            r#"{"step":"E","ids":[10,20]}"#,
        );
    }

    #[test]
    fn test_has_step() {
        roundtrip(
            TraversalStep::Has { key: "name".into(), value: serde_json::json!("Alice") },
            r#"{"step":"has","key":"name","value":"Alice"}"#,
        );
    }

    #[test]
    fn test_has_numeric_value() {
        roundtrip(
            TraversalStep::Has { key: "age".into(), value: serde_json::json!(30) },
            r#"{"step":"has","key":"age","value":30}"#,
        );
    }

    #[test]
    fn test_has_label() {
        roundtrip(
            TraversalStep::HasLabel { labels: vec!["person".into(), "engineer".into()] },
            r#"{"step":"hasLabel","labels":["person","engineer"]}"#,
        );
    }

    #[test]
    fn test_out_step_default() {
        let step = TraversalStep::Out { label: None, depth: None };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["step"], "out");
        // serde serializes Option::None as null; accept that
        let deserialized: TraversalStep = serde_json::from_value(json).unwrap();
        match deserialized {
            TraversalStep::Out { label, depth } => {
                assert!(label.is_none());
                assert!(depth.is_none());
            }
            _ => panic!("Expected Out"),
        }
    }

    #[test]
    fn test_out_step_with_label() {
        let step = TraversalStep::Out { label: Some("knows".into()), depth: None };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["step"], "out");
        assert_eq!(json["label"], "knows");
        let deserialized: TraversalStep = serde_json::from_value(json).unwrap();
        match deserialized {
            TraversalStep::Out { label, depth: _ } => {
                assert_eq!(label.unwrap(), "knows");
            }
            _ => panic!("Expected Out"),
        }
    }

    #[test]
    fn test_out_step_with_depth() {
        roundtrip(
            TraversalStep::Out { label: Some("knows".into()), depth: Some(3) },
            r#"{"step":"out","label":"knows","depth":3}"#,
        );
    }

    #[test]
    fn test_in_step() {
        let step = TraversalStep::In { label: Some("knows".into()), depth: None };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["step"], "in");
        assert_eq!(json["label"], "knows");
        let deserialized: TraversalStep = serde_json::from_value(json).unwrap();
        match deserialized {
            TraversalStep::In { label, depth: _ } => {
                assert_eq!(label.unwrap(), "knows");
            }
            _ => panic!("Expected In"),
        }
    }

    #[test]
    fn test_both_step() {
        let step = TraversalStep::Both { label: None, depth: Some(2) };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["step"], "both");
        assert_eq!(json["depth"], 2);
        let deserialized: TraversalStep = serde_json::from_value(json).unwrap();
        match deserialized {
            TraversalStep::Both { label, depth } => {
                assert!(label.is_none());
                assert_eq!(depth.unwrap(), 2);
            }
            _ => panic!("Expected Both"),
        }
    }

    #[test]
    fn test_values_step() {
        roundtrip(
            TraversalStep::Values { key: "name".into() },
            r#"{"step":"values","key":"name"}"#,
        );
    }

    #[test]
    fn test_limit_step() {
        roundtrip(
            TraversalStep::Limit { count: 10 },
            r#"{"step":"limit","count":10}"#,
        );
    }

    #[test]
    fn test_count_step() {
        roundtrip(
            TraversalStep::Count,
            r#"{"step":"count"}"#,
        );
    }

    #[test]
    fn test_dedup_step() {
        roundtrip(
            TraversalStep::Dedup,
            r#"{"step":"dedup"}"#,
        );
    }

    #[test]
    fn test_has_text_step() {
        roundtrip(
            TraversalStep::HasText { key: "name".into(), pattern: "Ali".into() },
            r#"{"step":"hasText","key":"name","pattern":"Ali"}"#,
        );
    }

    #[test]
    fn test_repeat_step() {
        let step = TraversalStep::Repeat {
            times: 3,
            steps: vec![
                TraversalStep::Out { label: Some("knows".into()), depth: None },
            ],
        };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["step"], "repeat");
        assert_eq!(json["times"], 3);
        assert_eq!(json["steps"][0]["step"], "out");
        assert_eq!(json["steps"][0]["label"], "knows");
        // Verify roundtrip
        let json_str = serde_json::to_string(&step).unwrap();
        let deserialized: TraversalStep = serde_json::from_str(&json_str).unwrap();
        match &deserialized {
            TraversalStep::Repeat { times, steps } => {
                assert_eq!(*times, 3);
                assert_eq!(steps.len(), 1);
            }
            _ => panic!("Expected Repeat"),
        }
    }

    #[test]
    fn test_time_travel_integer() {
        roundtrip(
            TraversalStep::TimeTravel { at: serde_json::json!(1718000000000u64) },
            r#"{"step":"timeTravel","at":1718000000000}"#,
        );
    }

    #[test]
    fn test_time_travel_iso_string() {
        roundtrip(
            TraversalStep::TimeTravel { at: serde_json::json!("2024-06-10T12:00:00Z") },
            r#"{"step":"timeTravel","at":"2024-06-10T12:00:00Z"}"#,
        );
    }

    #[test]
    fn test_compact_step() {
        roundtrip(
            TraversalStep::Compact { before: serde_json::json!(1718000000000u64) },
            r#"{"step":"compact","before":1718000000000}"#,
        );
    }

    #[test]
    fn test_search_step() {
        roundtrip(
            TraversalStep::Search { keywords: vec!["AI".into(), "engineer".into()], mode: None },
            r#"{"step":"search","keywords":["AI","engineer"]}"#,
        );
    }

    #[test]
    fn test_full_query() {
        let query = GremlinQuery::new(vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Has { key: "name".into(), value: serde_json::json!("Alice") },
            TraversalStep::Out { label: Some("knows".into()), depth: None },
            TraversalStep::Values { key: "name".into() },
            TraversalStep::Limit { count: 10 },
        ]);
        let json = serde_json::to_string(&query).unwrap();
        let deserialized: GremlinQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.steps.len(), 5);
        match &deserialized.steps[0] {
            TraversalStep::V { ids } => assert!(ids.is_empty()),
            _ => panic!("Expected V"),
        }
    }

    #[test]
    fn test_vertex_result_roundtrip() {
        let result = VertexResult {
            element_type: "vertex".into(),
            id: 42,
            name: "Alice".into(),
            keywords: vec![],
            document: "".into(),
            labels: vec!["person".into()],
            properties: {
                let mut m = std::collections::HashMap::new();
                m.insert("extra".into(), serde_json::json!("info"));
                m
            },
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: VertexResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, 42);
        assert_eq!(deserialized.name, "Alice");
        assert_eq!(deserialized.properties["extra"], "info");
    }

    #[test]
    fn test_query_response() {
        let resp = QueryResponse {
            success: true,
            data: vec![
                TraversalResult::CountResult(3),
            ],
            error: None,
            ticks_used: Some(5),
            neurons_fired: Some(vec![1, 2, 3]),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: QueryResponse = serde_json::from_str(&json).unwrap();
        assert!(deserialized.success);
        assert_eq!(deserialized.ticks_used, Some(5));
    }

    // ─── New steps: HasNot / HasKey / HasValue ────────────────

    #[test]
    fn test_has_not_step() {
        roundtrip(
            TraversalStep::HasNot { key: "age".into(), value: serde_json::json!(30) },
            r#"{"step":"hasNot","key":"age","value":30}"#,
        );
    }

    #[test]
    fn test_has_key_step() {
        roundtrip(
            TraversalStep::HasKey { key: "name".into() },
            r#"{"step":"hasKey","key":"name"}"#,
        );
    }

    #[test]
    fn test_has_value_step() {
        roundtrip(
            TraversalStep::HasValue { value: serde_json::json!("Alice") },
            r#"{"step":"hasValue","value":"Alice"}"#,
        );
    }

    // ─── New steps: OutE / InE / BothE ────────────────────────

    #[test]
    fn test_out_e_step() {
        let step = TraversalStep::OutE { label: Some("knows".into()) };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["step"], "outE");
        assert_eq!(json["label"], "knows");
        let deserialized: TraversalStep = serde_json::from_value(json).unwrap();
        match deserialized {
            TraversalStep::OutE { label } => assert_eq!(label.unwrap(), "knows"),
            _ => panic!("Expected OutE"),
        }
    }

    #[test]
    fn test_in_e_step() {
        let step = TraversalStep::InE { label: None };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["step"], "inE");
        let deserialized: TraversalStep = serde_json::from_value(json).unwrap();
        match deserialized {
            TraversalStep::InE { label } => assert!(label.is_none()),
            _ => panic!("Expected InE"),
        }
    }

    #[test]
    fn test_both_e_step() {
        let step = TraversalStep::BothE { label: None };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["step"], "bothE");
        let deserialized: TraversalStep = serde_json::from_value(json).unwrap();
        match deserialized {
            TraversalStep::BothE { label } => assert!(label.is_none()),
            _ => panic!("Expected BothE"),
        }
    }
}
