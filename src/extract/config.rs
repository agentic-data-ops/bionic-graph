/// Configuration for the document knowledge extraction pipeline.
///
/// Controls how Markdown documents are processed: LLM endpoint, model selection,
/// context window limits, and retry behavior.
#[derive(Debug, Clone)]
pub struct ExtractionConfig {
    // ─── LLM API ───────────────────────────────────────────────

    /// OpenAI-compatible API endpoint (e.g. "https://api.deepseek.com/v1").
    pub api_base_url: String,

    /// API key. Best loaded from env var e.g. `BGRAPH_LLM_API_KEY`.
    pub api_key: String,

    /// Model identifier (default: "deepseek-v4-flash").
    pub model: String,

    // ─── Token Budget ──────────────────────────────────────────

    /// Maximum context window in tokens. Prompt + section content must fit.
    /// deepseek-v4-flash = 65536; GPT-4o = 128000; adjust per model.
    pub context_window: usize,

    /// Maximum tokens in the LLM response.
    pub max_output_tokens: usize,

    /// Estimated overhead for system prompt + user instructions (tokens).
    /// Subtracted from context_window when calculating available section space.
    pub prompt_overhead_tokens: usize,

    // ─── Execution ─────────────────────────────────────────────

    /// How many times to retry on API failure or malformed response.
    pub max_retries: u32,

    /// How many sections to process concurrently (1 = sequential, safe for rate limits).
    pub concurrent_sections: usize,

    /// Whether to include the previous section summary as context.
    pub pass_section_context: bool,

    /// Number of sections to extract in a single LLM call (1 = sequential per-section).
    /// Higher values reduce API calls but use more context per call.
    pub batch_size: usize,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            api_base_url: "https://api.deepseek.com/v1".to_string(),
            api_key: String::new(),
            model: "deepseek-v4-flash".to_string(),
            context_window: 65536,
            max_output_tokens: 16384,
            prompt_overhead_tokens: 4096,
            max_retries: 3,
            concurrent_sections: 3,
            pass_section_context: true,
            batch_size: 5,
        }
    }
}

impl ExtractionConfig {
    /// Create from `crate::config::Settings` (loaded from settings.json).
    pub fn from_settings(s: &crate::config::Settings) -> Self {
        let api_key = std::env::var("BGRAPH_LLM_API_KEY")
            .or_else(|_| std::env::var("BGRAPH_EXTRACT_API_KEY"))
            .unwrap_or_default();

        Self {
            api_base_url: s.extraction.api_base_url.clone(),
            api_key,
            model: s.extraction.model.clone(),
            context_window: s.extraction.context_window,
            max_output_tokens: s.extraction.max_output_tokens,
            prompt_overhead_tokens: 4096,
            max_retries: s.extraction.max_retries,
            concurrent_sections: s.extraction.concurrent_sections,
            pass_section_context: s.extraction.pass_section_context,
            batch_size: s.extraction.batch_size,
        }
    }

    /// Create a new config with the required API key.
    ///
    /// By default reads from the `BGRAPH_LLM_API_KEY` environment variable.
    /// All other fields use defaults (deepseek-v4-flash tuned).
    pub fn from_env() -> Result<Self, String> {
        let api_key = std::env::var("BGRAPH_LLM_API_KEY")
            .or_else(|_| std::env::var("BGRAPH_EXTRACT_API_KEY"))
            .map_err(|_| {
                "No API key found. Set BGRAPH_LLM_API_KEY env var.".to_string()
            })?;

        Ok(Self {
            api_key,
            ..Default::default()
        })
    }

    /// Available capacity per section (context_window - overhead).
    pub fn section_token_budget(&self) -> usize {
        self.context_window
            .saturating_sub(self.prompt_overhead_tokens)
            .saturating_sub(self.max_output_tokens)
    }

    /// Rough token estimate for a text string (char/4 ≈ token).
    pub fn estimate_tokens(text: &str) -> usize {
        text.len() / 4 + 1
    }
}

// ─── Data types returned by extraction ───────────────────────────

/// An entity extracted from a document section.
#[derive(Debug, Clone)]
pub struct ExtractedEntity {
    /// Unique identifier within this extraction run (e.g. "DeepSeekV4Flash").
    pub id: String,
    /// Type labels (e.g. ["model", "llm", "technology"]).
    pub labels: Vec<String>,
    /// Additional properties discovered.
    pub properties: std::collections::HashMap<String, String>,
}

/// A relationship between two extracted entities.
#[derive(Debug, Clone)]
pub struct ExtractedRelation {
    pub source: String,
    pub target: String,
    pub label: String,
    pub properties: std::collections::HashMap<String, String>,
}

/// Result of extracting a single section.
#[derive(Debug, Clone)]
pub struct SectionExtraction {
    pub heading: String,
    pub summary: String,
    pub entities: Vec<ExtractedEntity>,
    pub relations: Vec<ExtractedRelation>,
}
