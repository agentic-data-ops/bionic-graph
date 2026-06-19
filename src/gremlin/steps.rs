use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::graph::{Graph, VertexId, PropertyValue};
use crate::graph::traversal::Bfs;
use crate::neuron::NeuralNetwork;

use super::query::{
    EdgeResult, GremlinQuery, QueryResponse, TraversalResult, TraversalStep, VertexResult,
};

/// Execute a Gremlin query against the graph and neural network.
///
/// Steps are processed in sequence, with the output of each step becoming
/// the input of the next step (pipeline semantics, same as Gremlin).
pub fn execute_query(
    graph: &Arc<Mutex<Graph>>,
    neural_network: &Arc<Mutex<NeuralNetwork>>,
    query: &GremlinQuery,
) -> QueryResponse {
    // The current stream of results — starts empty (needs a source step)
    let mut current: Result<Vec<TraversalResult>, String> = Ok(Vec::new());
    let mut ticks_used: Option<usize> = None;
    let mut neurons_fired: Option<Vec<u64>> = None;

    for step in &query.steps {
        let input = match current {
            Ok(ref items) => items.clone(),
            Err(ref e) => {
                return QueryResponse {
                    success: false,
                    data: Vec::new(),
                    error: Some(format!("Step failed: {}", e)),
                    ticks_used: None,
                    neurons_fired: None,
                };
            }
        };

        current = match step {
            TraversalStep::NeuralSearch { keywords } => {
                let mut nn = neural_network.lock().unwrap();
                let query_str = keywords.join(" ");
                let (ranked_vertices, fired, _hot, ticks) = nn.search(&query_str);
                ticks_used = Some(ticks);
                neurons_fired = Some(fired);

                // Convert ranked vertices to TraversalResults
                let results: Vec<TraversalResult> = ranked_vertices
                    .into_iter()
                    .map(|(vid, _score)| TraversalResult::VertexResult(VertexResult {
                        element_type: "vertex".to_string(),
                        id: vid,
                        labels: Vec::new(), // Will be filled below
                        properties: std::collections::HashMap::new(),
                    }))
                    .collect();

                // Fill in vertex details from graph
                fill_vertex_details(&graph.lock().unwrap(), results)
            }

            TraversalStep::V { ids } => {
                let g = graph.lock().unwrap();
                let results: Vec<TraversalResult> = if ids.is_empty() {
                    // All vertices
                    g.vertex_ids()
                        .map(|&id| vertex_to_result(&g, id))
                        .collect()
                } else {
                    // Specific vertices
                    ids.iter()
                        .filter_map(|&id| {
                            if g.get_vertex(id).is_some() {
                                Some(vertex_to_result(&g, id))
                            } else {
                                None
                            }
                        })
                        .collect()
                };
                Ok(results)
            }

            TraversalStep::E { ids } => {
                let g = graph.lock().unwrap();
                let results: Vec<TraversalResult> = if ids.is_empty() {
                    g.all_edges()
                        .map(|e| edge_to_result(e))
                        .collect()
                } else {
                    let id_set: HashSet<_> = ids.iter().copied().collect();
                    g.all_edges()
                        .filter(|e| id_set.contains(&e.id))
                        .map(|e| edge_to_result(e))
                        .collect()
                };
                Ok(results)
            }

            TraversalStep::Has { key, value } => {
                let g = graph.lock().unwrap();
                let results = filter_by_property(&g, input, key, value);
                Ok(results)
            }

            TraversalStep::HasLabel { labels } => {
                let g = graph.lock().unwrap();
                let label_set: HashSet<&str> = labels.iter().map(|s| s.as_str()).collect();
                let results: Vec<TraversalResult> = input
                    .into_iter()
                    .filter(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            v.labels.iter().any(|l| label_set.contains(l.as_str()))
                        }
                        TraversalResult::EdgeResult(e) => {
                            label_set.contains(e.label.as_str())
                        }
                        _ => true, // Pass through non-graph results
                    })
                    .collect();
                Ok(results)
            }

            TraversalStep::Out { label, depth } => {
                let d = depth.unwrap_or(1);
                if d <= 1 {
                    // Single level (original behaviour)
                    let g = graph.lock().unwrap();
                    let results = single_level_traverse(input, &g, |g, id, lbl| {
                        g.out_neighbors(id, lbl)
                    }, label.as_deref());
                    Ok(results)
                } else {
                    // Multi-level BFS
                    let g = graph.lock().unwrap();
                    let results = multi_level_traverse(input, &g, label.as_deref(), d, true, false);
                    Ok(results)
                }
            }

            TraversalStep::In { label, depth } => {
                let d = depth.unwrap_or(1);
                if d <= 1 {
                    let g = graph.lock().unwrap();
                    let results = single_level_traverse(input, &g, |g, id, lbl| {
                        g.in_neighbors(id, lbl)
                    }, label.as_deref());
                    Ok(results)
                } else {
                    let g = graph.lock().unwrap();
                    let results = multi_level_traverse(input, &g, label.as_deref(), d, false, true);
                    Ok(results)
                }
            }

            TraversalStep::Both { label, depth } => {
                let d = depth.unwrap_or(1);
                if d <= 1 {
                    let g = graph.lock().unwrap();
                    let results = single_level_traverse(input, &g, |g, id, lbl| {
                        g.both_neighbors(id, lbl)
                    }, label.as_deref());
                    Ok(results)
                } else {
                    let g = graph.lock().unwrap();
                    let results = multi_level_traverse(input, &g, label.as_deref(), d, true, true);
                    Ok(results)
                }
            }

            TraversalStep::HasText { key, pattern } => {
                let pattern_lower = pattern.to_lowercase();
                let results: Vec<TraversalResult> = input
                    .into_iter()
                    .filter(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            v.properties.get(key).map_or(false, |val| {
                                val.as_str().map_or(false, |s| {
                                    s.to_lowercase().contains(&pattern_lower)
                                })
                            })
                        }
                        TraversalResult::EdgeResult(e) => {
                            e.properties.get(key).map_or(false, |val| {
                                val.as_str().map_or(false, |s| {
                                    s.to_lowercase().contains(&pattern_lower)
                                })
                            })
                        }
                        _ => false,
                    })
                    .collect();
                Ok(results)
            }

            TraversalStep::Repeat { times, steps } => {
                let mut current = input;
                for _ in 0..*times {
                    current = run_steps(&current, steps, graph, neural_network)?;
                    if current.is_empty() {
                        break;
                    }
                }
                Ok(current)
            }

            TraversalStep::Compact { before } => {
                let timestamp = parse_time_value(before)?;
                log::info!("Compacting history before timestamp {}", timestamp);
                let data_dir = std::path::Path::new("data");
                let mut g = graph.lock().unwrap();
                let stats = crate::storage::compaction::compact_graph(&mut g, data_dir, timestamp, 0);
                let result = serde_json::json!({
                    "compacted": {
                        "vertices_scanned": stats.vertices_scanned,
                        "vertices_compacted": stats.vertices_compacted,
                        "records_archived": stats.records_archived,
                        "records_truncated": stats.records_truncated,
                        "elapsed_us": stats.elapsed_us,
                    }
                });
                Ok(vec![TraversalResult::ValueResult(result)])
            }

            TraversalStep::TimeTravel { at } => {
                let timestamp = parse_time_value(at)?;
                log::debug!("TimeTravel: filtering at timestamp {}", timestamp);
                let g = graph.lock().unwrap();
                let results: Vec<TraversalResult> = input
                    .into_iter()
                    .filter_map(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            let original = g.get_vertex_including_deleted(v.id)?;
                            let snapshot = original.at_time(timestamp)?;
                            Some(vertex_to_result(&g, snapshot.id))
                        }
                        _ => Some(r),
                    })
                    .collect();
                Ok(results)
            }

            TraversalStep::Values { key } => {
                let results: Vec<TraversalResult> = input
                    .into_iter()
                    .filter_map(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            v.properties.get(key).map(|val| TraversalResult::ValueResult(val.clone()))
                        }
                        TraversalResult::EdgeResult(e) => {
                            e.properties.get(key).map(|val| TraversalResult::ValueResult(val.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                Ok(results)
            }

            TraversalStep::Limit { count } => {
                let limited: Vec<TraversalResult> = input.into_iter().take(*count).collect();
                Ok(limited)
            }

            TraversalStep::Count => {
                let count = input.len() as u64;
                Ok(vec![TraversalResult::CountResult(count)])
            }

            TraversalStep::Dedup => {
                let mut seen_ids = HashSet::new();
                let mut deduped = Vec::new();
                for r in input {
                    let id = match &r {
                        TraversalResult::VertexResult(v) => Some(v.id),
                        TraversalResult::EdgeResult(e) => Some(e.id),
                        _ => None,
                    };
                    if let Some(id) = id {
                        if seen_ids.insert(id) {
                            deduped.push(r);
                        }
                    } else {
                        deduped.push(r);
                    }
                }
                Ok(deduped)
            }
        };
    }

    match current {
        Ok(data) => QueryResponse {
            success: true,
            data,
            error: None,
            ticks_used,
            neurons_fired,
        },
        Err(e) => QueryResponse {
            success: false,
            data: Vec::new(),
            error: Some(e),
            ticks_used: None,
            neurons_fired: None,
        },
    }
}

// ─── Traversal Helpers ────────────────────────────────────────────

/// Single-level neighbor traversal (original Gremlin out/in/both).
fn single_level_traverse(
    input: Vec<TraversalResult>,
    g: &Graph,
    neighbor_fn: fn(&Graph, VertexId, Option<&str>) -> Vec<VertexId>,
    label: Option<&str>,
) -> Vec<TraversalResult> {
    input
        .into_iter()
        .flat_map(|r| match r {
            TraversalResult::VertexResult(v) => {
                let neighbors = neighbor_fn(g, v.id, label);
                neighbors
                    .into_iter()
                    .map(|nid| vertex_to_result(g, nid))
                    .collect::<Vec<_>>()
            }
            _ => Vec::new(),
        })
        .collect()
}

/// Multi-level BFS traversal.
fn multi_level_traverse(
    input: Vec<TraversalResult>,
    g: &Graph,
    label: Option<&str>,
    depth: usize,
    _out: bool,
    _in: bool,
) -> Vec<TraversalResult> {
    // Collect starting vertex IDs
    let start_ids: Vec<VertexId> = input
        .into_iter()
        .filter_map(|r| match r {
            TraversalResult::VertexResult(v) => Some(v.id),
            _ => None,
        })
        .collect();

    if start_ids.is_empty() {
        return Vec::new();
    }

    // Build BFS from start vertices
    let mut seen = HashSet::new();
    let mut results = Vec::new();
    let mut queue = VecDeque::new();

    for &sid in &start_ids {
        if seen.insert(sid) {
            queue.push_back((sid, 0usize));
        }
    }

    while let Some((vid, d)) = queue.pop_front() {
        // Emit the vertex if it's not a starting vertex (depth > 0)
        // or if it is a start vertex (depth == 0)
        if d > 0 && d <= depth {
            results.push(vertex_to_result(g, vid));
        }

        if d < depth {
            let neighbors = if _out && _in {
                g.both_neighbors(vid, label)
            } else if _out {
                g.out_neighbors(vid, label)
            } else {
                g.in_neighbors(vid, label)
            };

            for nid in neighbors {
                if seen.insert(nid) {
                    queue.push_back((nid, d + 1));
                }
            }
        }
    }

    results
}

/// Run a sequence of Gremlin steps on an input stream (used by Repeat).
fn run_steps(
    input: &[TraversalResult],
    steps: &[TraversalStep],
    graph: &Arc<Mutex<Graph>>,
    neural_network: &Arc<Mutex<NeuralNetwork>>,
) -> Result<Vec<TraversalResult>, String> {
    let mut current: Vec<TraversalResult> = input.to_vec();

    for step in steps {
        current = match step {
            TraversalStep::V { ids } => {
                let g = graph.lock().unwrap();
                if ids.is_empty() {
                    g.vertex_ids()
                        .map(|&id| vertex_to_result(&g, id))
                        .collect()
                } else {
                    ids.iter()
                        .filter_map(|&id| {
                            if g.get_vertex(id).is_some() {
                                Some(vertex_to_result(&g, id))
                            } else {
                                None
                            }
                        })
                        .collect()
                }
            }

            TraversalStep::Out { label, depth } => {
                let g = graph.lock().unwrap();
                let d = depth.unwrap_or(1);
                if d <= 1 {
                    single_level_traverse(current, &g, |g, id, lbl| g.out_neighbors(id, lbl), label.as_deref())
                } else {
                    multi_level_traverse(current, &g, label.as_deref(), d, true, false)
                }
            }

            TraversalStep::In { label, depth } => {
                let g = graph.lock().unwrap();
                let d = depth.unwrap_or(1);
                if d <= 1 {
                    single_level_traverse(current, &g, |g, id, lbl| g.in_neighbors(id, lbl), label.as_deref())
                } else {
                    multi_level_traverse(current, &g, label.as_deref(), d, false, true)
                }
            }

            TraversalStep::Both { label, depth } => {
                let g = graph.lock().unwrap();
                let d = depth.unwrap_or(1);
                if d <= 1 {
                    single_level_traverse(current, &g, |g, id, lbl| g.both_neighbors(id, lbl), label.as_deref())
                } else {
                    multi_level_traverse(current, &g, label.as_deref(), d, true, true)
                }
            }

            TraversalStep::Has { key, value } => {
                let g = graph.lock().unwrap();
                filter_by_property(&g, current, key, value)
            }

            TraversalStep::HasLabel { labels } => {
                let label_set: HashSet<&str> = labels.iter().map(|s| s.as_str()).collect();
                current.into_iter()
                    .filter(|r| match r {
                        TraversalResult::VertexResult(v) => v.labels.iter().any(|l| label_set.contains(l.as_str())),
                        TraversalResult::EdgeResult(e) => label_set.contains(e.label.as_str()),
                        _ => true,
                    })
                    .collect()
            }

            TraversalStep::HasText { key, pattern } => {
                let pattern_lower = pattern.to_lowercase();
                current.into_iter()
                    .filter(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            v.properties.get(key).map_or(false, |val| {
                                val.as_str().map_or(false, |s| s.to_lowercase().contains(&pattern_lower))
                            })
                        }
                        TraversalResult::EdgeResult(e) => {
                            e.properties.get(key).map_or(false, |val| {
                                val.as_str().map_or(false, |s| s.to_lowercase().contains(&pattern_lower))
                            })
                        }
                        _ => false,
                    })
                    .collect()
            }

            TraversalStep::Values { key } => {
                current.into_iter()
                    .filter_map(|r| match r {
                        TraversalResult::VertexResult(v) => v.properties.get(key).cloned().map(TraversalResult::ValueResult),
                        TraversalResult::EdgeResult(e) => e.properties.get(key).cloned().map(TraversalResult::ValueResult),
                        _ => None,
                    })
                    .collect()
            }

            TraversalStep::Limit { count } => current.into_iter().take(*count).collect(),

            TraversalStep::Count => vec![TraversalResult::CountResult(current.len() as u64)],

            TraversalStep::Dedup => {
                let mut seen_ids = HashSet::new();
                current.into_iter()
                    .filter(|r| {
                        let id = match r {
                            TraversalResult::VertexResult(v) => Some(v.id),
                            TraversalResult::EdgeResult(e) => Some(e.id),
                            _ => None,
                        };
                        id.map_or(true, |id| seen_ids.insert(id))
                    })
                    .collect()
            }

            TraversalStep::Repeat { times, steps } => {
                let mut cur = current;
                for _ in 0..*times {
                    cur = run_steps(&cur, steps, graph, neural_network)?;
                    if cur.is_empty() {
                        break;
                    }
                }
                cur
            }

            TraversalStep::Compact { before } => {
                let timestamp = parse_time_value(before)?;
                let mut g = graph.lock().unwrap();
                let data_dir = std::path::Path::new("data");
                crate::storage::compaction::compact_graph(&mut g, data_dir, timestamp, 0);
                current
            }

            TraversalStep::TimeTravel { at } => {
                let timestamp = parse_time_value(at)?;
                let g = graph.lock().unwrap();
                current = current.into_iter()
                    .filter_map(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            let original = g.get_vertex_including_deleted(v.id)?;
                            original.at_time(timestamp)?;
                            Some(TraversalResult::VertexResult(v))
                        }
                        _ => Some(r),
                    })
                    .collect();
                current
            }

            TraversalStep::NeuralSearch { .. } => {
                return Err("neuralSearch is not supported inside repeat".to_string());
            }

            TraversalStep::E { .. } => {
                return Err("E() is not supported inside repeat".to_string());
            }
        };
    }

    Ok(current)
}

// ─── Time Travel ─────────────────────────────────────────────────

/// Parse the `at` value from a timeTravel step.
/// Accepts: integer (Unix microseconds) or ISO 8601 string.
pub fn parse_time_value(at: &serde_json::Value) -> Result<i64, String> {
    match at {
        Value::Number(n) => n.as_i64().ok_or_else(|| "Invalid timestamp".to_string()),
        Value::String(s) => {
            // Try parsing as integer first
            if let Ok(ts) = s.parse::<i64>() {
                return Ok(ts);
            }
            // Try ISO 8601: "2024-06-10T12:00:00Z"
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                return Ok(dt.timestamp_micros());
            }
            // Try "2024-06-10T12:00:00" (no timezone — assume UTC)
            if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
                return Ok(dt.and_utc().timestamp_micros());
            }
            // Try date only "2024-06-10"
            if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                let dt = d.and_hms_opt(0, 0, 0).unwrap();
                return Ok(dt.and_utc().timestamp_micros());
            }
            Err(format!("Cannot parse time '{}'. Use Unix μs or ISO 8601.", s))
        }
        _ => Err("timeTravel.at must be a number (Unix μs) or string (ISO 8601)".to_string()),
    }
}

// ─── Helper Functions ─────────────────────────────────────────────

fn vertex_to_result(g: &Graph, id: VertexId) -> TraversalResult {
    let v = g.get_vertex(id).expect("vertex should exist");
    let props: std::collections::HashMap<String, Value> = v
        .properties
        .iter()
        .map(|(k, pv)| (k.clone(), property_to_json(pv)))
        .collect();

    TraversalResult::VertexResult(VertexResult {
        element_type: "vertex".to_string(),
        id: v.id,
        labels: v.labels.clone(),
        properties: props,
    })
}

fn edge_to_result(e: &crate::graph::Edge) -> TraversalResult {
    let props: std::collections::HashMap<String, Value> = e
        .properties
        .iter()
        .map(|(k, pv)| (k.clone(), property_to_json(pv)))
        .collect();

    TraversalResult::EdgeResult(EdgeResult {
        element_type: "edge".to_string(),
        id: e.id,
        label: e.label.clone(),
        source: e.source,
        target: e.target,
        properties: props,
    })
}

fn property_to_json(pv: &PropertyValue) -> Value {
    match pv {
        PropertyValue::String(s) => Value::String(s.clone()),
        PropertyValue::Integer(n) => Value::Number((*n).into()),
        PropertyValue::Float(f) => {
            serde_json::Number::from_f64(*f)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        PropertyValue::Boolean(b) => Value::Bool(*b),
        PropertyValue::List(items) => {
            Value::Array(items.iter().map(property_to_json).collect())
        }
        PropertyValue::Null => Value::Null,
    }
}

fn fill_vertex_details(g: &Graph, results: Vec<TraversalResult>) -> Result<Vec<TraversalResult>, String> {
    let filled: Vec<TraversalResult> = results
        .into_iter()
        .map(|r| match r {
            TraversalResult::VertexResult(v) => {
                if let Some(vertex) = g.get_vertex(v.id) {
                    let props: std::collections::HashMap<String, Value> = vertex
                        .properties
                        .iter()
                        .map(|(k, pv)| (k.clone(), property_to_json(pv)))
                        .collect();
                    TraversalResult::VertexResult(VertexResult {
                        labels: vertex.labels.clone(),
                        properties: props,
                        ..v
                    })
                } else {
                    r
                }
            }
            _ => r,
        })
        .collect();
    Ok(filled)
}

fn filter_by_property(
    g: &Graph,
    input: Vec<TraversalResult>,
    key: &str,
    value: &Value,
) -> Vec<TraversalResult> {
    input
        .into_iter()
        .filter(|r| {
            let props = match r {
                TraversalResult::VertexResult(ref v) => &v.properties,
                TraversalResult::EdgeResult(ref e) => &e.properties,
                _ => return false,
            };
            props.get(key).map_or(false, |pv| pv == value)
        })
        .collect()
}
