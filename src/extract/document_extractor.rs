use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;

use crate::graph::{Graph, PropertyValue, VertexId};
use crate::neuron::NeuralNetwork;

use super::config::ExtractionConfig;
use super::llm_client::chat_completion_with_retry;

/// Simple extraction statistics.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ExtractionResult {
    pub total_entities: usize,
    pub total_relations: usize,
    pub new_vertices: usize,
    pub new_edges: usize,
}

/// Progress callback: (step_label, progress_pct_0_to_100, optional_detail)
pub type StepCallback = Box<dyn Fn(&str, f64, Option<&str>) + Send + Sync>;

/// Token overhead for system prompt + user instructions.
const PROMPT_OVERHEAD_TOKENS: usize = 4096;

/// System prompt for full-document extraction.
const FULL_DOC_PROMPT: &str = r#"You are a precise knowledge graph extractor. 
Extract named entities and their relationships from the given document.

## Rules
1. **Entities**: Extract the most important named entities (5-25). Include: people, places, organizations, concepts, events, objects.
2. **Relationships**: Extract meaningful connections between entities. Use clear, concise predicate labels.
3. **Names**: Keep names in their original language (Chinese stays Chinese, English stays English).
4. **Types**: Each entity needs a type field. Pick from: person, place, organization, concept, event, object.
5. **Descriptions**: Each entity needs a brief description (1 sentence).

## Output Format — Return ONLY valid JSON, no markdown fences, no extra text:

{
  "entities": [
    {
      "name": "EntityName",
      "type": "person|place|organization|concept|event|object",
      "description": "Brief description"
    }
  ],
  "relations": [
    {
      "source": "EntityName",
      "target": "EntityName",
      "relation": "relationship description"
    }
  ]
}

If the document has no extractable entities, return {"entities": [], "relations": []}.
"#;

/// Parsed LLM response for full-document extraction.
#[derive(Debug, Clone, Deserialize)]
struct FullExtractionOutput {
    #[serde(default)]
    entities: Vec<EntityOutput>,
    #[serde(default)]
    relations: Vec<RelationOutput>,
}

#[derive(Debug, Clone, Deserialize)]
struct EntityOutput {
    name: Option<String>,
    #[serde(rename = "type")]
    type_: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RelationOutput {
    source: Option<String>,
    target: Option<String>,
    relation: Option<String>,
}

/// Extract entities and relations from a full document in ONE LLM call.
///
/// Steps reported via `on_step`:
///   1. "Analyzing document content" — token estimation & context check
///   2. "Calling LLM to extract knowledge" — the LLM call
///   3. "Creating graph vertices" — creating vertex for each entity
///   4. "Creating graph edges" — creating edge for each relation
///
/// Returns `ExtractionResult` with counts.
pub async fn extract_document_full(
    config: &ExtractionConfig,
    content: &str,
    doc_title: &str,
    graph: &Arc<Mutex<Graph>>,
    neural: &Arc<Mutex<NeuralNetwork>>,
    on_step: StepCallback,
) -> Result<ExtractionResult, String> {
    // ── Step 1: Analyze document ──────────────────────────────
    on_step("Analyzing document content", 10.0, None);

    // Estimate token count (rough: ~4 chars per token for CJK)
    let estimated_tokens = content.len() / 2 + content.split_whitespace().count();
    let available = config
        .context_window
        .saturating_sub(PROMPT_OVERHEAD_TOKENS)
        .saturating_sub(config.max_output_tokens);

    if estimated_tokens > available {
        return Err(format!(
            "DOCUMENT_TOO_LARGE: Document is too large ({} chars, ~{} estimated tokens, limit is {} tokens). \
             Please split it into smaller sections and import separately.",
            content.len(),
            estimated_tokens,
            available,
        ));
    }

    // ── Step 2: Call LLM ─────────────────────────────────────
    on_step("Calling LLM to extract knowledge", 0.0, None);

    let user_message = format!(
        "Document: {}\n\n---\n\n{}",
        doc_title,
        content
    );

    let llm_result = chat_completion_with_retry(config, FULL_DOC_PROMPT, &user_message).await
        .map_err(|e| format!("LLM call failed: {}", e))?;

    on_step("Calling LLM to extract knowledge", 100.0,
        Some(&format!("Used {} prompt + {} completion tokens",
            llm_result.prompt_tokens, llm_result.completion_tokens)));

    // Check for truncated output
    if let Some(ref finish_reason) = llm_result.finish_reason {
        if finish_reason == "length" {
            return Err(format!(
                "DOCUMENT_TOO_LARGE: LLM output was truncated (reached max output tokens). \
                 The document content is too large. Please split it into smaller sections and import separately.",
            ));
        }
    }

    // ── Parse LLM response ───────────────────────────────────
    let cleaned = clean_json(&llm_result.content);
    let parsed: FullExtractionOutput = serde_json::from_str(&cleaned)
        .map_err(|e| {
            format!(
                "Failed to parse LLM response: {}. Raw (first 500): {}",
                e,
                &llm_result.content[..llm_result.content.len().min(500)]
            )
        })?;

    let entities = parsed.entities;
    let relations = parsed.relations;

    if entities.is_empty() && relations.is_empty() {
        return Ok(ExtractionResult {
            total_entities: 0,
            total_relations: 0,
            new_vertices: 0,
            new_edges: 0,
        });
    }

    // ── Step 3: Create vertices for each entity ──────────────
    on_step("Creating graph vertices", 0.0, None);

    let total_entities = entities.len();
    let mut name_to_vid: HashMap<String, VertexId> = HashMap::new();
    let mut vertex_count = 0usize;

    for (i, entity) in entities.iter().enumerate() {
        let name = entity.name.as_deref().unwrap_or("unknown");
        let type_label = entity.type_.as_deref().unwrap_or("entity");
        let description = entity.description.as_deref().unwrap_or("");

        let labels = vec![type_label.to_string(), "entity".to_string()];
        let mut g = graph.lock().map_err(|e| e.to_string())?;
        let vid = g.create_vertex(labels.clone());
        if let Some(v) = g.get_vertex_mut(vid) {
            v.properties.insert("name".to_string(), PropertyValue::String(name.to_string()));
            v.properties.insert("description".to_string(), PropertyValue::String(description.to_string()));
            v.properties.insert("source_file".to_string(), PropertyValue::String(doc_title.to_string()));
        }
        drop(g);

        // Create a searchable neuron
        {
            let mut nn = neural.lock().map_err(|e| e.to_string())?;
            let nid = (nn.neuron_count() as u64) + 1;
            let mut neuron = crate::neuron::Neuron::for_vertex(nid, type_label, vid);
            neuron.keywords = vec![name.to_string(), type_label.to_string()];
            nn.add_neuron(neuron);
        }

        name_to_vid.insert(name.to_string(), vid);
        vertex_count += 1;

        let pct = ((i + 1) as f64 / total_entities as f64) * 100.0;
        on_step("Creating graph vertices", pct,
            Some(&format!("{}/{} vertices created", vertex_count, total_entities)));
    }

    // ── Step 4: Create edges for each relation ───────────────
    on_step("Creating graph edges", 0.0, None);

    let total_relations = relations.len();
    let mut edge_count = 0usize;

    for (i, relation) in relations.iter().enumerate() {
        let src_name = relation.source.as_deref().unwrap_or("");
        let tgt_name = relation.target.as_deref().unwrap_or("");
        let rel_label = relation.relation.as_deref().unwrap_or("related_to");

        if let (Some(&src_vid), Some(&tgt_vid)) = (name_to_vid.get(src_name), name_to_vid.get(tgt_name)) {
            let mut g = graph.lock().map_err(|e| e.to_string())?;
            if let Ok(eid) = g.create_edge(rel_label.to_string(), src_vid, tgt_vid) {
                drop(g);
                // Create edge neuron + auto-synapse
                let mut nn = neural.lock().map_err(|e| e.to_string())?;
                let nid = (nn.neuron_count() as u64) + 1;
                let mut neuron = crate::neuron::Neuron::for_edge(nid, rel_label, eid);
                neuron.keywords = vec![rel_label.to_string(), src_name.to_string(), tgt_name.to_string()];
                nn.add_neuron(neuron);
                nn.auto_synapse(src_vid, tgt_vid);
            }
            edge_count += 1;
        }

        let pct = ((i + 1) as f64 / total_relations.max(1) as f64) * 100.0;
        on_step("Creating graph edges", pct,
            Some(&format!("{}/{} edges created", edge_count, total_relations)));
    }

    let result = ExtractionResult {
        total_entities,
        total_relations,
        new_vertices: vertex_count,
        new_edges: edge_count,
    };

    on_step("Complete", 100.0,
        Some(&format!("{} entities, {} relations, {} vertices, {} edges",
            result.total_entities, result.total_relations,
            result.new_vertices, result.new_edges)));

    Ok(result)
}

/// Strip markdown code fences from LLM output.
fn clean_json(text: &str) -> String {
    let text = text.trim();
    if let Some(inner) = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
    {
        if let Some(end) = inner.rfind("```") {
            return inner[..end].trim().to_string();
        }
        return inner.trim().to_string();
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_json_fences() {
        assert_eq!(clean_json("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(clean_json("{\"a\":1}"), "{\"a\":1}");
        assert_eq!(clean_json("```\nhello\n```"), "hello");
    }

    #[test]
    fn test_entity_output_deserialize() {
        let json = r#"{"name": "韩立", "type": "person", "description": "主角"}"#;
        let e: EntityOutput = serde_json::from_str(json).unwrap();
        assert_eq!(e.name.unwrap(), "韩立");
        assert_eq!(e.type_.unwrap(), "person");
    }

    #[test]
    fn test_full_extraction_deserialize() {
        let json = r#"{
            "entities": [
                {"name": "韩立", "type": "person", "description": "主角"}
            ],
            "relations": [
                {"source": "韩立", "target": "南宫碗", "relation": "道侣"}
            ]
        }"#;
        let parsed: FullExtractionOutput = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.entities.len(), 1);
        assert_eq!(parsed.relations.len(), 1);
        assert_eq!(parsed.entities[0].name.as_deref(), Some("韩立"));
        assert_eq!(parsed.relations[0].relation.as_deref(), Some("道侣"));
    }
}
