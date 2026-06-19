use std::fs;

/// One logical section of a Markdown document.
#[derive(Debug, Clone)]
pub struct Section {
    /// The heading text (e.g. "Installation" for `## Installation`).
    pub heading: String,
    /// Heading depth (1 = `#`, 2 = `##`, 3 = `###`, etc.).
    pub depth: usize,
    /// The body content between this heading and the next heading at same or higher depth.
    pub content: String,
    /// Full heading chain from root, e.g. ["Getting Started", "Installation"].
    pub heading_chain: Vec<String>,
    /// 0-based section index in the document.
    pub index: usize,
}

/// Read a Markdown file and split it into sections by headings.
///
/// Splitting rules:
/// - `# Title` (depth 1) starts a new top-level section
/// - `## Section` (depth 2) starts a subsection
/// - Content before the first heading is treated as a "preamble" section
///
/// If a section's estimated token size exceeds `max_tokens`, it is
/// recursively split by sub-headings or truncated.
pub fn read_markdown(path: &str) -> Result<Vec<Section>, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path, e))?;
    split_sections(&text)
}

/// Split Markdown text into sections by heading.
pub fn split_sections(text: &str) -> Result<Vec<Section>, String> {
    let mut sections: Vec<Section> = Vec::new();

    // Split into lines for processing
    let mut current_heading = String::from("(preamble)");
    let mut current_depth: usize = 0;
    let mut current_content = String::new();
    let mut current_chain: Vec<String> = Vec::new();
    let mut heading_depths: Vec<(usize, String)> = Vec::new(); // stack tracking heading hierarchy

    for line in text.lines() {
        if let Some(heading) = parse_heading(line) {
            // Save the previous section
            if !current_content.trim().is_empty() || sections.is_empty() {
                let idx = sections.len();
                sections.push(Section {
                    heading: current_heading.clone(),
                    depth: current_depth,
                    content: current_content.trim().to_string(),
                    heading_chain: current_chain.clone(),
                    index: idx,
                });
            }

            // Start new section
            let (depth, title) = heading;

            // Update heading chain
            // Pop headings that are at same or deeper level
            while let Some(&(d, _)) = heading_depths.last() {
                if d >= depth {
                    heading_depths.pop();
                } else {
                    break;
                }
            }
            heading_depths.push((depth, title.clone()));
            current_chain = heading_depths.iter().map(|(_, t)| t.clone()).collect();

            current_heading = title;
            current_depth = depth;
            current_content = String::new();
        } else {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    // Save the last section
    let idx = sections.len();
    sections.push(Section {
        heading: current_heading,
        depth: current_depth,
        content: current_content.trim().to_string(),
        heading_chain: current_chain,
        index: idx,
    });

    // Filter out truly empty sections
    sections.retain(|s| !s.content.is_empty() || s.heading != "(preamble)");

    if sections.is_empty() {
        return Err("No sections found in document".to_string());
    }

    Ok(sections)
}

/// Parse a Markdown heading line. Returns (depth, title_text) or None.
fn parse_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') {
        return None;
    }
    // Count the # marks
    let depth = trimmed.chars().take_while(|&c| c == '#').count();
    if depth > 6 {
        return None; // Not a valid heading
    }
    let title = trimmed[depth..].trim().to_string();
    if title.is_empty() {
        return None;
    }
    Some((depth, title))
}

/// Recursively split a section if it exceeds the token budget.
///
/// Splits by sub-headings first; if none exist, truncates the content.
pub fn ensure_fits_budget(
    section: &Section,
    max_tokens: usize,
) -> Vec<Section> {
    let estimated = super::config::ExtractionConfig::estimate_tokens(&section.content);
    if estimated <= max_tokens {
        return vec![section.clone()];
    }

    // Try splitting by sub-headings
    let sub_sections = split_by_subheadings(section);
    if sub_sections.len() > 1 {
        // Recurse on each sub-section
        let mut result = Vec::new();
        for ss in sub_sections {
            result.extend(ensure_fits_budget(&ss, max_tokens));
        }
        return result;
    }

    // No sub-headings — truncate
    let mut truncated = section.clone();
    let max_chars = max_tokens * 4;
    if truncated.content.len() > max_chars {
        truncated.content = format!(
            "{}...\n\n**[TRUNCATED: original section exceeds token budget]**",
            &truncated.content[..max_chars]
        );
    }
    vec![truncated]
}

/// Split a section by its internal sub-headings (`###` or deeper).
fn split_by_subheadings(section: &Section) -> Vec<Section> {
    let mut sub_sections = Vec::new();
    let mut current_content = String::new();
    let mut current_heading = String::new();
    let mut current_depth = 0;
    let mut has_sub = false;

    for line in section.content.lines() {
        if let Some((depth, title)) = parse_heading(line) {
            if depth > section.depth {
                // Save previous
                if !current_heading.is_empty() {
                    let h = current_heading.clone();
                    sub_sections.push(Section {
                        heading: h,
                        depth: current_depth,
                        content: current_content.trim().to_string(),
                        heading_chain: {
                            let mut chain = section.heading_chain.clone();
                            chain.push(current_heading.clone());
                            chain
                        },
                        index: section.index,
                    });
                }
                current_heading = title;
                current_depth = depth;
                current_content = String::new();
                has_sub = true;
                continue;
            }
        }
        if !current_content.is_empty() {
            current_content.push('\n');
        }
        current_content.push_str(line);
    }

    // Last sub-section
    if has_sub && !current_heading.is_empty() {
        let h = current_heading.clone();
        sub_sections.push(Section {
            heading: h,
            depth: current_depth,
            content: current_content.trim().to_string(),
            heading_chain: {
                let mut chain = section.heading_chain.clone();
                chain.push(current_heading);
                chain
            },
            index: section.index,
        });
    }

    if !has_sub {
        return vec![section.clone()];
    }

    sub_sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_h1() {
        assert_eq!(parse_heading("# Hello"), Some((1, "Hello".to_string())));
    }

    #[test]
    fn test_parse_h2() {
        assert_eq!(
            parse_heading("## Section Title"),
            Some((2, "Section Title".to_string()))
        );
    }

    #[test]
    fn test_parse_heading_with_spaces() {
        assert_eq!(
            parse_heading("  ###  Deep  Heading  "),
            Some((3, "Deep  Heading".to_string()))
        );
    }

    #[test]
    fn test_parse_not_a_heading() {
        assert_eq!(parse_heading("Not # a heading"), None);
        assert_eq!(parse_heading("####### too deep"), None);
    }

    #[test]
    fn test_split_sections_basic() {
        let md = "\
# Title

Some intro text.

## Section 1

Content of section 1.

## Section 2

Content of section 2.

### Subsection 2.1

Sub content.";

        let sections = split_sections(md).unwrap();
        // Title + Section 1 + Section 2 + Subsection 2.1
        // (empty preamble is filtered out)
        assert_eq!(sections.len(), 4);

        // Check heading chain of Subsection 2.1 (now index 3)
        let sub = &sections[3];
        assert_eq!(sub.heading, "Subsection 2.1");
        assert!(sub.heading_chain.contains(&"Section 2".to_string()));
    }

    #[test]
    fn test_empty_document_fails() {
        let result = split_sections("");
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_fits_budget_small() {
        let section = Section {
            heading: "Test".to_string(),
            depth: 1,
            content: "Short content.".to_string(),
            heading_chain: vec!["Test".to_string()],
            index: 0,
        };
        let split = ensure_fits_budget(&section, 99999);
        assert_eq!(split.len(), 1);
    }

    #[test]
    fn test_ensure_fits_budget_truncates() {
        let section = Section {
            heading: "Big".to_string(),
            depth: 1,
            content: "A".repeat(1000),
            heading_chain: vec!["Big".to_string()],
            index: 0,
        };
        let split = ensure_fits_budget(&section, 10);
        assert_eq!(split.len(), 1);
        assert!(split[0].content.contains("[TRUNCATED:"));
    }
}
