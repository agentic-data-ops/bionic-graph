use std::collections::HashMap;

use serde::Deserialize;

use super::config::{ExtractedEntity, ExtractedRelation, SectionExtraction};
use super::document::Section;

// ─── Prompt Templates ────────────────────────────────────────────

/// System prompt instructing the LLM how to extract knowledge.
pub const SYSTEM_PROMPT: &str = r#"You are a precise knowledge extraction engine. Your task is to analyze a document section and extract named entities and their relationships.

## Rules

1. **Entities**: Extract named concepts, technologies, people, organizations, projects, APIs, protocols, data formats — anything with a distinct identity mentioned in the text.
2. **Relationships**: Extract meaningful connections between entities. Use clear, concise predicate labels (e.g. "depends_on", "implements", "extends", "uses", "part_of", "developed_by").
3. **IDs**: Use the entity's original name directly as the ID — Chinese names stay in Chinese (e.g. "乔峰", "段誉", not "QiaoFeng", "DuanYu"). For English names, keep the original spelling. Be consistent: the same entity always uses the exact same ID across all sections.
4. **Labels**: Each entity needs at least one label (type). Examples: ["technology"], ["concept"], ["protocol"], ["organization"], ["person"].
5. **Properties**: Include relevant details as key-value pairs. Standard keys: "description", "mentioned_in", "version", "url".
6. **No duplicates**: If a named entity was already extracted in a previous section, reuse its exact ID. Do NOT create a new entity with a different ID for the same thing.

## Output Format

Respond with ONLY valid JSON. No markdown fences, no extra text.

{
  "section_summary": "one-sentence summary of this section's topic",
  "entities": [
    {
      "id": "EntityName",
      "labels": ["Type1", "Type2"],
      "properties": {
        "description": "Brief description",
        "mentioned_in": "section heading"
      }
    }
  ],
  "relations": [
    {
      "source": "EntityName1",
      "target": "EntityName2",
      "label": "relationship_type",
      "properties": {
        "description": "Context of the relationship"
      }
    }
  ]
}

If the section contains no extractable entities, return {"section_summary": "...", "entities": [], "relations": []}.
"#;

/// Build the user message for a given section, optionally including context.
pub fn build_user_message(section: &Section, previous_summary: Option<&str>) -> String {
    let heading_chain = section.heading_chain.join(" > ");

    let context_note = match previous_summary {
        Some(summary) => format!(
            "[Previous section context: {}]\n\n",
            summary
        ),
        None => String::new(),
    };

    format!(
        "{}## Document Section\n\n**Heading chain**: {}\n**Heading**: {}\n\n**Content**:\n{}",
        context_note, heading_chain, section.heading, section.content
    )
}

// ─── Response Parser ─────────────────────────────────────────────

/// Parsed JSON structure from the LLM response.
#[derive(Debug, Clone, Deserialize)]
struct LlmExtraction {
    section_summary: Option<String>,
    entities: Option<Vec<LlmEntity>>,
    relations: Option<Vec<LlmRelation>>,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmEntity {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    labels: Option<Vec<String>>,
    #[serde(default)]
    properties: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmRelation {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    properties: Option<HashMap<String, serde_json::Value>>,
}

/// Parse the LLM response text into a SectionExtraction.
pub fn parse_response(heading: &str, response_text: &str) -> Result<SectionExtraction, String> {
    let cleaned = clean_json(response_text);
    let parsed: LlmExtraction =
        serde_json::from_str(&cleaned).map_err(|e| {
            format!(
                "Failed to parse LLM response as JSON: {}\nRaw (cleaned): {}",
                e,
                &cleaned[..cleaned.len().min(500)]
            )
        })?;

    let entities = parsed
        .entities
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| {
            let id = e.id.filter(|s| !s.is_empty())?;
            let labels = e.labels.unwrap_or_default();
            if labels.is_empty() {
                return None;
            }
            Some(ExtractedEntity {
                id,
                labels,
                properties: e.properties.unwrap_or_default()
                    .into_iter().map(|(k, v)| (k, json_val_to_string(v))).collect(),
            })
        })
        .collect();

    let relations = parsed
        .relations
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| {
            let source = r.source.filter(|s| !s.is_empty())?;
            let target = r.target.filter(|s| !s.is_empty())?;
            let label = r.label.filter(|s| !s.is_empty())?;
            Some(ExtractedRelation {
                source,
                target,
                label,
                properties: r.properties.unwrap_or_default()
                    .into_iter().map(|(k, v)| (k, json_val_to_string(v))).collect(),
            })
        })
        .collect();

    Ok(SectionExtraction {
        heading: heading.to_string(),
        summary: parsed.section_summary.unwrap_or_default(),
        entities,
        relations,
    })
}

/// Convert a serde_json::Value to a String for property storage.
fn json_val_to_string(v: serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ─── Batch Extraction ────────────────────────────────────────────

/// Build a user message for batch extraction: multiple sections in one LLM call.
///
/// Lists each section with its index, heading chain, heading, and content.
/// The LLM returns a JSON array, one extraction per section (by index).
pub fn build_batch_user_message(sections: &[(usize, &Section)], previous_summary: Option<&str>) -> String {
    let context_note = match previous_summary {
        Some(summary) => format!("[Previous batch context: {}]\n\n", summary),
        None => String::new(),
    };

    let mut body = String::new();
    body.push_str(&context_note);
    body.push_str("Extract entities and relations from the following document sections.\n\n");

    for (batch_idx, section) in sections {
        let heading_chain = section.heading_chain.join(" > ");
        body.push_str(&format!(
            "### Section {}\n**Heading chain**: {}\n**Heading**: {}\n\n{}\n\n",
            batch_idx, heading_chain, section.heading, section.content
        ));
    }

    body.push_str(
        "Return a JSON array where each element corresponds to one section, in order:\n\n\
         [\n  {\n    \"section_summary\": \"one-sentence summary\",\n    \"entities\": [...],\n    \"relations\": [...]\n  },\n  ...\n]\n\n\
         Follow the same entity/relation extraction rules as instructed.\n\
         If a section has no extractable content, return {\"section_summary\": \"\", \"entities\": [], \"relations\": []} for that index."
    );

    body
}

/// Parse a batch LLM response (JSON array) into per-section `SectionExtraction` results.
///
/// Returns a Vec with length equal to `expected_count`. Missing/invalid entries are
/// replaced with empty extractions.
pub fn parse_batch_response(
    response_text: &str,
    sections: &[(usize, &Section)],
) -> Result<Vec<SectionExtraction>, String> {
    let cleaned = clean_json(response_text);
    let parsed: Vec<LlmExtraction> =
        serde_json::from_str(&cleaned).map_err(|e| {
            format!(
                "Failed to parse batch LLM response as JSON array: {}\nRaw (first 500): {}",
                e,
                &cleaned[..cleaned.len().min(500)]
            )
        })?;

    let mut results = Vec::with_capacity(sections.len());
    for (i, (batch_idx, section)) in sections.iter().enumerate() {
        let entry = parsed.get(i).cloned().unwrap_or(LlmExtraction {
            section_summary: None,
            entities: None,
            relations: None,
        });

        let entities = entry
            .entities
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| {
                let id = e.id.filter(|s| !s.is_empty())?;
                let labels = e.labels.unwrap_or_default();
                if labels.is_empty() {
                    return None;
                }
                Some(ExtractedEntity {
                    id,
                    labels,
                    properties: e.properties.unwrap_or_default()
                        .into_iter().map(|(k, v)| (k, json_val_to_string(v))).collect(),
                })
            })
            .collect();

        let relations = entry
            .relations
            .unwrap_or_default()
            .into_iter()
            .filter_map(|r| {
                let source = r.source.filter(|s| !s.is_empty())?;
                let target = r.target.filter(|s| !s.is_empty())?;
                let label = r.label.filter(|s| !s.is_empty())?;
                Some(ExtractedRelation {
                    source,
                    target,
                    label,
                    properties: r.properties.unwrap_or_default()
                        .into_iter().map(|(k, v)| (k, json_val_to_string(v))).collect(),
                })
            })
            .collect();

        results.push(SectionExtraction {
            heading: section.heading.clone(),
            summary: entry.section_summary.unwrap_or_default(),
            entities,
            relations,
        });
    }

    Ok(results)
}

// ─── JSON Helpers ────────────────────────────────────────────────

/// Strip markdown code fences and other common LLM wrapping artifacts.
fn clean_json(text: &str) -> String {
    let text = text.trim();
    // Remove ```json ... ``` fences
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
    fn test_clean_json_removes_fences() {
        let raw = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(clean_json(raw), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_clean_json_passthrough() {
        let raw = "{\"key\": \"value\"}";
        assert_eq!(clean_json(raw), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_parse_response_empty() {
        let result = parse_response("Test", r#"{"section_summary":"none","entities":[],"relations":[]}"#).unwrap();
        assert_eq!(result.summary, "none");
        assert!(result.entities.is_empty());
        assert!(result.relations.is_empty());
    }

    #[test]
    fn test_parse_response_with_entities() {
        let json = r#"{
            "section_summary": "Introduction to the system",
            "entities": [
                {"id": "BionicGraph", "labels": ["project", "software"], "properties": {"description": "A graph index"}},
                {"id": "Rust", "labels": ["language"], "properties": {}}
            ],
            "relations": [
                {"source": "BionicGraph", "target": "Rust", "label": "written_in", "properties": {}}
            ]
        }"#;
        let result = parse_response("Overview", json).unwrap();
        assert_eq!(result.entities.len(), 2);
        assert_eq!(result.relations.len(), 1);
        assert_eq!(result.entities[0].id, "BionicGraph");
        assert_eq!(result.relations[0].label, "written_in");
    }

    #[test]
    fn test_build_batch_user_message() {
        let s1 = Section {
            heading: "Intro".to_string(),
            depth: 1,
            content: "First section.".to_string(),
            heading_chain: vec!["Intro".to_string()],
            index: 0,
        };
        let s2 = Section {
            heading: "Details".to_string(),
            depth: 2,
            content: "Second section.".to_string(),
            heading_chain: vec!["Intro".to_string(), "Details".to_string()],
            index: 1,
        };
        let sections = vec![(0, &s1), (1, &s2)];
        let msg = build_batch_user_message(&sections, Some("Prev summary"));
        assert!(msg.contains("### Section 0"));
        assert!(msg.contains("### Section 1"));
        assert!(msg.contains("First section."));
        assert!(msg.contains("Second section."));
        assert!(msg.contains("JSON array"));
    }

    #[test]
    fn test_parse_batch_response_basic() {
        let s1 = Section {
            heading: "A".to_string(), depth: 1, content: "x".to_string(),
            heading_chain: vec!["A".to_string()], index: 0,
        };
        let s2 = Section {
            heading: "B".to_string(), depth: 1, content: "y".to_string(),
            heading_chain: vec!["B".to_string()], index: 1,
        };
        let sections = vec![(0, &s1), (1, &s2)];
        let json = r#"[
            {"section_summary": "Section A", "entities": [{"id": "X", "labels": ["concept"]}], "relations": []},
            {"section_summary": "Section B", "entities": [], "relations": []}
        ]"#;
        let results = parse_batch_response(json, &sections).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].summary, "Section A");
        assert_eq!(results[0].entities.len(), 1);
        assert_eq!(results[0].entities[0].id, "X");
        assert_eq!(results[1].summary, "Section B");
        assert!(results[1].entities.is_empty());
    }

    #[test]
    fn test_parse_batch_response_fewer_items() {
        let s1 = Section {
            heading: "A".to_string(), depth: 1, content: "x".to_string(),
            heading_chain: vec!["A".to_string()], index: 0,
        };
        let s2 = Section {
            heading: "B".to_string(), depth: 1, content: "y".to_string(),
            heading_chain: vec!["B".to_string()], index: 1,
        };
        let sections = vec![(0, &s1), (1, &s2)];
        let json = r#"[
            {"section_summary": "Only A", "entities": [], "relations": []}
        ]"#;
        let results = parse_batch_response(json, &sections).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].summary, "Only A");
        assert_eq!(results[1].summary, ""); // second is empty default
    }

    #[test]
    fn test_build_user_message() {
        let section = Section {
            heading: "Installation".to_string(),
            depth: 2,
            content: "Run cargo build.".to_string(),
            heading_chain: vec!["Getting Started".to_string(), "Installation".to_string()],
            index: 1,
        };
        let msg = build_user_message(&section, Some("Previous info"));
        assert!(msg.contains("Previous section context"));
        assert!(msg.contains("Getting Started > Installation"));
        assert!(msg.contains("Run cargo build."));
    }
}
