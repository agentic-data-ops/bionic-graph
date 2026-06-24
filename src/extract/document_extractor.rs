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
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Progress callback: (step_label, progress_pct_0_to_100, optional_detail)
pub type StepCallback = Box<dyn Fn(&str, f64, Option<&str>) + Send + Sync>;

/// Token overhead for system prompt + user instructions.
const PROMPT_OVERHEAD_TOKENS: usize = 4096;

/// System prompt for full-document extraction.
const FULL_DOC_PROMPT: &str = r#"You are a knowledge graph extractor. Extract entities and their relationships from the given markdown document.

Return ONLY valid JSON with this structure:

{
  "entities": [
    {
      "name": "EntityName",
      "type": ["entity type1", "entity type2", ...],
      "keywords": ["search keyword1", "search keyword2", ...],
      "properties": {
        "property key1": "property value1",
        "property key2": "property value2",
        ...
      }
    }
  ],
  "relations": [
    {
      "source": "EntityName",
      "target": "EntityName",
      "relation": "relationship description"
    }
  ],
  "tags": ["tag1", "tag2", ...]
}

- Extract entities and edges as many as possible.
- For each entity, provide as many as possible search keywords that help find this entity. Do NOT include the entity name or type in keywords — they are already used as search terms automatically. Only provide ADDITIONAL keywords.
- For each entity, extract as many as possible properties of this entity. Property key should be in english, property value should be in the original language.
- Relations should use clear, concise descriptions in the original language.
- Entity type could be person, place, organization, concept, event, object or any other types identify what type of thing it is.
- Entity name, type, keywords should be in the original language.
- Generate 1~5 most important tags from the markdown document, to describe the content topic.
"#;

/// Parsed LLM response for full-document extraction.
#[derive(Debug, Clone, Deserialize)]
struct FullExtractionOutput {
    #[serde(default)]
    entities: Vec<EntityOutput>,
    #[serde(default)]
    relations: Vec<RelationOutput>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct EntityOutput {
    name: Option<String>,
    #[serde(rename = "type", default)]
    type_: Option<Vec<String>>,
    #[serde(default)]
    properties: Option<std::collections::HashMap<String, serde_json::Value>>,
    #[serde(default)]
    keywords: Option<Vec<String>>,
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
    doc_id: &str,
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
    let tags = parsed.tags;

    if entities.is_empty() && relations.is_empty() {
        return Ok(ExtractionResult {
            total_entities: 0,
            total_relations: 0,
            new_vertices: 0,
            new_edges: 0,
            tags: Vec::new(),
        });
    }

    // ── Step 3: Create vertices for each entity ──────────────
    on_step("Creating graph vertices", 0.0, None);

    let total_entities = entities.len();
    let mut name_to_vid: HashMap<String, VertexId> = HashMap::new();
    let mut vertex_count = 0usize;

    // Pre-compute starting nid to avoid lock-acquire race
    let start_nid = {
        let nn = neural.lock().map_err(|e| e.to_string())?;
        (nn.neuron_count() as u64) + 1
    };

    for (i, entity) in entities.iter().enumerate() {
        let name = entity.name.as_deref().unwrap_or("unknown");
        let type_labels = entity.type_.as_ref()
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| vec!["entity".to_string()]);
        let entity_props = entity.properties.clone().unwrap_or_default();
        let entity_kw = entity.keywords.clone().unwrap_or_default();

        let labels = type_labels.clone();
        let mut g = graph.lock().map_err(|e| e.to_string())?;
        let vid = g.create_vertex(labels.clone());
        if let Some(v) = g.get_vertex_mut(vid) {
            v.name = name.to_string();
            v.document = doc_id.to_string();
            v.keywords = entity_kw.clone();
            for (k, val) in entity_props {
                let str_val = match &val {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                v.properties.insert(k, PropertyValue::String(str_val));
            }
        }
        drop(g);

        let nid = start_nid + i as u64;

        // Build neuron keywords from entity keywords, name, and type labels
        let mut neuron_kw = entity_kw.clone();
        if !neuron_kw.contains(&name.to_string()) {
            neuron_kw.push(name.to_string());
        }
        for t in &type_labels {
            if !neuron_kw.contains(t) {
                neuron_kw.push(t.clone());
            }
        }

        // Create a searchable neuron
        {
            let mut nn = neural.lock().map_err(|e| e.to_string())?;
            let first_type = type_labels.first().map(|s| s.as_str()).unwrap_or("entity");
            let mut neuron = crate::neuron::Neuron::for_vertex(nid, first_type, vid);
            neuron.keywords = neuron_kw;
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

    let edge_start_nid = {
        let nn = neural.lock().map_err(|e| e.to_string())?;
        (nn.neuron_count() as u64) + 1
    };

    for (i, relation) in relations.iter().enumerate() {
        let src_name = relation.source.as_deref().unwrap_or("");
        let tgt_name = relation.target.as_deref().unwrap_or("");
        let rel_label = relation.relation.as_deref().unwrap_or("related_to");

        if let (Some(&src_vid), Some(&tgt_vid)) = (name_to_vid.get(src_name), name_to_vid.get(tgt_name)) {
            let mut g = graph.lock().map_err(|e| e.to_string())?;
            if let Ok(eid) = g.create_edge(rel_label.to_string(), src_vid, tgt_vid) {
                if let Some(e) = g.get_edge_mut(eid) {
                    e.document = doc_id.to_string();
                }
                drop(g);
                // Create edge neuron + auto-synapse
                let nid = edge_start_nid + edge_count as u64;
                let mut nn = neural.lock().map_err(|e| e.to_string())?;
                let mut neuron = crate::neuron::Neuron::for_edge(nid, rel_label, eid);
                neuron.vertex_refs = vec![src_vid, tgt_vid];
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
        tags,
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
        let json = r#"{"name": "韩立", "type": ["person", "protagonist"], "properties": {"cultivation": "金丹期"}, "keywords": ["韩跑跑", "厉飞雨"]}"#;
        let e: EntityOutput = serde_json::from_str(json).unwrap();
        assert_eq!(e.name.unwrap(), "韩立");
        let types = e.type_.unwrap();
        assert_eq!(types.len(), 2);
        assert_eq!(types[0], "person");
        let props = e.properties.unwrap();
        assert_eq!(props.get("cultivation").unwrap(), "金丹期");
        let kws = e.keywords.unwrap();
        assert!(kws.contains(&"韩跑跑".to_string()));
    }

    #[test]
    fn test_full_extraction_deserialize() {
        let json = r#"{
            "entities": [
                {"name": "韩立", "type": ["person", "protagonist"], "properties": {"cultivation": "金丹期"}, "keywords": ["韩跑跑"]}
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
