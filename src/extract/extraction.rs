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
3. **IDs**: Use PascalCase identifiers for entity IDs, e.g. "DeepSeekV4Flash", "KnowledgeGraph", "GremlinQuery".
4. **Labels**: Each entity needs at least one label (type). Examples: ["technology"], ["concept", "protocol"], ["organization"].
5. **Properties**: Include relevant details as key-value pairs. Standard keys: "description", "mentioned_in", "version", "url".

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
#[derive(Debug, Deserialize)]
struct LlmExtraction {
    section_summary: Option<String>,
    entities: Option<Vec<LlmEntity>>,
    relations: Option<Vec<LlmRelation>>,
}

#[derive(Debug, Deserialize)]
struct LlmEntity {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    labels: Option<Vec<String>>,
    #[serde(default)]
    properties: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct LlmRelation {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    properties: Option<HashMap<String, String>>,
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
                properties: e.properties.unwrap_or_default(),
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
                properties: r.properties.unwrap_or_default(),
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
