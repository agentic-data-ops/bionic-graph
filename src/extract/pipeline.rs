use std::collections::{HashMap, HashSet};

use serde::Serialize;

use std::sync::Arc;
use std::sync::Mutex;

use crate::graph::{Graph, PropertyValue, VertexId};
use crate::memory_system::MemorySystem;

use super::config::{ExtractionConfig, ExtractedEntity, ExtractedRelation};
use super::document::{ensure_fits_budget, read_markdown, split_sections, Section};
use super::extraction::{build_batch_user_message, build_user_message, parse_batch_response, parse_response, SYSTEM_PROMPT};
use super::llm_client::chat_completion_with_retry;

/// Callback for reporting extraction progress.
/// Arguments: (processed_sections, total_sections, current_heading)
pub type ProgressCallback = Option<Arc<dyn Fn(usize, usize, &str) + Send + Sync>>;

/// Statistics from a document extraction run.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ExtractionStats {
    pub total_sections: usize,
    pub processed_sections: usize,
    pub total_entities: usize,
    pub total_relations: usize,
    pub new_vertices: usize,
    pub new_edges: usize,
    pub total_prompt_tokens: u32,
    pub total_completion_tokens: u32,
    pub sections_in_graph: usize,
    pub paragraphs_in_graph: usize,
}

/// Run extraction on a Markdown file on disk (works with MemorySystem).
pub async fn extract_document(
    config: &ExtractionConfig,
    file_path: &str,
    memory: &MemorySystem,
) -> Result<ExtractionStats, String> {
    let sections = read_markdown(file_path)?;
    extract_sections_graph(config, &sections, &memory.graph, &memory.neural_network).await
}

/// Run extraction on raw Markdown content (works with MemorySystem).
pub async fn extract_content(
    config: &ExtractionConfig,
    content: &str,
    source_name: &str,
    memory: &MemorySystem,
) -> Result<ExtractionStats, String> {
    log::info!("Extracting knowledge from: {}", source_name);
    let sections = split_sections(content)
        .map_err(|e| format!("Failed to parse '{}': {}", source_name, e))?;
    extract_sections_graph(config, &sections, &memory.graph, &memory.neural_network).await
}

/// Same as `extract_content` but takes raw `Arc<Mutex<Graph>>` — for HTTP handlers.
pub async fn extract_content_raw(
    config: &ExtractionConfig,
    content: &str,
    source_name: &str,
    graph: &Arc<Mutex<Graph>>,
) -> Result<ExtractionStats, String> {
    log::info!("Extracting knowledge from: {}", source_name);
    let sections = split_sections(content)
        .map_err(|e| format!("Failed to parse '{}': {}", source_name, e))?;
    extract_sections_core(config, &sections, graph, None, None).await
}

/// Same as `extract_content_raw` but with neural network for auto-synapse.
pub async fn extract_content_raw_with_nn(
    config: &ExtractionConfig,
    content: &str,
    source_name: &str,
    graph: &Arc<Mutex<Graph>>,
    neural: &Arc<Mutex<crate::neuron::NeuralNetwork>>,
) -> Result<ExtractionStats, String> {
    extract_content_raw_with_nn_and_progress(config, content, source_name, graph, neural, None).await
}

/// Like `extract_content_raw_with_nn` but accepts a progress callback.
pub async fn extract_content_raw_with_nn_and_progress(
    config: &ExtractionConfig,
    content: &str,
    source_name: &str,
    graph: &Arc<Mutex<Graph>>,
    neural: &Arc<Mutex<crate::neuron::NeuralNetwork>>,
    on_progress: ProgressCallback,
) -> Result<ExtractionStats, String> {
    log::info!("Extracting knowledge from: {}", source_name);
    let sections = split_sections(content)
        .map_err(|e| format!("Failed to parse '{}': {}", source_name, e))?;
    extract_sections_core(config, &sections, graph, Some(neural), on_progress).await
}

// ─── Shared processing core ──────────────────────────────────────

/// Core extraction loop using MemorySystem (for convenience API).
async fn extract_sections_graph(
    config: &ExtractionConfig,
    sections: &[Section],
    graph: &Arc<Mutex<Graph>>,
    neural: &Arc<Mutex<crate::neuron::NeuralNetwork>>,
) -> Result<ExtractionStats, String> {
    extract_sections_core(config, sections, graph, Some(neural), None).await
}

/// Core extraction loop — the shared engine.
/// Processes sections concurrently using a semaphore limited by `config.concurrent_sections`.
async fn extract_sections_core(
    config: &ExtractionConfig,
    sections: &[Section],
    graph: &Arc<Mutex<Graph>>,
    neural: Option<&Arc<Mutex<crate::neuron::NeuralNetwork>>>,
    on_progress: ProgressCallback,
) -> Result<ExtractionStats, String> {
    let max_tokens = config.section_token_budget();
    log::info!(
        "Document split into {} sections (max {} tokens/section, {} concurrent)",
        sections.len(),
        max_tokens,
        config.concurrent_sections,
    );

    // Fit sections to token budget
    let mut fitted = Vec::new();
    for section in sections {
        fitted.extend(ensure_fits_budget(section, max_tokens));
    }
    let total_fitted = fitted.len();
    log::info!("After fitting: {} sections to process", total_fitted);

    let mut section_id_to_vid: HashMap<String, VertexId> = HashMap::new();
    let mut stats = ExtractionStats {
        total_sections: sections.len(),
        ..Default::default()
    };

    // Step 1: Insert all sections as vertices with hierarchical edges (sequential, fast)
    let section_vids = insert_section_hierarchy(graph, sections);
    for (heading, vid) in &section_vids {
        section_id_to_vid.insert(heading.clone(), *vid);
        stats.sections_in_graph += 1;
    }
    stats.new_vertices += section_vids.len();

    // Step 2: Process sections in batches (batch_size per LLM call)
    let total = Arc::new(total_fitted);
    let processed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(config.concurrent_sections.max(1)));
    let graph = Arc::clone(graph);
    let neural = neural.map(Arc::clone);

    // Shared state for deduplication (wrapped in Arc<Mutex<>> for concurrent access)
    let seen_entity_ids = Arc::new(Mutex::new(HashSet::<String>::new()));
    let entity_id_to_vid = Arc::new(Mutex::new(HashMap::<String, VertexId>::new()));
    let seen_relation_keys = Arc::new(Mutex::new(HashSet::<(String, String, String)>::new()));
    let section_id_to_vid = Arc::new(section_id_to_vid);

    // Shared aggregated stats
    let final_stats = Arc::new(Mutex::new(ExtractionStats {
        total_sections: sections.len(),
        sections_in_graph: stats.sections_in_graph,
        new_vertices: stats.new_vertices,
        ..Default::default()
    }));

    // For pass_section_context: track latest summary across batches
    let latest_summary = Arc::new(Mutex::new(None::<String>));

    // Group fitted sections into batches
    let batch_size = config.batch_size.max(1);
    let mut batches: Vec<Vec<(usize, Section)>> = Vec::new();
    for (idx, section) in fitted.into_iter().enumerate() {
        if batches.is_empty() || batches.last().unwrap().len() >= batch_size {
            batches.push(Vec::new());
        }
        let batch_idx = batches.len() - 1;
        batches[batch_idx].push((idx, section));
    }

    let num_batches = batches.len();
    log::info!(
        "Processing {} sections in {} batches (batch_size={}, {} concurrent)",
        total_fitted, num_batches, batch_size, config.concurrent_sections,
    );

    // Spawn each batch as a concurrent task
    let mut handles = Vec::with_capacity(num_batches);
    for batch in batches {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let config = config.clone();
        let graph = Arc::clone(&graph);
        let neural = neural.clone();
        let seen_entity_ids = Arc::clone(&seen_entity_ids);
        let entity_id_to_vid = Arc::clone(&entity_id_to_vid);
        let seen_relation_keys = Arc::clone(&seen_relation_keys);
        let section_id_to_vid = Arc::clone(&section_id_to_vid);
        let final_stats = Arc::clone(&final_stats);
        let processed = Arc::clone(&processed);
        let total = Arc::clone(&total);
        let on_progress = on_progress.clone();
        let latest_summary = Arc::clone(&latest_summary);
        let pass_context = config.pass_section_context;

        handles.push(tokio::spawn(async move {
            let _permit = permit; // held for the duration of this batch
            let batch = batch; // owned by this closure

            // Build references for prompt
            let batch_refs: Vec<(usize, &Section)> = batch.iter().enumerate()
                .map(|(i, (_, s))| (i, s))
                .collect();

            // Report first section of batch as current heading
            let first_heading = batch_refs.first().map(|(_, s)| s.heading.as_str()).unwrap_or("");

            // Read latest summary for context
            let summary_for_msg = if pass_context {
                latest_summary.lock().unwrap().clone()
            } else {
                None
            };

            let batch_start = std::time::Instant::now();
            let user_msg = build_batch_user_message(&batch_refs, summary_for_msg.as_deref());

            log::info!(
                "Processing batch [{} sections, starting with: {}]",
                batch_refs.len(),
                first_heading,
            );

            let llm_start = std::time::Instant::now();
            let llm_result = chat_completion_with_retry(&config, SYSTEM_PROMPT, &user_msg).await;
            let llm_elapsed = llm_start.elapsed();

            let extractions = match llm_result {
                Ok(result) => {
                    log::info!(
                        "[TIMING] Batch LLM call ({} sections): {:.2}s ({} prompt + {} completion tokens)",
                        batch_refs.len(), llm_elapsed.as_secs_f64(),
                        result.prompt_tokens, result.completion_tokens,
                    );
                    // Update token stats
                    {
                        let mut s = final_stats.lock().unwrap();
                        s.total_prompt_tokens += result.prompt_tokens;
                        s.total_completion_tokens += result.completion_tokens;
                    }

                    match parse_batch_response(&result.content, &batch_refs) {
                        Ok(exs) => exs,
                        Err(e) => {
                            log::error!("Parse error in batch '{}': {}. Falling back to per-section extraction.", first_heading, e);
                            // Fallback: process each section individually
                            let mut fallback = Vec::new();
                            for (bi, (_, sec)) in batch_refs.iter().enumerate() {
                                let user_msg = build_user_message(sec, None);
                                match chat_completion_with_retry(&config, SYSTEM_PROMPT, &user_msg).await {
                                    Ok(r) => {
                                        let mut s = final_stats.lock().unwrap();
                                        s.total_prompt_tokens += r.prompt_tokens;
                                        s.total_completion_tokens += r.completion_tokens;
                                        drop(s);
                                        match parse_response(&sec.heading, &r.content) {
                                            Ok(ex) => fallback.push(ex),
                                            Err(e2) => log::error!("Fallback parse error for '{}': {}", sec.heading, e2),
                                        }
                                    }
                                    Err(e2) => log::error!("Fallback LLM error for '{}': {}", sec.heading, e2),
                                }
                            }
                            fallback
                        }
                    }
                }
                Err(e) => {
                    log::error!("LLM call failed on batch '{}' (took {:.2}s): {}", first_heading, llm_elapsed.as_secs_f64(), e);
                    return;
                }
            };

            // Apply each extraction to the graph (under mutex)
            let graph_start = std::time::Instant::now();
            let mut s = final_stats.lock().unwrap();

            for (i, extraction) in extractions.into_iter().enumerate() {
                // Get the corresponding section for this extraction
                let section = match batch.get(i) {
                    Some((_, sec)) => sec,
                    None => continue,
                };
                let heading = &section.heading;

                // Update latest summary for subsequent batches
                if pass_context && !extraction.summary.is_empty() {
                    let mut summary = latest_summary.lock().unwrap();
                    if summary.is_none() {
                        *summary = Some(extraction.summary.clone());
                    }
                }

                // Insert paragraphs for this section
                let para_ids = insert_paragraphs_for_section(&graph, section, &*section_id_to_vid);
                s.paragraphs_in_graph += para_ids.len();
                s.new_vertices += para_ids.len();

                // Deduplicate and insert entities into graph
                let mut local_eid_to_vid: HashMap<String, VertexId> = HashMap::new();
                {
                    let mut seen = seen_entity_ids.lock().unwrap();
                    let mut eid_to_vid = entity_id_to_vid.lock().unwrap();
                    for entity in &extraction.entities {
                        if seen.insert(entity.id.clone()) {
                            s.total_entities += 1;
                            if let Ok(vid) = insert_entity_to_graph(&graph, entity) {
                                eid_to_vid.insert(entity.id.clone(), vid);
                                local_eid_to_vid.insert(entity.id.clone(), vid);
                                s.new_vertices += 1;

                                // Create a neuron for this entity so it's searchable by name
                                if let Some(ref nn) = neural {
                                    if let Ok(mut nn) = nn.lock() {
                                        let nid = (nn.neuron_count() as u64) + 1;
                                        let label = entity.labels.first().cloned().unwrap_or_else(|| "entity".to_string());
                                        let mut neuron = crate::neuron::Neuron::for_vertex(nid, &label, vid);
                                        // Use the entity's ID and labels as searchable keywords
                                        let mut keywords = vec![entity.id.clone()];
                                        keywords.extend(entity.labels.clone());
                                        neuron.keywords = keywords;
                                        nn.add_neuron(neuron);
                                    }
                                }
                            }
                        } else if let Some(&vid) = eid_to_vid.get(&entity.id) {
                            local_eid_to_vid.insert(entity.id.clone(), vid);
                        }
                    }
                }

                // Insert relations as edges (and create searchable edge neurons)
                {
                    let eid_to_vid = entity_id_to_vid.lock().unwrap();
                    let mut seen_rels = seen_relation_keys.lock().unwrap();
                    for relation in &extraction.relations {
                        let key = (
                            relation.source.clone(),
                            relation.target.clone(),
                            relation.label.clone(),
                        );
                        if seen_rels.insert(key) {
                            s.total_relations += 1;
                            if let (Some(&src_vid), Some(&tgt_vid)) = (
                                eid_to_vid.get(&relation.source),
                                eid_to_vid.get(&relation.target),
                            ) {
                                if let Ok(mut g) = graph.lock() {
                                    if let Ok(eid) = g.create_edge(relation.label.clone(), src_vid, tgt_vid) {
                                        s.new_edges += 1;

                                        // Create a searchable neuron for this edge
                                        if let Some(ref nn) = neural {
                                            if let Ok(mut nn) = nn.lock() {
                                                let nid = (nn.neuron_count() as u64) + 1;
                                                let mut neuron = crate::neuron::Neuron::for_edge(nid, &relation.label, eid);
                                                // Keywords: relation label + source name + target name
                                                let mut keywords = vec![
                                                    relation.label.clone(),
                                                    relation.source.clone(),
                                                    relation.target.clone(),
                                                ];
                                                neuron.keywords = keywords;
                                                nn.add_neuron(neuron);
                                                nn.auto_synapse(src_vid, tgt_vid);
                                            }
                                        }
                                    }
                                    drop(g);
                                }
                            } else {
                                log::debug!(
                                    "Skipping relation {} --[{}]--> {}: entity not found",
                                    relation.source, relation.label, relation.target
                                );
                            }
                        }
                    }
                }

                // Link extracted entities to their section
                if let Some(&sec_vid) = section_id_to_vid.get(heading) {
                    let eid_to_vid = entity_id_to_vid.lock().unwrap();
                    for entity in &extraction.entities {
                        if let Some(&ent_vid) = eid_to_vid.get(&entity.id) {
                            if let Ok(mut g) = graph.lock() {
                                let _ = g.create_edge("mentioned_in".to_string(), ent_vid, sec_vid);
                            }
                        }
                    }
                }

                // Update processed count and report progress
                let done = processed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                log::info!(
                    "[TIMING] Section '{heading}' done (batch): LLM {llm:.2}s, total so far {total_elapsed:.2}s",
                    heading = heading,
                    llm = llm_elapsed.as_secs_f64(),
                    total_elapsed = batch_start.elapsed().as_secs_f64(),
                );
                if let Some(ref cb) = on_progress {
                    cb(done, *total, heading);
                }
            }

            let graph_elapsed = graph_start.elapsed();
            log::info!(
                "[TIMING] Batch done: LLM {llm:.2}s, graph {graph:.2}s, total {total:.2}s",
                llm = llm_elapsed.as_secs_f64(),
                graph = graph_elapsed.as_secs_f64(),
                total = batch_start.elapsed().as_secs_f64(),
            );
        }));
    }

    // Wait for all sections to complete
    for handle in handles {
        let _ = handle.await;
    }

    // Merge final stats
    let final_s = final_stats.lock().unwrap();
    stats.processed_sections = processed.load(std::sync::atomic::Ordering::SeqCst);
    stats.total_entities = final_s.total_entities;
    stats.total_relations = final_s.total_relations;
    stats.new_vertices = final_s.new_vertices;
    stats.new_edges = final_s.new_edges;
    stats.paragraphs_in_graph = final_s.paragraphs_in_graph;
    stats.total_prompt_tokens = final_s.total_prompt_tokens;
    stats.total_completion_tokens = final_s.total_completion_tokens;

    log::info!(
        "Extraction complete: {} sections, {} entities, {} relations written to graph ({} concurrent)",
        stats.processed_sections,
        stats.new_vertices,
        stats.new_edges,
        config.concurrent_sections,
    );

    Ok(stats)
}

// ─── Graph helpers ───────────────────────────────────────────────

/// Insert an entity as a vertex.
fn insert_entity_to_graph(
    graph: &Arc<Mutex<Graph>>,
    entity: &ExtractedEntity,
) -> Result<VertexId, String> {
    let labels = if entity.labels.is_empty() {
        vec!["entity".to_string()]
    } else {
        entity.labels.clone()
    };
    let mut g = graph.lock().map_err(|e| e.to_string())?;
    let vid = g.create_vertex(labels);
    if let Some(v) = g.get_vertex_mut(vid) {
        v.properties.insert(
            "name".to_string(),
            PropertyValue::String(entity.id.clone()),
        );
        v.properties.insert(
            "extracted_id".to_string(),
            PropertyValue::String(entity.id.clone()),
        );
        for (key, val) in &entity.properties {
            v.properties.insert(
                key.clone(),
                PropertyValue::String(val.clone()),
            );
        }
    }
    Ok(vid)
}

// ─── Section & paragraph helpers ───────────────────────────

/// Insert all sections as vertices with hierarchical edges.
/// Returns map: heading → vertex_id.
fn insert_section_hierarchy(
    graph: &Arc<Mutex<Graph>>,
    sections: &[Section],
) -> Vec<(String, VertexId)> {
    // Stack of (depth, vertex_id) for tracking the current heading hierarchy
    let mut depth_stack: Vec<(usize, VertexId)> = Vec::new();
    let mut result: Vec<(String, VertexId)> = Vec::new();

    let mut g = match graph.lock() {
        Ok(g) => g,
        Err(_) => return result,
    };

    for section in sections {
        let vid = g.create_vertex(vec!["section".to_string()]);
        if let Some(v) = g.get_vertex_mut(vid) {
            v.properties.insert("heading".to_string(), PropertyValue::String(section.heading.clone()));
            v.properties.insert("depth".to_string(), PropertyValue::Integer(section.depth as i64));
            v.properties.insert("heading_chain".to_string(), PropertyValue::String(section.heading_chain.join(" > ")));
            v.properties.insert("index".to_string(), PropertyValue::Integer(section.index as i64));
        }

        // Pop stack until we find the parent (shallower depth)
        while let Some(&(d, _)) = depth_stack.last() {
            if d >= section.depth {
                depth_stack.pop();
            } else {
                break;
            }
        }

        // Link to parent if one exists
        if let Some(&(_, parent_vid)) = depth_stack.last() {
            let _ = g.create_edge("has_subsection".to_string(), parent_vid, vid);
        }

        depth_stack.push((section.depth, vid));
        result.push((section.heading.clone(), vid));
    }

    result
}

/// Split a section's content into paragraphs and insert each as a vertex.
/// Links each paragraph to its parent section via `belongs_to` edge.
/// Returns the list of paragraph vertex IDs.
fn insert_paragraphs_for_section(
    graph: &Arc<Mutex<Graph>>,
    section: &Section,
    section_id_to_vid: &HashMap<String, VertexId>,
) -> Vec<VertexId> {
    let parent_vid = match section_id_to_vid.get(&section.heading) {
        Some(&vid) => vid,
        None => return Vec::new(),
    };

    // Split content by blank lines
    let paragraphs: Vec<&str> = section.content
        .split("\n\n")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    if paragraphs.is_empty() {
        return Vec::new();
    }

    let mut g = match graph.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };

    let mut result = Vec::new();
    for (i, text) in paragraphs.iter().enumerate() {
        let vid = g.create_vertex(vec!["paragraph".to_string()]);
        if let Some(v) = g.get_vertex_mut(vid) {
            v.properties.insert("content".to_string(), PropertyValue::String((*text).to_string()));
            v.properties.insert("index".to_string(), PropertyValue::Integer(i as i64));
        }
        let _ = g.create_edge("belongs_to".to_string(), vid, parent_vid);
        result.push(vid);
    }

    result
}

// ─── Graph helpers (MemorySystem variant) ───────────────────────────

fn insert_entity(memory: &MemorySystem, entity: &ExtractedEntity) -> Result<(), String> {
    let labels = if entity.labels.is_empty() {
        vec!["entity".to_string()]
    } else {
        entity.labels.clone()
    };
    let vid = memory.add_vertex(labels);
    memory.set_vertex_properties(vid, &entity.id, &entity.properties);
    Ok(())
}

fn insert_relation(
    memory: &MemorySystem,
    relation: &ExtractedRelation,
    known_entities: &HashSet<String>,
) -> Result<(), String> {
    if !known_entities.contains(&relation.source)
        || !known_entities.contains(&relation.target)
    {
        return Err(format!(
            "Entities not found: '{}' or '{}'",
            relation.source, relation.target
        ));
    }
    log::debug!(
        "Relation queued: {} --[{}]--> {}",
        relation.source, relation.label, relation.target
    );
    Ok(())
}

// ─── MemorySystem extension ──────────────────────────────────────

impl crate::memory_system::MemorySystem {
    pub fn set_vertex_properties(
        &self,
        vertex_id: crate::graph::VertexId,
        extracted_id: &str,
        props: &HashMap<String, String>,
    ) {
        let mut g = self.graph.lock().unwrap();
        if let Some(v) = g.get_vertex_mut(vertex_id) {
            v.properties.insert(
                "name".to_string(),
                crate::graph::PropertyValue::String(extracted_id.to_string()),
            );
            v.properties.insert(
                "extracted_id".to_string(),
                crate::graph::PropertyValue::String(extracted_id.to_string()),
            );
            for (key, val) in props {
                v.properties.insert(
                    key.clone(),
                    crate::graph::PropertyValue::String(val.clone()),
                );
            }
        }
    }
}
