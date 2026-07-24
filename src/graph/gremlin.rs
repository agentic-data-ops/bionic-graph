//! Gremlin pipeline step execution engine.
//!
//! Processes a sequence of `GremlinStep` values against a `Graph`,
//! producing a list of `GremlinResult` items.

use std::collections::HashMap;
use std::sync::Arc;

use crate::graph::crud;
use crate::graph::graph::Graph;
use crate::storage::memory_index::MetaPointer;
use crate::storage::types::{
    DataHeader, EdgePayload, PropertyValue, StorageResult, VertexPayload,
};

// ── Step definitions ────────────────────────────────────────────────────────

/// A single step in a Gremlin pipeline.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "step")]
pub enum GremlinStep {
    #[serde(rename = "search")]
    Search {
        /// 用户输入的原始文本，后端内置 tokenize 分词
        text: String,
        /// "greedy" | "exact"
        mode: Option<String>,
        /// 关键词匹配模式: "prefix"（前缀匹配）| "word"（分词精确匹配）
        match_mode: Option<String>,
        limit: Option<u32>,
        min_rank: Option<u32>,
    },
    #[serde(rename = "V")]
    V {
        ids: Option<Vec<u32>>,
        #[serde(default)]
        names: Option<Vec<String>>,
        /// Optional limit — when set, use rank index to fetch top-N vertices.
        #[serde(default)]
        limit: Option<u32>,
    },
    #[serde(rename = "E")]
    E {
        ids: Option<Vec<u32>>,
        #[serde(default)]
        names: Option<Vec<String>>,
        /// Optional limit — when set, use rank index to fetch top-N edges.
        #[serde(default)]
        limit: Option<u32>,
    },
    #[serde(rename = "has")]
    Has {
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "hasNot")]
    HasNot {
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "hasKey")]
    HasKey { key: String },
    #[serde(rename = "hasValue")]
    HasValue { value: serde_json::Value },
    #[serde(rename = "hasLabel")]
    HasLabel { label: String },
    #[serde(rename = "hasText")]
    HasText { text: String },
    #[serde(rename = "out")]
    Out {
        depth: Option<u8>,
        labels: Option<Vec<String>>,
    },
    #[serde(rename = "in")]
    In {
        depth: Option<u8>,
        labels: Option<Vec<String>>,
    },
    #[serde(rename = "both")]
    Both {
        depth: Option<u8>,
        labels: Option<Vec<String>>,
    },
    #[serde(rename = "outE")]
    OutE {
        labels: Option<Vec<String>>,
    },
    #[serde(rename = "inE")]
    InE {
        labels: Option<Vec<String>>,
    },
    #[serde(rename = "bothE")]
    BothE {
        labels: Option<Vec<String>>,
    },
    #[serde(rename = "values")]
    Values { keys: Option<Vec<String>> },
    #[serde(rename = "limit")]
    Limit { count: u32 },
    #[serde(rename = "count")]
    Count,
    #[serde(rename = "dedup")]
    Dedup,
    #[serde(rename = "repeat")]
    Repeat {
        steps: Vec<GremlinStep>,
        times: u8,
    },
    #[serde(rename = "expand")]
    Expand { depth: Option<u8>, label: Option<String> },
    #[serde(rename = "traverse")]
    Traverse {
        decay: Option<f32>,
        activate: Option<f32>,
        max_depth: Option<u8>,
        min_score: Option<f32>,
    },
    #[serde(rename = "rank")]
    Rank {
        limit: Option<u32>,
        /// Minimum rank threshold (inclusive).
        min: Option<u32>,
    },
}

/// A Gremlin query — a sequence of steps to execute.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GremlinQuery {
    pub steps: Vec<GremlinStep>,
}

// ── Result types ─────────────────────────────────────────────────────────────

/// A result item from a Gremlin pipeline step.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum GremlinResult {
    Vertex {
        #[serde(rename = "type")]
        element_type: String,
        id: u32,
        name: String,
        labels: Vec<String>,
        keywords: Vec<String>,
        properties: HashMap<String, PropertyValue>,
        score: Option<f32>,
    },
    Edge {
        #[serde(rename = "type")]
        element_type: String,
        id: u32,
        name: String,
        labels: Vec<String>,
        keywords: Vec<String>,
        source: u32,
        target: u32,
        strength: f32,
        properties: HashMap<String, PropertyValue>,
        score: Option<f32>,
    },
    Count {
        count: usize,
    },
}

/// The response returned from a Gremlin query.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GremlinResponse {
    pub success: bool,
    pub data: Vec<GremlinResult>,
    pub error: Option<String>,
}

impl GremlinResponse {
    pub fn success(data: Vec<GremlinResult>) -> Self {
        Self {
            success: true,
            data,
            error: None,
        }
    }

    pub fn error(msg: String) -> Self {
        Self {
            success: false,
            data: vec![],
            error: Some(msg),
        }
    }
}

// ── Convenience constructors ─────────────────────────────────────────────────

impl GremlinResult {
    pub fn from_vertex(id: u32, v: &VertexPayload, score: Option<f32>) -> Self {
        GremlinResult::Vertex {
            element_type: "vertex".to_string(),
            id,
            name: v.name.clone(),
            labels: v.labels.clone(),
            keywords: v.keywords.clone(),
            properties: v.properties.clone(),
            score,
        }
    }

    pub fn from_edge(id: u32, e: &EdgePayload, score: Option<f32>) -> Self {
        GremlinResult::Edge {
            element_type: "edge".to_string(),
            id,
            name: e.name.clone(),
            labels: e.labels.clone(),
            keywords: e.keywords.clone(),
            source: e.source,
            target: e.target,
            strength: e.strength,
            properties: e.properties.clone(),
            score,
        }
    }
}

// ── Pipeline execution ───────────────────────────────────────────────────────

/// Read a DataHeader from the data file at a given MetaPointer.
/// Used by the Gremlin engine to determine entity type/id from rank index pointers.
fn read_header_by_ptr(graph: &Graph, ptr: &MetaPointer) -> StorageResult<DataHeader> {
    let mut buf = [0u8; 64];
    {
        let cache = graph.block_cache.read().unwrap_or_else(|e| e.into_inner());
        if let Some(block) = cache.peek(ptr.block_idx) {
            let start = (ptr.chunk_offset as usize) * 64;
            buf.copy_from_slice(&block[start..start + 64]);
            return Ok(DataHeader::decode(&buf));
        }
    }
    let mut cache = graph.block_cache.write().unwrap_or_else(|e| e.into_inner());
    let block = cache.get_or_load(
        ptr.block_idx,
        |idx| graph.data_file.read_block(idx),
        &|idx, data| graph.data_file.write_block(idx, data).map_err(|e| e.into()),
    )?;
    let start = (ptr.chunk_offset as usize) * 64;
    buf.copy_from_slice(&block[start..start + 64]);
    Ok(DataHeader::decode(&buf))
}

/// Execute a complete Gremlin query against a graph.
/// `time_travel_at` comes from the X-Time-Travel header (None means present time).
pub fn execute(
    graph: &Arc<Graph>,
    query: &GremlinQuery,
    time_travel_at: Option<u64>,
) -> GremlinResponse {

    let mut current: Vec<GremlinResult> = Vec::new();

    let steps = &query.steps;
    let mut skip_next = false;
    for (i, step) in steps.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }

        // Peek ahead optimizations for source steps (no input).
        if current.is_empty() {
            // Check if next step is Count → shortcut via memory index.
            if let Some(next) = steps.get(i + 1) {
                if matches!(next, GremlinStep::Count) {
                    match step {
                        GremlinStep::V { ids, .. } => {
                            current = step_v_count(graph, ids.as_deref());
                            skip_next = true;
                            continue;
                        }
                        GremlinStep::E { ids, .. } => {
                            current = step_e_count(graph, ids.as_deref());
                            skip_next = true;
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            // Check if next step is Limit → propagate limit for early-break.
            let peek_limit = match step {
                GremlinStep::V { ids, names, limit } if ids.is_none() && names.is_none() && limit.is_none() => {
                    steps.get(i + 1).and_then(|next| {
                        if let GremlinStep::Limit { count } = next {
                            Some(*count)
                        } else {
                            None
                        }
                    })
                }
                GremlinStep::E { ids, names, limit } if ids.is_none() && names.is_none() && limit.is_none() => {
                    steps.get(i + 1).and_then(|next| {
                        if let GremlinStep::Limit { count } = next {
                            Some(*count)
                        } else {
                            None
                        }
                    })
                }
                _ => None,
            };

            let step = if let Some(limit) = peek_limit {
                match step {
                    GremlinStep::V { ids: _, names: _, limit: _ } => {
                        &GremlinStep::V { ids: None, names: None, limit: Some(limit) }
                    }
                    GremlinStep::E { ids: _, names: _, limit: _ } => {
                        &GremlinStep::E { ids: None, names: None, limit: Some(limit) }
                    }
                    _ => step,
                }
            } else {
                step
            };

            current = match execute_step(graph, step, current, time_travel_at) {
                Ok(results) => results,
                Err(e) => return GremlinResponse::error(format!("Step error: {}", e)),
            };
        } else {
            current = match execute_step(graph, step, current, time_travel_at) {
                Ok(results) => results,
                Err(e) => return GremlinResponse::error(format!("Step error: {}", e)),
            };
        }
    }

    GremlinResponse::success(current)
}

/// Optimized count for `V` — only reads from in-memory index, zero file I/O.
fn step_v_count(graph: &Arc<Graph>, ids: Option<&[u32]>) -> Vec<GremlinResult> {
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
    let count = match ids {
        Some(ids) => ids.iter().filter(|id| mi.vertices.contains(**id)).count(),
        None => mi.vertices.len(),
    };
    vec![GremlinResult::Count { count }]
}

/// Optimized count for `E` — only reads from in-memory index, zero file I/O.
fn step_e_count(graph: &Arc<Graph>, ids: Option<&[u32]>) -> Vec<GremlinResult> {
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
    let count = match ids {
        Some(ids) => ids.iter().filter(|id| mi.edges.contains(**id)).count(),
        None => mi.edges.len(),
    };
    vec![GremlinResult::Count { count }]
}

fn execute_step(
    graph: &Arc<Graph>,
    step: &GremlinStep,
    input: Vec<GremlinResult>,
    time_travel_at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    match step {
        GremlinStep::V { ids, names, limit } => step_v(graph, ids.as_deref(), names.as_deref(), *limit, time_travel_at),
        GremlinStep::E { ids, names, limit } => step_e(graph, ids.as_deref(), names.as_deref(), *limit, time_travel_at),
        GremlinStep::Search { text, mode, match_mode, limit, min_rank } => {
            step_search(graph, text, mode.as_deref(), match_mode.as_deref(), time_travel_at, *limit, *min_rank)
        }
        GremlinStep::Has { key, value } => step_has(input, key, value),
        GremlinStep::HasNot { key, value } => step_has_not(input, key, value),
        GremlinStep::HasKey { key } => step_has_key(input, key),
        GremlinStep::HasValue { value } => step_has_value(input, value),
        GremlinStep::HasLabel { label } => step_has_label(input, label),
        GremlinStep::HasText { text } => step_has_text(input, text),
        GremlinStep::Out { depth, labels } => step_out(graph, input, *depth, labels.as_deref(), time_travel_at),
        GremlinStep::In { depth, labels } => step_in(graph, input, *depth, labels.as_deref(), time_travel_at),
        GremlinStep::Both { depth, labels } => step_both(graph, input, *depth, labels.as_deref(), time_travel_at),
        GremlinStep::OutE { labels } => step_oute(graph, input, labels.as_deref(), time_travel_at),
        GremlinStep::InE { labels } => step_ine(graph, input, labels.as_deref(), time_travel_at),
        GremlinStep::BothE { labels } => step_bothe(graph, input, labels.as_deref(), time_travel_at),
        GremlinStep::Values { keys } => step_values(input, keys.as_deref()),
        GremlinStep::Limit { count } => step_limit(input, *count),
        GremlinStep::Count => step_count(input),
        GremlinStep::Dedup => step_dedup(input),
        GremlinStep::Repeat { steps, times } => step_repeat(graph, input, steps, *times),
        GremlinStep::Expand { depth, label } => step_expand(graph, input, *depth, label.as_deref(), time_travel_at),
        GremlinStep::Traverse { decay, activate, max_depth, min_score } => {
            step_traverse(graph, input, *decay, *activate, *max_depth, *min_score, time_travel_at)
        }
        GremlinStep::Rank { limit, min } => step_rank(graph, input, *limit, *min),
    }
}

// ── Step implementations ─────────────────────────────────────────────────────

fn step_v(
    graph: &Arc<Graph>,
    ids: Option<&[u32]>,
    names: Option<&[String]>,
    limit: Option<u32>,
    at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());

    if let Some(ids) = ids {
        // Specific IDs requested — collect meta, drop lock, then read.
        let mut candidates: Vec<(u32, MetaPointer)> = Vec::with_capacity(ids.len());
        for &vid in ids {
            if let Some(ptr) = mi.vertices.get(vid) {
                candidates.push((vid, *ptr));
            }
        }
        drop(mi);
        let mut results = Vec::with_capacity(candidates.len());
        for (vid, ptr) in candidates {
            if let Ok(Some(v)) = crud::read_vertex_by_ptr(graph, ptr, at) {
                results.push(GremlinResult::from_vertex(vid, &v, None));
            }
        }
        return Ok(results);
    }

    if let Some(names) = names {
        // Specific names requested — look up each name in vertex_names.
        let mut candidates: Vec<(u32, MetaPointer)> = Vec::with_capacity(names.len());
        for name in names {
            if let Some(&vid) = mi.vertex_names.get(name) {
                if let Some(ptr) = mi.vertices.get(vid) {
                    candidates.push((vid, *ptr));
                }
            }
        }
        drop(mi);
        let mut results = Vec::with_capacity(candidates.len());
        for (vid, ptr) in candidates {
            if let Ok(Some(v)) = crud::read_vertex_by_ptr(graph, ptr, at) {
                results.push(GremlinResult::from_vertex(vid, &v, None));
            }
        }
        return Ok(results);
    }

    // No specific IDs — iterate by rank or full scan.
    let limit = limit.unwrap_or(u32::MAX) as usize;

    if limit < u32::MAX as usize {
        // Use rank index descending to fetch top-N vertices.
        let ptrs = mi.ranks.top_pointers(limit, None);

        let mut results = Vec::with_capacity(limit.min(ptrs.len()));
        for ptr in &ptrs {
            // Read DataHeader to determine entity_id and type.
            if let Ok(dh) = read_header_by_ptr(graph, ptr) {
                if matches!(dh.chunk_type, crate::storage::types::ChunkType::Vertex) {
                    if let Ok(Some(v)) = crud::read_vertex_by_ptr(graph, *ptr, at) {
                        results.push(GremlinResult::from_vertex(dh.entity_id, &v, None));
                        if results.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }
        return Ok(results);
    }

    // Full scan (no practical limit) — iterate all vertices.
    let ids: Vec<u32> = mi.vertices.keys().copied().collect();
    let mut candidates: Vec<(u32, MetaPointer)> = Vec::with_capacity(ids.len());
    for vid in &ids {
        if let Some(ptr) = mi.vertices.get(*vid) {
            candidates.push((*vid, *ptr));
        }
    }
    drop(mi);

    let mut results = Vec::with_capacity(candidates.len());
    for (vid, ptr) in candidates {
        if let Ok(Some(v)) = crud::read_vertex_by_ptr(graph, ptr, at) {
            results.push(GremlinResult::from_vertex(vid, &v, None));
        }
    }
    Ok(results)
}

fn step_e(
    graph: &Arc<Graph>,
    ids: Option<&[u32]>,
    names: Option<&[String]>,
    limit: Option<u32>,
    at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());

    if let Some(ids) = ids {
        // Specific IDs requested — collect ptrs, drop lock, then read.
        let mut candidates: Vec<(u32, MetaPointer)> = Vec::with_capacity(ids.len());
        for &eid in ids {
            if let Some(ptr) = mi.edges.get(eid) {
                candidates.push((eid, *ptr));
            }
        }
        drop(mi);
        let mut results = Vec::with_capacity(candidates.len());
        for (eid, ptr) in candidates {
            if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, ptr, at) {
                results.push(GremlinResult::from_edge(eid, &e, None));
            }
        }
        return Ok(results);
    }

    if let Some(names) = names {
        // Specific names requested — look up each name in edge_names.
        let mut candidates: Vec<(u32, MetaPointer)> = Vec::with_capacity(names.len());
        for name in names {
            if let Some(&eid) = mi.edge_names.get(name) {
                if let Some(ptr) = mi.edges.get(eid) {
                    candidates.push((eid, *ptr));
                }
            }
        }
        drop(mi);
        let mut results = Vec::with_capacity(candidates.len());
        for (eid, ptr) in candidates {
            if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, ptr, at) {
                results.push(GremlinResult::from_edge(eid, &e, None));
            }
        }
        return Ok(results);
    }

    // No specific IDs — iterate by rank or full scan.
    let limit = limit.unwrap_or(u32::MAX) as usize;

    if limit < u32::MAX as usize {
        // Use rank index descending to fetch top-N edges.
        let ptrs = mi.ranks.top_pointers(limit, None);

        let mut results = Vec::with_capacity(limit.min(ptrs.len()));
        for ptr in &ptrs {
            // Read DataHeader to determine entity_id and type.
            if let Ok(dh) = read_header_by_ptr(graph, ptr) {
                if matches!(dh.chunk_type, crate::storage::types::ChunkType::Edge) {
                    if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, *ptr, at) {
                        results.push(GremlinResult::from_edge(dh.entity_id, &e, None));
                        if results.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }
        return Ok(results);
    }

    // Full scan (no practical limit) — iterate all edges.
    let ids: Vec<u32> = mi.edges.keys().copied().collect();
    let mut candidates: Vec<(u32, MetaPointer)> = Vec::with_capacity(ids.len());
    for eid in &ids {
        if let Some(ptr) = mi.edges.get(*eid) {
            candidates.push((*eid, *ptr));
        }
    }
    drop(mi);

    let mut results = Vec::with_capacity(candidates.len());
    for (eid, ptr) in candidates {
        if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, ptr, at) {
            results.push(GremlinResult::from_edge(eid, &e, None));
        }
    }
    Ok(results)
}

fn step_search(
    graph: &Arc<Graph>,
    text: &str,
    mode: Option<&str>,
    match_mode: Option<&str>,
    at: Option<u64>,
    limit: Option<u32>,
    _min_rank: Option<u32>,
) -> StorageResult<Vec<GremlinResult>> {
    let mode = mode.unwrap_or("greedy");
    let match_mode = match_mode.unwrap_or("prefix");
    let limit = limit.unwrap_or(100) as usize;

    // Tokenize the raw user text (frontend no longer tokenizes).
    let tokens: Vec<String> = crate::graph::tokenizer::Tokenizer::tokenize_query(text);

    if tokens.is_empty() {
        return Ok(vec![]);
    }

    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());

    // For each token, collect matching vertex/edge IDs per token.
    let mut token_vertex_matches: Vec<(String, Vec<u32>)> = Vec::new();
    let mut vertex_scores: HashMap<u32, f32> = HashMap::new();
    let mut edge_scores: HashMap<u32, f32> = HashMap::new();

    for token in &tokens {
        let mut vids_for_token = Vec::new();

        // Look up matching stored tokens based on match_mode.
        let matching_tokens: Vec<Vec<crate::storage::memory_index::MetaPointer>> = if match_mode == "word" {
            // Word mode: exact match on stored token (O(1) HashMap lookup)
            mi.tokens.get(token).map(|ptrs| vec![ptrs.clone()]).unwrap_or_default()
        } else {
            // Prefix mode: FST-backed prefix search (O(len(prefix) + M))
            mi.tokens.search_prefix(token)
                .into_iter()
                .map(|(_, ptrs)| ptrs)
                .collect()
        };

        for ptrs in &matching_tokens {
            for ptr in ptrs {
                // Read the DataHeader to get data_len, then read token by ptr.
                if let Ok(dh) = read_header_by_ptr(graph, ptr) {
                    if let Ok(Some(tpay)) = crud::read_token_by_ptr(graph, *ptr, dh.payload_len) {
                        for tref in &tpay.refs {
                            // Score from this ref (frequency weighting).
                            let score = tref.ref_frequency as f32;
                            if tref.ref_type == 0 {
                                vids_for_token.push(tref.ref_id);
                                *vertex_scores.entry(tref.ref_id).or_insert(0.0) += score;
                            } else {
                                *edge_scores.entry(tref.ref_id).or_insert(0.0) += score;
                            }
                        }
                    }
                }
            }
        }
        token_vertex_matches.push((token.clone(), vids_for_token));
    }

    // Determine which vertices to include.
    let include_vertices: Vec<u32> = if mode == "exact" {
        // Exact: only include vertices that matched EVERY token.
        if token_vertex_matches.is_empty() {
            vec![]
        } else {
            let mut common: Option<Vec<u32>> = None;
            for (_, vids) in &token_vertex_matches {
                if vids.is_empty() {
                    // A token matched nothing → empty result for exact mode.
                    common = Some(vec![]);
                    break;
                }
                let set: std::collections::HashSet<u32> = vids.iter().copied().collect();
                common = Some(match common.take() {
                    None => vids.clone(),
                    Some(c) => c.into_iter().filter(|id| set.contains(id)).collect(),
                });
            }
            common.unwrap_or_default()
        }
    } else {
        // Greedy: include all vertices that matched ANY token.
        vertex_scores.keys().copied().collect()
    };

    let mut results: Vec<GremlinResult> = Vec::new();

    // Helper: check if a token was actually valid in the payload at the query time.
    let token_valid_in_payload = |token: &str, name: &str, labels: &[String], keywords: &[String], properties: &HashMap<String, PropertyValue>| -> bool {
        name.to_lowercase().contains(token)
            || labels.iter().any(|l| l.to_lowercase().contains(token))
            || keywords.iter().any(|k| k.to_lowercase().contains(token))
            || properties.values().any(|pv| match pv {
                PropertyValue::String(s) => s.to_lowercase().contains(token),
                _ => false,
            })
    };

    // Process vertices — collect meta, drop lock, then read.
    let mut v_candidates: Vec<(u32, MetaPointer)> = Vec::new();
    for vid in &include_vertices {
        if let Some(ptr) = mi.vertices.get(*vid) {
            v_candidates.push((*vid, *ptr));
        }
    }

    // Process edges — collect ptrs, drop lock, then read.
    let mut e_candidates: Vec<(u32, MetaPointer, f32)> = Vec::new();
    for (eid, score) in &edge_scores {
        if let Some(ptr) = mi.edges.get(*eid) {
            e_candidates.push((*eid, *ptr, *score));
        }
    }
    drop(mi);

    let mut results = Vec::new();

    for (vid, ptr) in v_candidates {
        match crud::read_vertex_by_ptr(graph, ptr, at) {
            Ok(Some(v)) => {
                // Verify the payload at query time still matches the search token.
                if at.is_some()
                    && !tokens.iter().any(|t| token_valid_in_payload(t, &v.name, &v.labels, &v.keywords, &v.properties))
                {
                    // false positive — token was removed by this time
                } else {
                    let score = vertex_scores.get(&vid).copied().unwrap_or(0.0);
                    results.push(GremlinResult::from_vertex(vid, &v, Some(score)));
                }
            }
            Ok(None) => {}
            Err(e) => {
                log::debug!("search: vid={} error: {}", vid, e);
            }
        }
    }

    log::debug!("step_search: {} vtx_scores {} edge_scores {} include_vtx",
        vertex_scores.len(), edge_scores.len(), include_vertices.len());

    // Process edges (greedy only for now — exact mode for edges is analogous).
    for (eid, ptr, score) in e_candidates {
        if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, ptr, at) {
            if at.is_some()
                && !tokens.iter().any(|t| {
                    e.name.to_lowercase().contains(t)
                        || e.labels.iter().any(|l| l.to_lowercase().contains(t))
                        || e.keywords.iter().any(|k| k.to_lowercase().contains(t))
                        || e.properties.values().any(|pv| match pv {
                            PropertyValue::String(s) => s.to_lowercase().contains(t),
                            _ => false,
                        })
                }) {
                continue;
            }
            results.push(GremlinResult::from_edge(eid, &e, Some(score)));
        }
    }

    // Sort by score descending.
    results.sort_by(|a, b| {
        let sa = score_of(a);
        let sb = score_of(b);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    results.truncate(limit);
    Ok(results)
}

fn score_of(r: &GremlinResult) -> f32 {
    match r {
        GremlinResult::Vertex { score, .. } => score.unwrap_or(0.0),
        GremlinResult::Edge { score, .. } => score.unwrap_or(0.0),
        GremlinResult::Count { .. } => 0.0,
    }
}

// ── Filter steps ─────────────────────────────────────────────────────────────

fn step_has(
    input: Vec<GremlinResult>,
    key: &str,
    value: &serde_json::Value,
) -> StorageResult<Vec<GremlinResult>> {
    Ok(input
        .into_iter()
        .filter(|r| {
            let props = match r {
                GremlinResult::Vertex { properties, .. } => properties,
                GremlinResult::Edge { properties, .. } => properties,
                GremlinResult::Count { .. } => return false,
            };
            props.get(key).map_or(false, |pv| pv_matches(pv, value))
        })
        .collect())
}

fn step_has_not(
    input: Vec<GremlinResult>,
    key: &str,
    value: &serde_json::Value,
) -> StorageResult<Vec<GremlinResult>> {
    Ok(input
        .into_iter()
        .filter(|r| {
            let props = match r {
                GremlinResult::Vertex { properties, .. } => properties,
                GremlinResult::Edge { properties, .. } => properties,
                GremlinResult::Count { .. } => return false,
            };
            !props.get(key).map_or(false, |pv| pv_matches(pv, value))
        })
        .collect())
}

fn step_has_key(
    input: Vec<GremlinResult>,
    key: &str,
) -> StorageResult<Vec<GremlinResult>> {
    Ok(input
        .into_iter()
        .filter(|r| {
            let props = match r {
                GremlinResult::Vertex { properties, .. } => properties,
                GremlinResult::Edge { properties, .. } => properties,
                GremlinResult::Count { .. } => return false,
            };
            props.contains_key(key)
        })
        .collect())
}

fn step_has_value(
    input: Vec<GremlinResult>,
    value: &serde_json::Value,
) -> StorageResult<Vec<GremlinResult>> {
    Ok(input
        .into_iter()
        .filter(|r| {
            let props = match r {
                GremlinResult::Vertex { properties, .. } => properties,
                GremlinResult::Edge { properties, .. } => properties,
                GremlinResult::Count { .. } => return false,
            };
            props.values().any(|pv| pv_matches(pv, value))
        })
        .collect())
}

fn step_has_label(
    input: Vec<GremlinResult>,
    label: &str,
) -> StorageResult<Vec<GremlinResult>> {
    Ok(input
        .into_iter()
        .filter(|r| match r {
            GremlinResult::Vertex { labels, .. } => labels.iter().any(|l| l == label),
            GremlinResult::Edge { labels, .. } => labels.iter().any(|l| l == label),
            GremlinResult::Count { .. } => false,
        })
        .collect())
}

fn step_has_text(
    input: Vec<GremlinResult>,
    text: &str,
) -> StorageResult<Vec<GremlinResult>> {
    let lower = text.to_lowercase();
    Ok(input
        .into_iter()
        .filter(|r| match r {
            GremlinResult::Vertex {
                name, labels, keywords, properties, ..
            } => {
                name.to_lowercase().contains(&lower)
                    || labels.iter().any(|l| l.to_lowercase().contains(&lower))
                    || keywords.iter().any(|k| k.to_lowercase().contains(&lower))
                    || properties.values().any(|pv| pv_str(pv).to_lowercase().contains(&lower))
            }
            GremlinResult::Edge {
                name, labels, keywords, properties, ..
            } => {
                name.to_lowercase().contains(&lower)
                    || labels.iter().any(|l| l.to_lowercase().contains(&lower))
                    || keywords.iter().any(|k| k.to_lowercase().contains(&lower))
                    || properties.values().any(|pv| pv_str(pv).to_lowercase().contains(&lower))
            }
            GremlinResult::Count { .. } => false,
        })
        .collect())
}

// ── Traversal steps ──────────────────────────────────────────────────────────

fn step_out(
    graph: &Arc<Graph>,
    input: Vec<GremlinResult>,
    depth: Option<u8>,
    labels: Option<&[String]>,
    at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let max_depth = depth.unwrap_or(1) as usize;
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());

    // Collect all target vertex IDs discovered during BFS.
    let mut target_ids: Vec<u32> = Vec::new();
    for item in &input {
        let vid = match item {
            GremlinResult::Vertex { id, .. } => *id,
            _ => continue,
        };
        let mut visited = std::collections::HashSet::new();
        let mut frontier = vec![(vid, 0usize)];
        visited.insert(vid);

        while let Some((cur_id, cur_depth)) = frontier.pop() {
            if cur_depth >= max_depth {
                continue;
            }
            for (_eid, target, _ptr) in mi.adjacency.out_edges(cur_id) {
                let target_id = *target;
                if visited.insert(target_id) {
                    target_ids.push(target_id);
                    frontier.push((target_id, cur_depth + 1));
                }
            }
        }
    }

    // Collect ptrs for all target vertices, drop lock, then read.
    let mut candidates: Vec<(u32, MetaPointer)> = Vec::with_capacity(target_ids.len());
    for &tid in &target_ids {
        if let Some(ptr) = mi.vertices.get(tid) {
            candidates.push((tid, *ptr));
        }
    }
    drop(mi);

    let mut results = Vec::new();
    for (tid, ptr) in candidates {
        if let Ok(Some(v)) = crud::read_vertex_by_ptr(graph, ptr, at) {
            // Check label filter.
            if let Some(labels) = labels {
                if !v.labels.iter().any(|l| labels.contains(l)) {
                    continue;
                }
            }
            results.push(GremlinResult::from_vertex(tid, &v, None));
        }
    }
    Ok(results)
}

fn step_in(
    graph: &Arc<Graph>,
    input: Vec<GremlinResult>,
    depth: Option<u8>,
    labels: Option<&[String]>,
    at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let max_depth = depth.unwrap_or(1) as usize;
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());

    // Collect all source vertex IDs discovered during BFS.
    let mut source_ids: Vec<u32> = Vec::new();
    for item in &input {
        let vid = match item {
            GremlinResult::Vertex { id, .. } => *id,
            _ => continue,
        };
        let mut visited = std::collections::HashSet::new();
        let mut frontier = vec![(vid, 0usize)];
        visited.insert(vid);

        while let Some((cur_id, cur_depth)) = frontier.pop() {
            if cur_depth >= max_depth {
                continue;
            }
            for (_eid, source, _ptr) in mi.adjacency.in_edges(cur_id) {
                let source_id = *source;
                if visited.insert(source_id) {
                    source_ids.push(source_id);
                    frontier.push((source_id, cur_depth + 1));
                }
            }
        }
    }

    // Collect ptrs for all source vertices, drop lock, then read.
    let mut candidates: Vec<(u32, MetaPointer)> = Vec::with_capacity(source_ids.len());
    for &sid in &source_ids {
        if let Some(ptr) = mi.vertices.get(sid) {
            candidates.push((sid, *ptr));
        }
    }
    drop(mi);

    let mut results = Vec::new();
    for (sid, ptr) in candidates {
        if let Ok(Some(v)) = crud::read_vertex_by_ptr(graph, ptr, at) {
            if let Some(labels) = labels {
                if !v.labels.iter().any(|l| labels.contains(l)) {
                    continue;
                }
            }
            results.push(GremlinResult::from_vertex(sid, &v, None));
        }
    }
    Ok(results)
}

fn step_both(
    graph: &Arc<Graph>,
    input: Vec<GremlinResult>,
    depth: Option<u8>,
    labels: Option<&[String]>,
    at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let out = step_out(graph, input.clone(), depth, labels, at)?;
    let inp = step_in(graph, input, depth, labels, at)?;
    let mut combined: Vec<GremlinResult> = out.into_iter().chain(inp).collect();
    combined.sort_by_key(|r| match r {
        GremlinResult::Vertex { id, .. } => (0u8, *id),
        GremlinResult::Edge { id, .. } => (1u8, *id),
        GremlinResult::Count { .. } => (2u8, 0),
    });
    combined.dedup_by_key(|r| match r {
        GremlinResult::Vertex { id, .. } => (0u8, *id),
        GremlinResult::Edge { id, .. } => (1u8, *id),
        GremlinResult::Count { .. } => (2u8, 0),
    });
    Ok(combined)
}

// ── Edge traversal steps ─────────────────────────────────────────────────────

fn step_oute(
    graph: &Arc<Graph>,
    input: Vec<GremlinResult>,
    labels: Option<&[String]>,
    at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());

    // Collect ptrs for each out-edge, drop lock, then read.
    let mut candidates: Vec<(u32, MetaPointer)> = Vec::new();
    for item in &input {
        let vid = match item {
            GremlinResult::Vertex { id, .. } => *id,
            _ => continue,
        };
        for (eid, _target, _ptr) in mi.adjacency.out_edges(vid) {
            if let Some(ptr) = mi.edges.get(*eid) {
                candidates.push((*eid, *ptr));
            }
        }
    }
    drop(mi);

    let mut results = Vec::new();
    for (eid, ptr) in candidates {
        if let Some(labels) = labels {
            if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, ptr, at) {
                if !labels.iter().any(|l| e.labels.contains(l)) {
                    continue;
                }
                results.push(GremlinResult::from_edge(eid, &e, None));
            }
        } else if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, ptr, at) {
            results.push(GremlinResult::from_edge(eid, &e, None));
        }
    }
    Ok(results)
}

fn step_ine(
    graph: &Arc<Graph>,
    input: Vec<GremlinResult>,
    labels: Option<&[String]>,
    at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());

    // Collect ptrs for each in-edge, drop lock, then read.
    let mut candidates: Vec<(u32, MetaPointer)> = Vec::new();
    for item in &input {
        let vid = match item {
            GremlinResult::Vertex { id, .. } => *id,
            _ => continue,
        };
        for (eid, _source, _ptr) in mi.adjacency.in_edges(vid) {
            if let Some(ptr) = mi.edges.get(*eid) {
                candidates.push((*eid, *ptr));
            }
        }
    }
    drop(mi);

    let mut results = Vec::new();
    for (eid, ptr) in candidates {
        if let Some(labels) = labels {
            if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, ptr, at) {
                if !labels.iter().any(|l| e.labels.contains(l)) {
                    continue;
                }
                results.push(GremlinResult::from_edge(eid, &e, None));
            }
        } else if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, ptr, at) {
            results.push(GremlinResult::from_edge(eid, &e, None));
        }
    }
    Ok(results)
}

fn step_bothe(
    graph: &Arc<Graph>,
    input: Vec<GremlinResult>,
    labels: Option<&[String]>,
    at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let out = step_oute(graph, input.clone(), labels, at)?;
    let inp = step_ine(graph, input, labels, at)?;
    let mut combined: Vec<GremlinResult> = out.into_iter().chain(inp).collect();
    combined.sort_by_key(|r| match r {
        GremlinResult::Edge { id, .. } => *id,
        _ => 0,
    });
    combined.dedup_by_key(|r| match r {
        GremlinResult::Edge { id, .. } => *id,
        _ => 0,
    });
    Ok(combined)
}

// ── Result processing steps ──────────────────────────────────────────────────

fn step_values(
    input: Vec<GremlinResult>,
    keys: Option<&[String]>,
) -> StorageResult<Vec<GremlinResult>> {
    if let Some(keys) = keys {
        Ok(input
            .into_iter()
            .map(|r| match r {
                GremlinResult::Vertex {
                    id, name, labels, keywords, properties, score, ..
                } => {
                    let filtered: HashMap<String, PropertyValue> = properties
                        .into_iter()
                        .filter(|(k, _)| keys.contains(k))
                        .collect();
                    GremlinResult::Vertex {
                        element_type: "vertex".to_string(),
                        id,
                        name,
                        labels,
                        keywords,
                        properties: filtered,
                        score,
                    }
                }
                GremlinResult::Edge {
                    id, name, labels, keywords, source, target, strength, properties, score, ..
                } => {
                    let filtered: HashMap<String, PropertyValue> = properties
                        .into_iter()
                        .filter(|(k, _)| keys.contains(k))
                        .collect();
                    GremlinResult::Edge {
                        element_type: "edge".to_string(),
                        id,
                        name,
                        labels,
                        keywords: keywords.clone(),
                        source,
                        target,
                        strength,
                        properties: filtered,
                        score,
                    }
                }
                other => other,
            })
            .collect())
    } else {
        Ok(input)
    }
}

fn step_limit(
    input: Vec<GremlinResult>,
    count: u32,
) -> StorageResult<Vec<GremlinResult>> {
    Ok(input.into_iter().take(count as usize).collect())
}

fn step_count(input: Vec<GremlinResult>) -> StorageResult<Vec<GremlinResult>> {
    let count = input.len();
    Ok(vec![GremlinResult::Count { count }])
}

fn step_dedup(input: Vec<GremlinResult>) -> StorageResult<Vec<GremlinResult>> {
    let mut seen = std::collections::HashSet::new();
    Ok(input
        .into_iter()
        .filter(|r| {
            let key = match r {
                GremlinResult::Vertex { id, .. } => format!("v:{}", id),
                GremlinResult::Edge { id, .. } => format!("e:{}", id),
                GremlinResult::Count { .. } => "count".to_string(),
            };
            seen.insert(key)
        })
        .collect())
}

fn step_repeat(
    graph: &Arc<Graph>,
    input: Vec<GremlinResult>,
    steps: &[GremlinStep],
    times: u8,
) -> StorageResult<Vec<GremlinResult>> {
    let mut current = input;
    for _ in 0..times {
        let mut next = Vec::new();
        for item in current {
            let single = vec![item];
            let result = execute_step_chain(graph, steps, single, None)?;
            next.extend(result);
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    Ok(current)
}

fn execute_step_chain(
    graph: &Arc<Graph>,
    steps: &[GremlinStep],
    input: Vec<GremlinResult>,
    time_travel_at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let mut current = input;
    for step in steps {
        current = execute_step(graph, step, current, time_travel_at)?;
    }
    Ok(current)
}

// ── Expand step ──────────────────────────────────────────────────────────────

fn step_expand(
    graph: &Arc<Graph>,
    input: Vec<GremlinResult>,
    depth: Option<u8>,
    label: Option<&str>,
    at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let d = depth.unwrap_or(1);

    // Build label filter from the optional label string.
    let label_vec: Option<Vec<String>> = label.map(|l| vec![l.to_string()]);
    let label_filter: Option<&[String]> = label_vec.as_deref();

    // Include original input vertices.
    let mut results: Vec<GremlinResult> = input.clone();

    // Add out/in neighbors AND the connecting edges, filtered by label if given.
    let out_v = step_out(graph, input.clone(), Some(d), label_filter, at)?;
    let out_e = step_oute(graph, input.clone(), label_filter, at)?;
    let inp_v = step_in(graph, input.clone(), Some(d), label_filter, at)?;
    let inp_e = step_ine(graph, input, label_filter, at)?;

    for r in out_v.into_iter().chain(inp_v) {
        if matches!(r, GremlinResult::Vertex { .. }) {
            results.push(r);
        }
    }
    // Add edges.
    for r in out_e.into_iter().chain(inp_e) {
        if matches!(r, GremlinResult::Edge { .. }) {
            results.push(r);
        }
    }

    // Sort and dedup: use a compound key (type_priority, id) so that
    // vertices and edges with the same numeric id never collide.
    // Priority: vertex=0, edge=1, count=2.
    results.sort_by_key(|r| match r {
        GremlinResult::Vertex { id, .. } => (0u8, *id),
        GremlinResult::Edge { id, .. } => (1u8, *id),
        GremlinResult::Count { .. } => (2u8, 0),
    });
    results.dedup_by_key(|r| match r {
        GremlinResult::Vertex { id, .. } => (0u8, *id),
        GremlinResult::Edge { id, .. } => (1u8, *id),
        GremlinResult::Count { .. } => (2u8, 0),
    });
    Ok(results)
}

// ── Activate step (neuron-style activation) ──────────────────────────────────

fn step_traverse(
    graph: &Arc<Graph>,
    input: Vec<GremlinResult>,
    decay: Option<f32>,
    activate_threshold: Option<f32>,
    max_depth: Option<u8>,
    min_score: Option<f32>,
    at: Option<u64>,
) -> StorageResult<Vec<GremlinResult>> {
    let decay = decay.unwrap_or(1.0);
    let activate = activate_threshold.unwrap_or(0.0);
    let max_depth = max_depth.unwrap_or(1) as usize;
    let min_score = min_score.unwrap_or(0.0);

    let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());

    // Seed: input vertices get score = 1.0.
    // If input contains edges, their endpoints also get score = 1.0.
    let mut scored: Vec<(u32, f32)> = Vec::new();
    for item in &input {
        if let GremlinResult::Vertex { id, .. } = item {
            scored.push((*id, 1.0));
        }
        if let GremlinResult::Edge { source, target, .. } = item {
            scored.push((*source, 1.0));
            scored.push((*target, 1.0));
        }
    }

    // Collect all adjacency edges reachable from seeds into local structures,
    // along with edge ptrs+metas for strength lookups.
    let mut adjacency_out: HashMap<u32, Vec<(u32, u32, MetaPointer)>> = HashMap::new();
    let mut adjacency_in: HashMap<u32, Vec<(u32, u32, MetaPointer)>> = HashMap::new();
    let mut edge_str_map: HashMap<u32, MetaPointer> = HashMap::new();

    for &(vid, _) in &scored {
        let out: Vec<(u32, u32, MetaPointer)> = mi.adjacency.out_edges(vid)
            .iter().map(|&(eid, target, ptr)| (eid, target, ptr))
            .collect();
        if !out.is_empty() {
            adjacency_out.insert(vid, out);
        }
        let inp: Vec<(u32, u32, MetaPointer)> = mi.adjacency.in_edges(vid)
            .iter().map(|&(eid, source, ptr)| (eid, source, ptr))
            .collect();
        if !inp.is_empty() {
            adjacency_in.insert(vid, inp);
        }
    }

    // Collect edge ptrs for all reachable edges.
    for (&eid, &ptr) in mi.edges.iter() {
        edge_str_map.insert(eid, ptr);
    }
    drop(mi);

    // Read all edge strengths.
    let mut edge_strengths: HashMap<u32, f32> = HashMap::new();
    for (&eid, &ptr) in &edge_str_map {
        let strength = if let Ok(Some(epay)) = crud::read_edge_by_ptr(graph, ptr, at) {
            epay.strength
        } else {
            1.0
        };
        edge_strengths.insert(eid, strength);
    }

    // BFS-style activation spreading using local adjacency data.
    let mut results: Vec<(u32, f32)> = scored.clone();
    let mut visited: std::collections::HashMap<u32, f32> = HashMap::new();
    for (id, score) in &scored {
        visited.insert(*id, *score);
    }

    // Track edges traversed during BFS (source, target, edge_id).
    let mut traversed_edges: Vec<(u32, u32, u32)> = Vec::new();

    let mut frontier: Vec<(u32, f32, usize)> = scored.into_iter().map(|(id, s)| (id, s, 0)).collect();
    let mut front_idx = 0;

    while front_idx < frontier.len() {
        let (cur_id, cur_score, cur_depth) = frontier[front_idx];
        front_idx += 1;

        if cur_depth >= max_depth {
            continue;
        }

        // Spread to outgoing neighbors via edges.
        let out_list: Vec<_> = adjacency_out.get(&cur_id).cloned().unwrap_or_default();
        for &(eid, target, _eptr) in &out_list {
            let edge_strength = edge_strengths.get(&eid).copied().unwrap_or(1.0);
            let new_score = cur_score * decay * edge_strength;
            if new_score < activate {
                continue;
            }

            traversed_edges.push((cur_id, target, eid));

            let prev = visited.entry(target).or_insert(0.0);
            if new_score > *prev {
                *prev = new_score;
                if new_score >= min_score {
                    results.push((target, new_score));
                }
                frontier.push((target, new_score, cur_depth + 1));

                // Collect adjacency for newly discovered vertex.
                if !adjacency_out.contains_key(&target) {
                    // Re-acquire mi temporarily to collect more adjacency.
                    let mi2 = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
                    let out2: Vec<(u32, u32, MetaPointer)> = mi2.adjacency.out_edges(target)
                        .iter().map(|&(e, t, p)| (e, t, p))
                        .collect();
                    if !out2.is_empty() {
                        adjacency_out.insert(target, out2);
                    }
                    let in2: Vec<(u32, u32, MetaPointer)> = mi2.adjacency.in_edges(target)
                        .iter().map(|&(e, s, p)| (e, s, p))
                        .collect();
                    if !in2.is_empty() {
                        adjacency_in.insert(target, in2);
                    }
                    drop(mi2);
                }
            }
        }

        // Spread to incoming neighbors via edges.
        let in_list: Vec<_> = adjacency_in.get(&cur_id).cloned().unwrap_or_default();
        for &(eid, source, _eptr) in &in_list {
                let edge_strength = edge_strengths.get(&eid).copied().unwrap_or(1.0);
                let new_score = cur_score * decay * edge_strength;
                if new_score < activate {
                    continue;
                }

                traversed_edges.push((source, cur_id, eid));

                let prev = visited.entry(source).or_insert(0.0);
                if new_score > *prev {
                    *prev = new_score;
                    if new_score >= min_score {
                        results.push((source, new_score));
                    }
                    frontier.push((source, new_score, cur_depth + 1));

                    // Collect adjacency for newly discovered vertex.
                    if !adjacency_out.contains_key(&source) {
                        let mi2 = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
                        let out2: Vec<(u32, u32, MetaPointer)> = mi2.adjacency.out_edges(source)
                            .iter().map(|&(e, t, p)| (e, t, p))
                            .collect();
                        if !out2.is_empty() {
                            adjacency_out.insert(source, out2);
                        }
                        let in2: Vec<(u32, u32, MetaPointer)> = mi2.adjacency.in_edges(source)
                            .iter().map(|&(e, s, p)| (e, s, p))
                            .collect();
                        if !in2.is_empty() {
                            adjacency_in.insert(source, in2);
                        }
                        drop(mi2);
                    }
                }
            }
    }

    // Collect non-vertex results (edges from search) to preserve them.
    let mut gremlin_results: Vec<GremlinResult> = input.into_iter().filter(|r| {
        !matches!(r, GremlinResult::Vertex { .. })
    }).collect();

    // Dedup and collect vertex results.
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results.dedup_by_key(|(id, _)| *id);

    // Collect vertex ptrs and read vertices.
    let mi3 = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
    let mut v_candidates: Vec<(u32, MetaPointer, f32)> = Vec::new();
    for (vid, score) in &results {
        if let Some(ptr) = mi3.vertices.get(*vid) {
            v_candidates.push((*vid, *ptr, *score));
        }
    }

    // Collect edge candidates for traversed edges.
    let mut e_candidates: Vec<(u32, u32, u32, MetaPointer)> = Vec::new();
    traversed_edges.sort();
    traversed_edges.dedup();
    for (src, tgt, eid) in &traversed_edges {
        let src_score = visited.get(src).copied().unwrap_or(0.0);
        let tgt_score = visited.get(tgt).copied().unwrap_or(0.0);
        if src_score >= min_score && tgt_score >= min_score {
            if let Some(ptr) = mi3.edges.get(*eid) {
                e_candidates.push((*src, *tgt, *eid, *ptr));
            }
        }
    }
    drop(mi3);

    for (vid, ptr, score) in v_candidates {
        if let Ok(Some(v)) = crud::read_vertex_by_ptr(graph, ptr, at) {
            gremlin_results.push(GremlinResult::from_vertex(vid, &v, Some(score)));
        }
    }

    for (src, tgt, eid, ptr) in e_candidates {
        let src_score = visited.get(&src).copied().unwrap_or(0.0);
        let tgt_score = visited.get(&tgt).copied().unwrap_or(0.0);
        if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, ptr, at) {
            let edge_score = (src_score + tgt_score) / 2.0;
            gremlin_results.push(GremlinResult::from_edge(eid, &e, Some(edge_score)));
        }
    }

    Ok(gremlin_results)
}

/// Gremlin `rank` step — return top results by rank.
///
/// As a source step (empty input): iterate rank index descending.
/// As a filter step: sort existing results by rank.
fn step_rank(
    graph: &Arc<Graph>,
    input: Vec<GremlinResult>,
    limit: Option<u32>,
    min_rank: Option<u32>,
) -> StorageResult<Vec<GremlinResult>> {
    let min = min_rank.unwrap_or(0);
    let limit = limit.unwrap_or(u32::MAX) as usize;

    if input.is_empty() {
        // Source mode: iterate rank index descending.
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        let ptrs = mi.ranks.top_pointers(limit, Some(min));

        let mut results = Vec::new();
        for ptr in &ptrs {
            if results.len() >= limit {
                break;
            }
            // Read DataHeader to determine entity type and id, and check rank.
            if let Ok(dh) = read_header_by_ptr(graph, ptr) {
                if dh.rank >= min {
                    if matches!(dh.chunk_type, crate::storage::types::ChunkType::Vertex) {
                        if let Ok(Some(v)) = crud::read_vertex_by_ptr(graph, *ptr, None) {
                            results.push(GremlinResult::from_vertex(dh.entity_id, &v, None));
                            continue;
                        }
                    }
                    if results.len() < limit {
                        if matches!(dh.chunk_type, crate::storage::types::ChunkType::Edge) {
                            if let Ok(Some(e)) = crud::read_edge_by_ptr(graph, *ptr, None) {
                                results.push(GremlinResult::from_edge(dh.entity_id, &e, None));
                            }
                        }
                    }
                }
            }
        }
        Ok(results)
    } else {
        // Filter mode: rank-sort existing results by reading DataHeaders.
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        let mut ranked: Vec<(u32, GremlinResult)> = Vec::new();

        for item in input {
            let id = match item {
                GremlinResult::Vertex { id, .. } => id,
                GremlinResult::Edge { id, .. } => id,
                _ => continue,
            };
            let ptr = match &item {
                GremlinResult::Vertex { .. } => mi.vertices.get(id).copied(),
                GremlinResult::Edge { .. } => mi.edges.get(id).copied(),
                _ => None,
            };
            let rank = if let Some(ptr) = ptr {
                if let Ok(dh) = read_header_by_ptr(graph, &ptr) {
                    dh.rank
                } else {
                    0
                }
            } else {
                0
            };
            if rank >= min {
                ranked.push((rank, item));
            }
        }

        ranked.sort_by(|a, b| b.0.cmp(&a.0));
        ranked.truncate(limit);

        Ok(ranked.into_iter().map(|(_, r)| r).collect())
    }
}

// ── Property value helpers ───────────────────────────────────────────────────

fn pv_matches(pv: &PropertyValue, json_val: &serde_json::Value) -> bool {
    match (pv, json_val) {
        (PropertyValue::String(s), serde_json::Value::String(j)) => s == j,
        (PropertyValue::Integer(i), serde_json::Value::Number(n)) => {
            n.as_i64().map_or(false, |n| *i == n)
        }
        (PropertyValue::Float(f), serde_json::Value::Number(n)) => {
            n.as_f64().map_or(false, |n| (*f - n).abs() < f64::EPSILON)
        }
        (PropertyValue::Boolean(b), serde_json::Value::Bool(j)) => *b == *j,
        _ => false,
    }
}

fn pv_str(pv: &PropertyValue) -> String {
    match pv {
        PropertyValue::String(s) => s.clone(),
        PropertyValue::Integer(i) => i.to_string(),
        PropertyValue::Float(f) => f.to_string(),
        PropertyValue::Boolean(b) => b.to_string(),
        PropertyValue::List(l) => format!("{:?}", l),
        PropertyValue::Null => "null".to_string(),
    }
}

/// Entry point for executing a Gremlin query.
/// This is the public API used by the REST layer.
pub fn execute_gremlin(graph: &Arc<Graph>, query: &GremlinQuery, time_travel_at: Option<u64>) -> GremlinResponse {
    execute(graph, query, time_travel_at)
}
