use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::extract::ExtractionConfig;
use crate::graph::{Graph, VertexId, PropertyValue};
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
    execute_query_with_llm(graph, neural_network, query, None)
}

/// Execute a Gremlin query with optional LLM config for semantic search.
pub fn execute_query_with_llm(
    graph: &Arc<Mutex<Graph>>,
    neural_network: &Arc<Mutex<NeuralNetwork>>,
    query: &GremlinQuery,
    _llm_config: Option<&ExtractionConfig>,
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
            TraversalStep::Search { keywords, mode } => {
                let mut nn = neural_network.lock().unwrap();
                let query_str = keywords.join(" ");
                nn.set_search_mode(mode.as_deref());
                let (ranked_vertices, ranked_edges, fired, _hot, ticks) = nn.search(&query_str);
                ticks_used = Some(ticks);
                neurons_fired = Some(fired);

                let g = graph.lock().unwrap();
                let mut results: Vec<TraversalResult> = ranked_vertices
                    .into_iter()
                    .take(100)
                    .map(|(vid, _score)| {
                        if let Some(vertex) = g.get_vertex(vid) {
                            let props: std::collections::HashMap<String, Value> = vertex
                                .properties
                                .iter()
                                .map(|(k, pv)| (k.clone(), property_to_json(pv)))
                                .collect();
                            TraversalResult::VertexResult(VertexResult {
                                element_type: "vertex".to_string(),
                                id: vertex.id,
                                name: vertex.name.clone(),
                                keywords: vertex.keywords.clone(),
                                document: vertex.document.clone(),
                                labels: vertex.labels.clone(),
                                properties: props,
                            })
                        } else {
                            TraversalResult::VertexResult(VertexResult {
                                element_type: "vertex".to_string(),
                                id: vid,
                                name: String::new(),
                                keywords: Vec::new(),
                                document: String::new(),
                                labels: Vec::new(),
                                properties: std::collections::HashMap::new(),
                            })
                        }
                    })
                    .collect();

                // Add edge results from search
                for (eid, _score) in ranked_edges {
                    if let Some(e) = g.get_edge(eid) {
                        let eprops: std::collections::HashMap<String, Value> = e
                            .properties
                            .iter()
                            .map(|(k, pv)| (k.clone(), property_to_json(pv)))
                            .collect();
                        results.push(TraversalResult::EdgeResult(EdgeResult {
                            element_type: "edge".to_string(),
                            id: e.id,
                            label: e.label.clone(),
                            source: e.source,
                            target: e.target,
                            properties: eprops,
                        }));
                    }
                }
                drop(g);

                Ok(results)
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

            TraversalStep::HasNot { key, value } => {
                let _g = graph.lock().unwrap();
                let results = filter_by_property_not(&_g, input, key, value);
                Ok(results)
            }

            TraversalStep::HasKey { key } => {
                let _g = graph.lock().unwrap();
                let results: Vec<TraversalResult> = input
                    .into_iter()
                    .filter(|r| match r {
                        TraversalResult::VertexResult(v) => v.properties.contains_key(key),
                        TraversalResult::EdgeResult(e) => e.properties.contains_key(key),
                        _ => false,
                    })
                    .collect();
                Ok(results)
            }

            TraversalStep::HasValue { value } => {
                let results: Vec<TraversalResult> = input
                    .into_iter()
                    .filter(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            v.properties.values().any(|pv| pv == value)
                        }
                        TraversalResult::EdgeResult(e) => {
                            e.properties.values().any(|pv| pv == value)
                        }
                        _ => false,
                    })
                    .collect();
                Ok(results)
            }

            TraversalStep::HasLabel { labels } => {
                let _g = graph.lock().unwrap();
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

            TraversalStep::OutE { label } => {
                let g = graph.lock().unwrap();
                let results: Vec<TraversalResult> = input
                    .into_iter()
                    .flat_map(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            let eids = g.outgoing_edges(v.id);
                            let out: Vec<TraversalResult> = eids.iter()
                                .filter_map(|eid| g.get_edge(*eid))
                                .filter(|e| label.as_deref().map_or(true, |l| e.label == l))
                                .map(|e| edge_to_result(e))
                                .collect();
                            out
                        }
                        other => vec![other],
                    })
                    .collect();
                Ok(results)
            }

            TraversalStep::InE { label } => {
                let g = graph.lock().unwrap();
                let results: Vec<TraversalResult> = input
                    .into_iter()
                    .flat_map(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            let eids = g.incoming_edges(v.id);
                            let out: Vec<TraversalResult> = eids.iter()
                                .filter_map(|eid| g.get_edge(*eid))
                                .filter(|e| label.as_deref().map_or(true, |l| e.label == l))
                                .map(|e| edge_to_result(e))
                                .collect();
                            out
                        }
                        other => vec![other],
                    })
                    .collect();
                Ok(results)
            }

            TraversalStep::BothE { label } => {
                let g = graph.lock().unwrap();
                let results: Vec<TraversalResult> = input
                    .into_iter()
                    .flat_map(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            let eids: Vec<_> = g.outgoing_edges(v.id).into_iter()
                                .chain(g.incoming_edges(v.id).into_iter())
                                .collect();
                            let out: Vec<TraversalResult> = eids.iter()
                                .filter_map(|eid| g.get_edge(*eid))
                                .filter(|e| label.as_deref().map_or(true, |l| e.label == l))
                                .map(|e| edge_to_result(e))
                                .collect();
                            out
                        }
                        other => vec![other],
                    })
                    .collect();
                Ok(results)
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
                    current = match run_steps(&current, steps, graph, neural_network) {
                        Ok(v) => v,
                        Err(e) => {
                            return QueryResponse {
                                success: false, data: Vec::new(), error: Some(e),
                                ticks_used: None, neurons_fired: None,
                            };
                        }
                    };
                    if current.is_empty() {
                        break;
                    }
                }
                Ok(current)
            }

            TraversalStep::Compact { before } => {
                let timestamp = match parse_time_value(before) {
                    Ok(ts) => ts,
                    Err(e) => {
                        return QueryResponse {
                            success: false, data: Vec::new(), error: Some(e),
                            ticks_used: None, neurons_fired: None,
                        };
                    }
                };
                log::info!("Compacting history before timestamp {}", timestamp);
                let data_dir = std::path::Path::new("data/graphs");
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
                let timestamp = match parse_time_value(at) {
                    Ok(ts) => ts,
                    Err(e) => {
                        return QueryResponse {
                            success: false, data: Vec::new(), error: Some(e),
                            ticks_used: None, neurons_fired: None,
                        };
                    }
                };
                log::debug!("TimeTravel: filtering at timestamp {}", timestamp);
                let g = graph.lock().unwrap();
                let results: Vec<TraversalResult> = input
                    .into_iter()
                    .filter_map(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            let original = g.get_vertex_including_deleted(v.id)?;
                            let snapshot = original.at_time(timestamp)?;
                            Some(vertex_from_snapshot(&snapshot))
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

            TraversalStep::OutE { label } => {
                let g = graph.lock().unwrap();
                let eids: Vec<_> = current.iter()
                    .flat_map(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            let mut e = g.outgoing_edges(v.id);
                            if let Some(ref lbl) = label {
                                e.retain(|eid| g.get_edge(*eid).map_or(false, |e| e.label == *lbl));
                            }
                            e
                        }
                        _ => vec![],
                    })
                    .collect();
                eids.iter()
                    .filter_map(|eid| g.get_edge(*eid))
                    .map(|e| edge_to_result(e))
                    .collect::<Vec<_>>()
            }

            TraversalStep::InE { label } => {
                let g = graph.lock().unwrap();
                let eids: Vec<_> = current.iter()
                    .flat_map(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            let mut e = g.incoming_edges(v.id);
                            if let Some(ref lbl) = label {
                                e.retain(|eid| g.get_edge(*eid).map_or(false, |e| e.label == *lbl));
                            }
                            e
                        }
                        _ => vec![],
                    })
                    .collect();
                eids.iter()
                    .filter_map(|eid| g.get_edge(*eid))
                    .map(|e| edge_to_result(e))
                    .collect::<Vec<_>>()
            }

            TraversalStep::BothE { label } => {
                let g = graph.lock().unwrap();
                let eids: Vec<_> = current.iter()
                    .flat_map(|r| match r {
                        TraversalResult::VertexResult(v) => {
                            let mut e: Vec<_> = g.outgoing_edges(v.id).into_iter()
                                .chain(g.incoming_edges(v.id).into_iter())
                                .collect();
                            if let Some(ref lbl) = label {
                                e.retain(|eid| g.get_edge(*eid).map_or(false, |e| e.label == *lbl));
                            }
                            e
                        }
                        _ => vec![],
                    })
                    .collect();
                eids.iter()
                    .filter_map(|eid| g.get_edge(*eid))
                    .map(|e| edge_to_result(e))
                    .collect::<Vec<_>>()
            }

            TraversalStep::Has { key, value } => {
                let g = graph.lock().unwrap();
                filter_by_property(&g, current, key, value)
            }

            TraversalStep::HasNot { key, value } => {
                let g = graph.lock().unwrap();
                filter_by_property_not(&g, current, key, value)
            }

            TraversalStep::HasKey { key } => {
                current.into_iter()
                    .filter(|r| match r {
                        TraversalResult::VertexResult(v) => v.properties.contains_key(key),
                        TraversalResult::EdgeResult(e) => e.properties.contains_key(key),
                        _ => false,
                    })
                    .collect::<Vec<_>>()
            }

            TraversalStep::HasValue { value } => {
                current.into_iter()
                    .filter(|r| match r {
                        TraversalResult::VertexResult(v) => v.properties.values().any(|pv| pv == value),
                        TraversalResult::EdgeResult(e) => e.properties.values().any(|pv| pv == value),
                        _ => false,
                    })
                    .collect::<Vec<_>>()
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
                let data_dir = std::path::Path::new("data/graphs");
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

            TraversalStep::Search { .. } => {
                return Err("search is not supported inside repeat".to_string());
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
/// Extract search keywords from a natural language query using LLM.
/// Falls back to simple whitespace splitting if LLM is unavailable.
#[allow(dead_code)]
fn extract_search_keywords(llm_config: Option<&ExtractionConfig>, query: &str) -> Vec<String> {
    if let Some(config) = llm_config {
        let system_prompt = "Extract 3-5 key search keywords from this query. Keep names in their original language (e.g. Chinese names stay in Chinese). Return ONLY a JSON array of strings, no other text.";
        let user_msg = format!("Query: {}", query);
        match tokio::runtime::Handle::current().block_on(
            crate::extract::llm_client::chat_completion_with_retry(config, system_prompt, &user_msg)
        ) {
            Ok(result) => {
                let trimmed = result.content.trim();
                if trimmed.starts_with('[') {
                    serde_json::from_str::<Vec<String>>(trimmed).unwrap_or_else(|_| {
                        query.split_whitespace().map(|s| s.to_string()).collect()
                    })
                } else {
                    query.split_whitespace().map(|s| s.to_string()).collect()
                }
            }
            Err(_) => query.split_whitespace().map(|s| s.to_string()).collect(),
        }
    } else {
        query.split_whitespace().map(|s| s.to_string()).collect()
    }
}

/// After search, ask the LLM to prune results not semantically relevant.
#[allow(dead_code)]
pub(super) fn semantic_filter_results(
    config: &ExtractionConfig,
    query: &str,
    results: &[TraversalResult],
) -> Result<Vec<TraversalResult>, String> {
    if results.is_empty() {
        return Ok(Vec::new());
    }

    // Build a compact summary of the search results (vertices + edges)
    let mut summary = String::new();
    for (i, r) in results.iter().enumerate() {
        match r {
            TraversalResult::VertexResult(v) => {
                let name = v.properties.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let labels = v.labels.join(", ");
                summary.push_str(&format!("{}. V:{} [{}]\n", i + 1, name, labels));
            }
            TraversalResult::EdgeResult(e) => {
                summary.push_str(&format!("{}. EDGE:{} ({})\n", i + 1, e.label, e.id));
            }
            _ => {}
        }
    }

    let system_prompt = "You are a semantic relevance filter. Given a user query and a list of search results, return ONLY the indices (comma-separated, 1-based) of results that are semantically relevant to the query. If none are relevant, return \"NONE\". No other text.";
    let user_msg = format!("Query: {}\n\nSearch results:\n{}", query, summary);

    match tokio::runtime::Handle::current().block_on(
        crate::extract::llm_client::chat_completion_with_retry(config, system_prompt, &user_msg)
    ) {
        Ok(reply) => {
            let text = reply.content.trim();
            if text == "NONE" {
                return Ok(Vec::new());
            }
            let indices: Vec<usize> = text.split(',')
                .filter_map(|s| s.trim().parse::<usize>().ok())
                .filter(|&i| i > 0 && i <= results.len())
                .map(|i| i - 1)
                .collect();
            if indices.is_empty() {
                // Fall back to returning all results
                return Ok(results.to_vec());
            }
            Ok(indices.into_iter().map(|i| results[i].clone()).collect())
        }
        Err(_) => Ok(results.to_vec()), // LLM failed, keep all results
    }
}

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
        name: v.name.clone(),
        keywords: v.keywords.clone(),
                document: v.document.clone(),
        labels: v.labels.clone(),
        properties: props,
    })
}

pub(super) fn edge_to_result(e: &crate::graph::Edge) -> TraversalResult {
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

/// Create a VertexResult from a Vertex snapshot (used by TimeTravel).
fn vertex_from_snapshot(v: &crate::graph::Vertex) -> TraversalResult {
    let props: std::collections::HashMap<String, Value> = v
        .properties
        .iter()
        .map(|(k, pv)| (k.clone(), property_to_json(pv)))
        .collect();

    TraversalResult::VertexResult(VertexResult {
        element_type: "vertex".to_string(),
        id: v.id,
        name: v.name.clone(),
        keywords: v.keywords.clone(),
                document: v.document.clone(),
        labels: v.labels.clone(),
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

pub(super) fn fill_vertex_details(g: &Graph, results: Vec<TraversalResult>) -> Result<Vec<TraversalResult>, String> {
    let filled: Vec<TraversalResult> = results
        .into_iter()
        .filter_map(|r| match r {
            TraversalResult::VertexResult(ref v) => {
                if let Some(vertex) = g.get_vertex(v.id) {
                    let props: std::collections::HashMap<String, Value> = vertex
                        .properties
                        .iter()
                        .map(|(k, pv)| (k.clone(), property_to_json(pv)))
                        .collect();
                    Some(TraversalResult::VertexResult(VertexResult {
                        labels: vertex.labels.clone(),
                        properties: props,
                        ..v.clone()
                    }))
                } else {
                    None
                }
            }
            _ => Some(r),
        })
        .collect();
    Ok(filled)
}

fn filter_by_property(
    _g: &Graph,
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

/// Filter results where a property does NOT match the given value.
fn filter_by_property_not(
    _g: &Graph,
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
            props.get(key).map_or(true, |pv| pv != value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Graph, PropertyValue};
    use crate::neuron::{NeuralNetwork, Neuron};

    fn setup_graph() -> Arc<Mutex<Graph>> {
        let mut g = Graph::new();
        let alice = g.create_vertex(vec!["person".into(), "engineer".into()]);
        let bob = g.create_vertex(vec!["person".into(), "scientist".into()]);
        let carol = g.create_vertex(vec!["person".into(), "designer".into()]);
        let project = g.create_vertex(vec!["project".into()]);

        // Set properties
        for (vid, name, age) in &[(alice, "Alice", 30), (bob, "Bob", 25), (carol, "Carol", 28)] {
            if let Some(v) = g.get_vertex_mut(*vid) {
                v.properties.insert("name".into(), PropertyValue::String(name.to_string()));
                v.properties.insert("age".into(), PropertyValue::Integer(*age));
            }
        }
        if let Some(v) = g.get_vertex_mut(project) {
            v.properties.insert("name".into(), PropertyValue::String("BionicGraph".into()));
        }

        // Add edges: Alice --knows--> Bob, Alice --knows--> Carol
        g.create_edge("knows".into(), alice, bob).unwrap();
        g.create_edge("knows".into(), alice, carol).unwrap();
        // Carol --works_at--> project
        g.create_edge("works_at".into(), carol, project).unwrap();

        Arc::new(Mutex::new(g))
    }

    fn setup_neural_network() -> Arc<Mutex<NeuralNetwork>> {
        let mut nn = NeuralNetwork::new();
        let n = Neuron::new(1, "person")
            .with_keywords(vec!["person".to_string(), "engineer".to_string()]);
        nn.add_neuron(n);
        Arc::new(Mutex::new(nn))
    }

    fn run_query(graph: &Arc<Mutex<Graph>>, nn: &Arc<Mutex<NeuralNetwork>>, steps: Vec<TraversalStep>) -> QueryResponse {
        let query = GremlinQuery::new(steps);
        execute_query(graph, nn, &query)
    }

    // ─── V / has / values basic pipeline ────────────────────

    #[test]
    fn test_v_all_vertices() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![TraversalStep::V { ids: vec![] }]);
        assert!(resp.success, "V() should succeed");
        assert_eq!(resp.data.len(), 4, "Should return all 4 vertices");
        // All should be vertex results
        for r in &resp.data {
            match r {
                TraversalResult::VertexResult(_) => {}
                _ => panic!("Expected VertexResult"),
            }
        }
    }

    #[test]
    fn test_v_specific_ids() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // First find IDs by running V() 
        let all = run_query(&g, &nn, vec![TraversalStep::V { ids: vec![] }]);
        let ids: Vec<u64> = all.data.iter().filter_map(|r| {
            if let TraversalResult::VertexResult(v) = r { Some(v.id) } else { None }
        }).collect();
        assert_eq!(ids.len(), 4);

        // Query specific vertices
        let resp = run_query(&g, &nn, vec![TraversalStep::V { ids: vec![ids[0], ids[1]] }]);
        assert_eq!(resp.data.len(), 2);
    }

    #[test]
    fn test_has_filter() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Has { key: "name".into(), value: serde_json::json!("Alice") },
        ]);
        assert_eq!(resp.data.len(), 1);
        if let TraversalResult::VertexResult(v) = &resp.data[0] {
            assert_eq!(v.properties["name"], "Alice");
        } else {
            panic!("Expected VertexResult");
        }
    }

    #[test]
    fn test_has_with_integer_value() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Has { key: "age".into(), value: serde_json::json!(30) },
        ]);
        assert_eq!(resp.data.len(), 1);
    }

    #[test]
    fn test_has_label() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::HasLabel { labels: vec!["project".into()] },
        ]);
        assert_eq!(resp.data.len(), 1);
        if let TraversalResult::VertexResult(v) = &resp.data[0] {
            assert_eq!(v.labels[0], "project");
        }
    }

    #[test]
    fn test_values_step() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Values { key: "name".into() },
        ]);
        assert!(resp.success);
        // Should return 4 name values
        assert_eq!(resp.data.len(), 4);
    }

    // ─── out / in / both traversal ──────────────────────────

    #[test]
    fn test_out_traverse() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // Get all V, then out("knows")
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Out { label: Some("knows".into()), depth: None },
        ]);
        assert!(resp.success);
        // Alice knows Bob and Carol → should find Bob and Carol
        assert_eq!(resp.data.len(), 2, "Should find 2 'knows' neighbors");
    }

    #[test]
    fn test_out_with_depth() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // Start from Alice, out("knows", depth=2):
        // Alice → knows → Bob, Carol
        // Carol → works_at → project
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Has { key: "name".into(), value: serde_json::json!("Alice") },
            TraversalStep::Out { label: None, depth: Some(2) },
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 3, "Depth-2 from Alice: Bob + Carol + project");
    }

    #[test]
    fn test_in_traverse() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // project has 1 incoming edge from Carol
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::HasLabel { labels: vec!["project".into()] },
            TraversalStep::In { label: None, depth: None },
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 1, "project should have 1 incoming neighbor (Carol)");
    }

    // ─── Count / dedup ───────────────────────────────────────

    #[test]
    fn test_count() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Count,
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 1);
        match &resp.data[0] {
            TraversalResult::CountResult(n) => assert_eq!(*n, 4),
            _ => panic!("Expected CountResult"),
        }
    }

    #[test]
    fn test_limit() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Limit { count: 2 },
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 2);
    }

    // ─── repeat ──────────────────────────────────────────────

    #[test]
    fn test_repeat_traverse() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // V → has(name="Alice") → repeat(times=2) { out() } 
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Has { key: "name".into(), value: serde_json::json!("Alice") },
            TraversalStep::Repeat {
                times: 2,
                steps: vec![
                    TraversalStep::Out { label: None, depth: None },
                ],
            },
        ]);
        assert!(resp.success);
        // Alice→ knows → Bob,Carol; Carol→ works_at → project
        // After 2 iterations we reach project
        assert!(resp.data.len() >= 1, "Should reach project after 2 hops");
    }

    // ─── hasText ─────────────────────────────────────────────

    #[test]
    fn test_has_text() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::HasText { key: "name".into(), pattern: "Ali".into() },
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 1, "Only Alice matches 'Ali'");
    }

    #[test]
    fn test_has_text_case_insensitive() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::HasText { key: "name".into(), pattern: "ali".into() },
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 1, "Should be case-insensitive");
    }

    // ─── dedup ───────────────────────────────────────────────

    #[test]
    fn test_dedup() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // both() from Alice → should return Bob (out) + Carol (out) (no incoming here)
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Has { key: "name".into(), value: serde_json::json!("Alice") },
            TraversalStep::Both { label: None, depth: None },
        ]);
        assert!(resp.success);
        // Alice has 2 outgoing edges to Bob and Carol
        assert_eq!(resp.data.len(), 2);
    }

    // ─── Error handling ──────────────────────────────────────

    #[test]
    fn test_empty_query_returns_empty() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![]);
        // Empty pipeline returns success with no data
        assert!(resp.success);
        assert!(resp.data.is_empty());
    }

    #[test]
    fn test_query_without_source_step_returns_empty() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // No V or E step first — pipeline starts empty
        let resp = run_query(&g, &nn, vec![
            TraversalStep::Limit { count: 5 },
        ]);
        // Pipeline succeeds but produces no data
        assert!(resp.success);
        assert!(resp.data.is_empty());
    }

    #[test]
    fn test_search_step() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![
            TraversalStep::Search { keywords: vec!["person".into()] },
        ]);
        // Search through neural network — may return results or empty
        assert!(resp.success, "Search should succeed");
        // The neural network has a "person" neuron linked to no vertices yet
        // So it may return empty — that's fine, just check no crash
    }

    // ─── E (edges) step ──────────────────────────────────────

    #[test]
    fn test_e_all_edges() {
        let g = setup_graph();
        let nn = setup_neural_network();
        let resp = run_query(&g, &nn, vec![
            TraversalStep::E { ids: vec![] },
        ]);
        assert!(resp.success);
        // We created 3 edges: knows(2) + works_at(1)
        assert_eq!(resp.data.len(), 3);
        for r in &resp.data {
            match r {
                TraversalResult::EdgeResult(_) => {}
                _ => panic!("Expected EdgeResult"),
            }
        }
    }

    #[test]
    fn test_e_specific_ids() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // Get all edge IDs first
        let all = run_query(&g, &nn, vec![TraversalStep::E { ids: vec![] }]);
        let eids: Vec<u64> = all.data.iter().filter_map(|r| {
            if let TraversalResult::EdgeResult(e) = r { Some(e.id) } else { None }
        }).collect();
        assert_eq!(eids.len(), 3);

        // Filter by specific edge ID
        let resp = run_query(&g, &nn, vec![
            TraversalStep::E { ids: vec![eids[0]] },
        ]);
        assert_eq!(resp.data.len(), 1);
    }

    // ─── TimeTravel step ──────────────────────────────────────
    //
    // NOTE: earlier bug: vertex_to_result(g, snapshot.id) re-read current state.
    // Fixed by vertex_from_snapshot(&snapshot). Test verifies historical values.

    #[test]
    fn test_time_travel_returns_historical_state() {
        // Create a time_travel-enabled graph
        let g = Arc::new(Mutex::new(Graph::new().with_time_travel()));
        let nn = Arc::new(Mutex::new(NeuralNetwork::new()));

        let vid;
        let t0;
        let t1;
        {
            let mut graph = g.lock().unwrap();
            vid = graph.create_vertex(vec!["test".into()]);

            // Set initial value via update_properties (pushes to history)
            let mut init = std::collections::HashMap::new();
            init.insert("name".into(), PropertyValue::String("original".into()));
            graph.get_vertex_mut(vid).unwrap().update_properties(init, true);

            std::thread::sleep(std::time::Duration::from_millis(2));
            t0 = crate::graph::vertex::now_micros();

            // First update — pushes "original" to history, sets "updated"
            let mut props = std::collections::HashMap::new();
            props.insert("name".into(), PropertyValue::String("updated".into()));
            graph.get_vertex_mut(vid).unwrap().update_properties(props, true);

            std::thread::sleep(std::time::Duration::from_millis(2));
            t1 = crate::graph::vertex::now_micros();

            // Second update — pushes "updated" to history, sets "final"
            let mut props = std::collections::HashMap::new();
            props.insert("name".into(), PropertyValue::String("final".into()));
            graph.get_vertex_mut(vid).unwrap().update_properties(props, true);
        }

        // Query at t0 (before first update) — should see "original"
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![vid] },
            TraversalStep::TimeTravel { at: serde_json::json!(t0) },
            TraversalStep::Values { key: "name".into() },
        ]);
        assert!(resp.success, "TimeTravel should succeed at t0");
        assert!(!resp.data.is_empty(), "Should find vertex at t0");
        if let TraversalResult::ValueResult(val) = &resp.data[0] {
            assert_eq!(*val, serde_json::json!("original"),
                "Should see 'original' at t0");
        }

        // Query at t1 (between updates) — should see "updated"
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![vid] },
            TraversalStep::TimeTravel { at: serde_json::json!(t1) },
            TraversalStep::Values { key: "name".into() },
        ]);
        assert!(resp.success, "TimeTravel should succeed at t1");
        assert!(!resp.data.is_empty(), "Should find vertex at t1");
        if let TraversalResult::ValueResult(val) = &resp.data[0] {
            assert_eq!(*val, serde_json::json!("updated"),
                "Should see 'updated' at t1");
        }

        // Query WITHOUT timeTravel — returns current "final"
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![vid] },
            TraversalStep::Values { key: "name".into() },
        ]);
        assert!(resp.success);
        if let TraversalResult::ValueResult(val) = &resp.data[0] {
            assert_eq!(*val, serde_json::json!("final"),
                "Current state should be 'final'");
        }
    }

    // ─── HasNot / HasKey / HasValue execution tests ───────────

    #[test]
    fn test_has_not_filter() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // hasNot(age=25) should return vertices where age != 25: Alice(30) and Carol(28)
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::HasNot { key: "age".into(), value: serde_json::json!(25) },
        ]);
        assert!(resp.success);
        // Bob(25) is excluded. Alice(30), Carol(28), and project (no age) pass.
        assert_eq!(resp.data.len(), 3, "Should exclude only Bob");
    }

    #[test]
    fn test_has_key_filter() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // hasKey("age") should return vertices that have an "age" property
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::HasKey { key: "age".into() },
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 3, "Only person vertices have age property");
    }

    #[test]
    fn test_has_value_filter() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // hasValue("Bob") should find vertices with any property = "Bob"
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::HasValue { value: serde_json::json!("Bob") },
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 1, "Only Bob has name = Bob");
    }

    // ─── OutE / InE / BothE execution tests ──────────────────

    #[test]
    fn test_out_e_from_vertex() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // Get all V, then outE("knows") from Alice
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Has { key: "name".into(), value: serde_json::json!("Alice") },
            TraversalStep::OutE { label: Some("knows".into()) },
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 2, "Alice has 2 outgoing knows edges");
        for r in &resp.data {
            match r {
                TraversalResult::EdgeResult(e) => assert_eq!(e.label, "knows"),
                _ => panic!("Expected EdgeResult"),
            }
        }
    }

    #[test]
    fn test_in_e_to_vertex() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // inE to project (Carol works_at project)
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::HasLabel { labels: vec!["project".into()] },
            TraversalStep::InE { label: None },
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 1, "project has 1 incoming edge from Carol");
    }

    #[test]
    fn test_both_e_from_vertex() {
        let g = setup_graph();
        let nn = setup_neural_network();
        // bothE from Alice: outgoing(2) + incoming(0) = 2
        let resp = run_query(&g, &nn, vec![
            TraversalStep::V { ids: vec![] },
            TraversalStep::Has { key: "name".into(), value: serde_json::json!("Alice") },
            TraversalStep::BothE { label: None },
        ]);
        assert!(resp.success);
        assert_eq!(resp.data.len(), 2, "Alice has 2 total incident edges");
    }
}
