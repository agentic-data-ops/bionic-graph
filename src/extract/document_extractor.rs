use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;

use crate::graph::PropertyValue;
use crate::graph_manager::GraphManager;

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

/// System prompt for tag extraction.
const TAG_EXTRACT_PROMPT: &str = r#"You are a knowledge graph assistant. Generate 1~5 most important tags from the markdown document below, to describe the content topic.

Return ONLY valid JSON: {"tags": ["tag1", "tag2", ...]}
"#;

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
  ]
}

- Extract entities and edges as many as possible.
- For each entity, provide as many as possible search keywords that help find this entity. Do NOT include the entity name or type in keywords — they are already used as search terms automatically. Only provide ADDITIONAL keywords.
- For each entity, extract as many as possible properties of this entity. Property key should be in english, property value should be in the original language.
- Relations should use clear, concise descriptions in the original language.
- Entity type could be person, place, organization, concept, event, object or any other types identify what type of thing it is.
- Entity name, type, keywords should be in the original language.
"#;

/// Parsed LLM response for tags.
#[derive(Debug, Clone, Deserialize)]
struct TagOutput {
    #[serde(default)]
    tags: Vec<String>,
}

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

/// Estimate whether a content will fit within the LLM context + output limits.
fn estimate_tokens(content: &str) -> usize {
    // Rough: ~2 chars per token for CJK text
    content.len() / 2 + content.split_whitespace().count()
}

fn is_over_limit(config: &ExtractionConfig, content: &str) -> bool {
    let estimated = estimate_tokens(content);
    let available = config
        .context_window
        .saturating_sub(PROMPT_OVERHEAD_TOKENS)
        .saturating_sub(config.max_output_tokens);
    estimated > available
}

/// Split content by chapter headings (## or ###).
fn split_by_chapters(content: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for line in content.lines() {
        if line.starts_with("## ") || line.starts_with("### ") {
            if !current.is_empty() {
                parts.push(current.trim().to_string());
            }
            current = line.to_string();
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

/// Call LLM for tags, with splitting if content is too large.
async fn extract_tags(
    config: &ExtractionConfig,
    content: &str,
    doc_title: &str,
    on_step: &StepCallback,
) -> Result<Vec<String>, String> {
    if is_over_limit(config, content) {
        let chapters = split_by_chapters(content);
        let mut all_tags = Vec::new();
        for (i, chapter) in chapters.iter().enumerate() {
            on_step("Extracting tags", (i as f64 / chapters.len() as f64) * 50.0,
                Some(&format!("Chapter {}/{}", i + 1, chapters.len())));
            let user_msg = format!("Document section: {}\n\n---\n\n{}", doc_title, chapter);
            let resp = chat_completion_with_retry(config, TAG_EXTRACT_PROMPT, &user_msg).await
                .map_err(|e| format!("Tag LLM call failed: {}", e))?;
            let cleaned = clean_json(&resp.content);
            if let Ok(parsed) = serde_json::from_str::<TagOutput>(&cleaned) {
                for tag in parsed.tags {
                    if !all_tags.contains(&tag) {
                        all_tags.push(tag);
                    }
                }
            }
        }
        on_step("Extracting tags", 50.0, Some(&format!("{} tags merged", all_tags.len())));
        Ok(all_tags)
    } else {
        let user_msg = format!("Document: {}\n\n---\n\n{}", doc_title, content);
        let resp = chat_completion_with_retry(config, TAG_EXTRACT_PROMPT, &user_msg).await
            .map_err(|e| format!("Tag LLM call failed: {}", e))?;
        let cleaned = clean_json(&resp.content);
        let parsed: TagOutput = serde_json::from_str(&cleaned)
            .map_err(|e| format!("Failed to parse tag response: {}", e))?;
        Ok(parsed.tags)
    }
}

/// Call LLM for entities/relations, with splitting + merging.
async fn extract_entities_and_relations(
    config: &ExtractionConfig,
    content: &str,
    doc_title: &str,
    on_step: &StepCallback,
) -> Result<(Vec<EntityOutput>, Vec<RelationOutput>), String> {
    if is_over_limit(config, content) {
        let chapters = split_by_chapters(content);
        let mut all_entities: Vec<EntityOutput> = Vec::new();
        let mut all_relations: Vec<RelationOutput> = Vec::new();
        for (i, chapter) in chapters.iter().enumerate() {
            on_step("Extracting knowledge", (i as f64 / chapters.len() as f64) * 80.0,
                Some(&format!("Chapter {}/{}", i + 1, chapters.len())));
            let user_msg = format!("Document section: {}\n\n---\n\n{}", doc_title, chapter);
            let resp = chat_completion_with_retry(config, FULL_DOC_PROMPT, &user_msg).await
                .map_err(|e| format!("LLM call failed: {}", e))?;
            let cleaned = clean_json(&resp.content);
            if let Ok(parsed) = serde_json::from_str::<FullExtractionOutput>(&cleaned) {
                all_entities.extend(parsed.entities);
                all_relations.extend(parsed.relations);
            }
        }
        on_step("Extracting knowledge", 80.0,
            Some(&format!("{} entities, {} relations from {} chapters",
                all_entities.len(), all_relations.len(), chapters.len())));
        Ok((all_entities, all_relations))
    } else {
        let user_msg = format!("Document: {}\n\n---\n\n{}", doc_title, content);
        let resp = chat_completion_with_retry(config, FULL_DOC_PROMPT, &user_msg).await
            .map_err(|e| format!("LLM call failed: {}", e))?;
        on_step("Extracting knowledge", 80.0,
            Some(&format!("Used {} prompt + {} completion tokens",
                resp.prompt_tokens, resp.completion_tokens)));

        // Check for truncated output
        if let Some(ref finish_reason) = resp.finish_reason {
            if finish_reason == "length" {
                // Split and retry
                let chapters = split_by_chapters(content);
                let mut all_entities = Vec::new();
                let mut all_relations = Vec::new();
                for (i, chapter) in chapters.iter().enumerate() {
                    on_step("Extracting knowledge (retry)", (i as f64 / chapters.len() as f64) * 80.0,
                        Some(&format!("Chapter {}/{}", i + 1, chapters.len())));
                    let user_msg = format!("Document section: {}\n\n---\n\n{}", doc_title, chapter);
                    if let Ok(retry_resp) = chat_completion_with_retry(config, FULL_DOC_PROMPT, &user_msg).await {
                        let cleaned = clean_json(&retry_resp.content);
                        if let Ok(parsed) = serde_json::from_str::<FullExtractionOutput>(&cleaned) {
                            all_entities.extend(parsed.entities);
                            all_relations.extend(parsed.relations);
                        }
                    }
                }
                return Ok((all_entities, all_relations));
            }
        }

        let cleaned = clean_json(&resp.content);
        let parsed: FullExtractionOutput = serde_json::from_str(&cleaned)
            .map_err(|e| {
                format!(
                    "Failed to parse LLM response: {}. Raw (first 500): {}",
                    e,
                    &resp.content[..resp.content.len().min(500)]
                )
            })?;
        Ok((parsed.entities, parsed.relations))
    }
}

/// Dedup entities by name: merge keywords, merge property keys, ignore value differences.
fn dedup_entities(entities: &[EntityOutput]) -> Vec<EntityOutput> {
    let mut merged: HashMap<String, EntityOutput> = HashMap::new();
    for entity in entities {
        let name = entity.name.as_deref().unwrap_or("unknown").to_string();
        let entry = merged.entry(name.clone()).or_insert_with(|| EntityOutput {
            name: Some(name.clone()),
            type_: entity.type_.clone(),
            properties: Some(HashMap::new()),
            keywords: Some(Vec::new()),
        });
        // Merge types
        if let Some(ref types) = entity.type_ {
            let entry_types = entry.type_.get_or_insert_with(Vec::new);
            for t in types {
                if !entry_types.contains(t) {
                    entry_types.push(t.clone());
                }
            }
        }
        // Merge keywords
        if let Some(ref kws) = entity.keywords {
            let entry_kws = entry.keywords.get_or_insert_with(Vec::new);
            for kw in kws {
                if !entry_kws.contains(kw) {
                    entry_kws.push(kw.clone());
                }
            }
        }
        // Merge property keys (ignore values)
        if let Some(ref props) = entity.properties {
            let entry_props = entry.properties.get_or_insert_with(HashMap::new);
            for (k, _v) in props {
                entry_props.entry(k.clone()).or_insert_with(|| serde_json::Value::String(String::new()));
            }
        }
    }
    merged.into_values().collect()
}

/// Convert serde_json::Value to PropertyValue for graph storage.
fn json_to_property_value(val: &serde_json::Value) -> PropertyValue {
    match val {
        serde_json::Value::String(s) => PropertyValue::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { PropertyValue::Integer(i) }
            else if let Some(f) = n.as_f64() { PropertyValue::Float(f) }
            else { PropertyValue::Null }
        }
        serde_json::Value::Bool(b) => PropertyValue::Boolean(*b),
        serde_json::Value::Array(arr) => PropertyValue::List(arr.iter().map(json_to_property_value).collect()),
        _ => PropertyValue::String(val.to_string()),
    }
}

/// Extract entities and relations from document content.
///
/// Steps reported via `on_step`:
///   1. "Extracting tags" — call LLM for tags, split if needed
///   2. "Extracting knowledge" — call LLM for entities+relations, split+merge if needed
///   3. "Creating graph vertices" — dedup + save via GraphManager
///   4. "Creating graph edges" — save via GraphManager
///
/// Returns `ExtractionResult` with counts.
pub async fn extract_document_full(
    config: &ExtractionConfig,
    content: &str,
    doc_title: &str,
    doc_id: &str,
    graph_manager: &Arc<Mutex<GraphManager>>,
    graph_name: &str,
    on_step: StepCallback,
) -> Result<ExtractionResult, String> {
    // ── Step 1: Extract tags (with splitting if needed) ──────
    on_step("Extracting tags", 0.0, None);
    let tags = extract_tags(config, content, doc_title, &on_step).await?;

    // ── Step 2: Extract entities + relations (with splitting if needed) ──
    on_step("Extracting knowledge", 20.0, None);
    let (entities, relations) = extract_entities_and_relations(
        config, content, doc_title, &on_step,
    ).await?;

    let total_entities = entities.len();
    let total_relations = relations.len();

    if entities.is_empty() && relations.is_empty() {
        return Ok(ExtractionResult {
            total_entities: 0,
            total_relations: 0,
            new_vertices: 0,
            new_edges: 0,
            tags,
        });
    }

    // ── Step 3: Dedup + create vertices ──────────────────────
    on_step("Creating graph vertices", 80.0, None);
    let deduped = dedup_entities(&entities);
    let total_deduped = deduped.len();
    let mut name_to_vid: HashMap<String, u64> = HashMap::new();
    let mut vertex_count = 0usize;

    for (i, entity) in deduped.iter().enumerate() {
        let name = entity.name.as_deref().unwrap_or("unknown");
        let type_labels = entity.type_.as_ref()
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| vec!["entity".to_string()]);
        let entity_kw = entity.keywords.clone().unwrap_or_default();
        let entity_props = entity.properties.clone().unwrap_or_default();

        let props: HashMap<String, PropertyValue> = entity_props.iter()
            .map(|(k, v)| (k.clone(), json_to_property_value(v)))
            .collect();

        let vid = {
            let gm = graph_manager.lock().map_err(|e| e.to_string())?;
            gm.add_vertex_to_graph(graph_name, name, &entity_kw, &type_labels, &props)
        }.map_err(|e| format!("Failed to create vertex '{}': {}", name, e))?;

        name_to_vid.insert(name.to_string(), vid);
        vertex_count += 1;

        let pct = 80.0 + ((i + 1) as f64 / total_deduped.max(1) as f64) * 10.0;
        on_step("Creating graph vertices", pct,
            Some(&format!("{}/{} vertices created", vertex_count, total_deduped)));
    }

    // ── Step 4: Create edges ──────────────────────────────────
    on_step("Creating graph edges", 90.0, None);

    let total_relations = relations.len();
    let mut edge_count = 0usize;

    for (i, relation) in relations.iter().enumerate() {
        let src_name = relation.source.as_deref().unwrap_or("");
        let tgt_name = relation.target.as_deref().unwrap_or("");
        let rel_label = relation.relation.as_deref().unwrap_or("related_to");

        if let (Some(&src_vid), Some(&tgt_vid)) = (name_to_vid.get(src_name), name_to_vid.get(tgt_name)) {
            let props = HashMap::new();
            let edge_result = {
                let gm = graph_manager.lock().map_err(|e| e.to_string())?;
                gm.add_edge_to_graph(graph_name, rel_label, src_vid, tgt_vid, &props)
            };
            if edge_result.is_ok() {
                edge_count += 1;
            }
        }

        let pct = 90.0 + ((i + 1) as f64 / total_relations.max(1) as f64) * 10.0;
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
        Some(&format!("{} entities, {} relations, {} vertices, {} edges ({} after dedup)",
            result.total_entities, result.total_relations,
            result.new_vertices, result.new_edges, total_deduped)));

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
    fn test_dedup_entities() {
        let entities = vec![
            EntityOutput {
                name: Some("Alice".into()),
                type_: Some(vec!["person".into()]),
                properties: Some([("age".into(), serde_json::json!("30"))].into()),
                keywords: Some(vec!["engineer".into()]),
            },
            EntityOutput {
                name: Some("Alice".into()),
                type_: Some(vec!["employee".into()]),
                properties: Some([("title".into(), serde_json::json!("SE"))].into()),
                keywords: Some(vec!["developer".into()]),
            },
        ];
        let deduped = dedup_entities(&entities);
        assert_eq!(deduped.len(), 1);
        let a = &deduped[0];
        assert_eq!(a.name.as_deref(), Some("Alice"));
        assert!(a.keywords.as_ref().unwrap().contains(&"engineer".to_string()));
        assert!(a.keywords.as_ref().unwrap().contains(&"developer".to_string()));
    }
}
