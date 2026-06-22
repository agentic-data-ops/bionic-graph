use serde::{Deserialize, Serialize};

// ─── Server ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { host: "127.0.0.1".to_string(), port: 8080 }
    }
}

// ─── LLM Provider (multi-vendor, each with string-list of models) ─

/// A single LLM provider (vendor) with its API endpoint and model list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProvider {
    pub name: String,
    pub api_base_url: String,
    pub api_key: String,
    /// Model names as plain strings, e.g. ["deepseek-v4-flash", "deepseek-v4-pro"]
    pub models: Vec<String>,
}

/// Full LLM configuration.
/// `default_model` uses format `"<provider_name>/<model_name>"`, e.g. `"DeepSeek/deepseek-v4-flash"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub providers: Vec<LlmProvider>,
    /// e.g. "DeepSeek/deepseek-v4-flash"
    pub default_model: String,
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub max_retries: u32,
}

impl LlmConfig {
    /// Parse `default_model` ("Provider/Model") into (provider_name, model_name).
    pub fn parse_default_model(&self) -> (&str, &str) {
        if let Some(slash) = self.default_model.find('/') {
            let provider = &self.default_model[..slash];
            let model = &self.default_model[slash + 1..];
            (provider, model)
        } else {
            ("", &self.default_model)
        }
    }

    /// Find the provider by name and return its api_key + api_base_url + resolved model name.
    pub fn resolve_default(&self) -> (String, String, String) {
        let (prov_name, model_name) = self.parse_default_model();
        if let Some(prov) = self.providers.iter().find(|p| p.name == prov_name) {
            (prov.api_key.clone(), prov.api_base_url.clone(), model_name.to_string())
        } else if let Some(first) = self.providers.first() {
            (first.api_key.clone(), first.api_base_url.clone(), model_name.to_string())
        } else {
            (String::new(), "https://api.deepseek.com/v1".to_string(), model_name.to_string())
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            providers: vec![LlmProvider {
                name: "DeepSeek".to_string(),
                api_base_url: "https://api.deepseek.com/v1".to_string(),
                api_key: String::new(),
                models: vec!["deepseek-v4-flash".to_string(), "deepseek-v4-pro".to_string()],
            }],
            default_model: "DeepSeek/deepseek-v4-flash".to_string(),
            context_window: 65536,
            max_output_tokens: 16384,
            max_retries: 3,
        }
    }
}

// ─── Storage ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub data_dir: String,
    pub cache_capacity: usize,
    pub checkpoint_interval_entries: u64,
    pub auto_save_interval_secs: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: "data".to_string(),
            cache_capacity: 1000,
            checkpoint_interval_entries: 1000,
            auto_save_interval_secs: 5,
        }
    }
}

// ─── Graph ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphConfig {
    pub default_vertex_labels: Vec<String>,
    pub max_edges_per_vertex: u32,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            default_vertex_labels: vec!["entity".to_string()],
            max_edges_per_vertex: 10000,
        }
    }
}

// ─── Neural ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NeuralConfig {
    pub default_threshold: f32,
    pub default_decay_rate: f32,
    pub default_refractory_ticks: usize,
    pub learning_enabled: bool,
    pub co_fire_window: usize,
}

impl Default for NeuralConfig {
    fn default() -> Self {
        Self {
            default_threshold: 0.7,
            default_decay_rate: 0.1,
            default_refractory_ticks: 3,
            learning_enabled: true,
            co_fire_window: 5,
        }
    }
}

// ─── Top-level Settings ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub server: ServerConfig,
    pub llm: LlmConfig,
    pub storage: StorageConfig,
    pub graph: GraphConfig,
    pub neural: NeuralConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            llm: LlmConfig::default(),
            storage: StorageConfig::default(),
            graph: GraphConfig::default(),
            neural: NeuralConfig::default(),
        }
    }
}
