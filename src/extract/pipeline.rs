use std::collections::{HashMap, HashSet};

use serde::Serialize;

use std::sync::Arc;
use std::sync::Mutex;

use crate::graph::{Graph, PropertyValue, VertexId};
use crate::memory_system::MemorySystem;

use super::config::{ExtractionConfig, ExtractedEntity, ExtractedRelation, SectionExtraction};
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
    extract_sections_core(config, &sections, graph).await
}

// ─── Shared processing core ──────────────────────────────────────

/// Core extraction loop using MemorySystem (for convenience API).
async fn extract_sections_graph(
    config: &ExtractionConfig,
    sections: &[Section],
    graph: &Arc<Mutex<Graph>>,
    _neural: &Arc<Mutex<crate::neuron::NeuralNetwork>>,
) -> Result<ExtractionStats, String> {
    extract_sections_core(config, sections, graph).await
}

/// Core extraction loop — the shared engine.
async fn extract_sections_core(
    config: &ExtractionConfig,
    sections: &[Section],
    graph: &Arc<Mutex<Graph>>,
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
    let mut seen_relation_keys: HashSet<(String, String, String)> = HashSet::new();
    let mut stats = ExtractionStats {
        total_sections: sections.len(),
        ..Default::default()
    };

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

        // Deduplicate and insert into graph
        for entity in &extraction.entities {
            if seen_entity_ids.insert(entity.id.clone()) {
                stats.total_entities += 1;
                if insert_entity_to_graph(graph, entity).is_ok() {
                    stats.new_vertices += 1;
                }
            }
        }

        for relation in &extraction.relations {
            let key = (
                relation.source.clone(),
                relation.target.clone(),
                relation.label.clone(),
            );
            if seen_relation_keys.insert(key) {
                stats.total_relations += 1;
                log::debug!(
                    "Relation: {} --[{}]--> {}",
                    relation.source, relation.label, relation.target
                );
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
) -> Result<(), String> {
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
    Ok(())
}

// Keep old MemorySystem-based helpers for backward compat
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
    _memory: &MemorySystem,
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
        "Relation: {} --[{}]--> {}",
        relation.source, relation.label, relation.target
    );
    Ok(())
}

// ─── Graph helpers ───────────────────────────────────────────────

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
