/// Configuration for the document knowledge extraction pipeline.
///
/// Built dynamically from the backend's `LlmConfig` providers list.
#[derive(Debug, Clone)]
pub struct ExtractionConfig {
    // ─── LLM API ───────────────────────────────────────────────
    pub api_base_url: String,
    pub api_key: String,
    pub model: String,

    // ─── Token Budget ──────────────────────────────────────────
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub prompt_overhead_tokens: usize,

    // ─── Execution ─────────────────────────────────────────────
    pub max_retries: u32,
    pub concurrent_sections: usize,
    pub pass_section_context: bool,
    pub batch_size: usize,
}

impl ExtractionConfig {
    /// Build from the backend's `LlmConfig`, resolving "Provider/Model" format.
    pub fn from_llm_config(llm: &crate::config::LlmConfig) -> Self {
        let (api_key, api_base_url, model) = llm.resolve_default();
        Self {
            api_base_url,
            api_key,
            model,
            context_window: llm.context_window,
            max_output_tokens: llm.max_output_tokens,
            prompt_overhead_tokens: 4096,
            max_retries: llm.max_retries,
            concurrent_sections: 1,
            pass_section_context: true,
            batch_size: 5,
        }
    }

    pub fn section_token_budget(&self) -> usize {
        self.context_window
            .saturating_sub(self.prompt_overhead_tokens)
            .saturating_sub(self.max_output_tokens)
    }

    pub fn estimate_tokens(text: &str) -> usize {
        text.len() / 4 + 1
    }
}

// ─── Data types returned by extraction ───────────────────────────

#[derive(Debug, Clone)]
pub struct ExtractedEntity {
    pub name: String,
    pub labels: Vec<String>,
    pub keywords: Vec<String>,
    pub properties: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedRelation {
    pub source: String,
    pub target: String,
    pub name: String,
    pub labels: Vec<String>,
    pub keywords: Vec<String>,
    pub strength: f32,
    pub properties: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SectionExtraction {
    pub heading: String,
    pub summary: String,
    pub entities: Vec<ExtractedEntity>,
    pub relations: Vec<ExtractedRelation>,
}
