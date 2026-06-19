use std::collections::{HashMap, HashSet};

use serde::Serialize;

use std::sync::Arc;
use std::sync::Mutex;

use crate::graph::{Graph, PropertyValue, VertexId};
use crate::memory_system::MemorySystem;

use super::config::{ExtractionConfig, ExtractedEntity, ExtractedRelation};
use super::document::{ensure_fits_budget, read_markdown, split_sections, Section};
use super::extraction::{build_user_message, parse_response, SYSTEM_PROMPT};
use super::llm_client::chat_completion_with_retry;

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
    extract_sections_core(config, &sections, graph, None).await
}

/// Same as `extract_content_raw` but with neural network for auto-synapse.
pub async fn extract_content_raw_with_nn(
    config: &ExtractionConfig,
    content: &str,
    source_name: &str,
    graph: &Arc<Mutex<Graph>>,
    neural: &Arc<Mutex<crate::neuron::NeuralNetwork>>,
) -> Result<ExtractionStats, String> {
    log::info!("Extracting knowledge from: {}", source_name);
    let sections = split_sections(content)
        .map_err(|e| format!("Failed to parse '{}': {}", source_name, e))?;
    extract_sections_core(config, &sections, graph, Some(neural)).await
}

// ─── Shared processing core ──────────────────────────────────────

/// Core extraction loop using MemorySystem (for convenience API).
async fn extract_sections_graph(
    config: &ExtractionConfig,
    sections: &[Section],
    graph: &Arc<Mutex<Graph>>,
    neural: &Arc<Mutex<crate::neuron::NeuralNetwork>>,
) -> Result<ExtractionStats, String> {
    extract_sections_core(config, sections, graph, Some(neural)).await
}

/// Core extraction loop — the shared engine.
async fn extract_sections_core(
    config: &ExtractionConfig,
    sections: &[Section],
    graph: &Arc<Mutex<Graph>>,
    neural: Option<&Arc<Mutex<crate::neuron::NeuralNetwork>>>,
) -> Result<ExtractionStats, String> {
    let max_tokens = config.section_token_budget();
    log::info!(
        "Document split into {} sections (max {} tokens/section)",
        sections.len(),
        max_tokens
    );

    // Fit sections to token budget
    let mut fitted = Vec::new();
    for section in sections {
        fitted.extend(ensure_fits_budget(section, max_tokens));
    }
    log::info!("After fitting: {} sections to process", fitted.len());

    let mut seen_entity_ids: HashSet<String> = HashSet::new();
    let mut entity_id_to_vid: HashMap<String, VertexId> = HashMap::new();
    let mut seen_relation_keys: HashSet<(String, String, String)> = HashSet::new();
    let mut section_id_to_vid: HashMap<String, VertexId> = HashMap::new();
    let mut stats = ExtractionStats {
        total_sections: sections.len(),
        ..Default::default()
    };

    // Step 1: Insert all sections as vertices with hierarchical edges
    let section_vids = insert_section_hierarchy(graph, sections);
    for (heading, vid) in &section_vids {
        section_id_to_vid.insert(heading.clone(), *vid);
        stats.sections_in_graph += 1;
    }
    stats.new_vertices += section_vids.len();

    let mut previous_summary: Option<String> = None;

    for section in &fitted {
        log::info!(
            "Processing section [{}/{}]: {}",
            stats.processed_sections + 1,
            fitted.len(),
            section.heading
        );

        let user_msg = build_user_message(section, previous_summary.as_deref());

        let result = chat_completion_with_retry(config, SYSTEM_PROMPT, &user_msg)
            .await
            .map_err(|e| {
                format!("LLM call failed on section '{}': {}", section.heading, e)
            })?;

        stats.total_prompt_tokens += result.prompt_tokens;
        stats.total_completion_tokens += result.completion_tokens;

        let extraction = parse_response(&section.heading, &result.content)
            .map_err(|e| format!("Parse error in section '{}': {}", section.heading, e))?;

        if config.pass_section_context && !extraction.summary.is_empty() {
            previous_summary = Some(extraction.summary.clone());
        }

        // Insert paragraphs for this section
        let para_ids = insert_paragraphs_for_section(graph, section, &section_id_to_vid);
        stats.paragraphs_in_graph += para_ids.len();
        stats.new_vertices += para_ids.len();

        // Deduplicate and insert into graph
        for entity in &extraction.entities {
            if seen_entity_ids.insert(entity.id.clone()) {
                stats.total_entities += 1;
                if let Ok(vid) = insert_entity_to_graph(graph, entity) {
                    entity_id_to_vid.insert(entity.id.clone(), vid);
                    stats.new_vertices += 1;
                }
            }
        }

        // Insert relations as edges (look up source/target vertex IDs)
        for relation in &extraction.relations {
            let key = (
                relation.source.clone(),
                relation.target.clone(),
                relation.label.clone(),
            );
            if seen_relation_keys.insert(key) {
                stats.total_relations += 1;
                match (
                    entity_id_to_vid.get(&relation.source),
                    entity_id_to_vid.get(&relation.target),
                ) {
                    (Some(&src_vid), Some(&tgt_vid)) => {
                        let mut g = graph.lock().map_err(|e| e.to_string())?;
                        if g.create_edge(relation.label.clone(), src_vid, tgt_vid).is_ok() {
                            stats.new_edges += 1;
                        }
                        drop(g);
                        // Auto-create neural synapses between entities
                        if let Some(nn) = neural {
                            nn.lock().unwrap().auto_synapse(src_vid, tgt_vid);
                        }
                    }
                    _ => {
                        log::debug!(
                            "Skipping relation {} --[{}]--> {}: entity not found",
                            relation.source, relation.label, relation.target
                        );
                    }
                }
            }
        }

        // Link extracted entities to their section
        if let Some(&sec_vid) = section_id_to_vid.get(&section.heading) {
            for entity in &extraction.entities {
                if let Some(&ent_vid) = entity_id_to_vid.get(&entity.id) {
                    let mut g = graph.lock().map_err(|e| e.to_string())?;
                    if g.create_edge("mentioned_in".to_string(), ent_vid, sec_vid).is_ok() {
                        stats.new_edges += 1;
                    }
                }
            }
        }

        stats.processed_sections += 1;
    }

    log::info!(
        "Extraction complete: {} sections, {} entities, {} relations written to graph",
        stats.processed_sections,
        stats.new_vertices,
        stats.new_edges
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
