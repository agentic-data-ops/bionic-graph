use serde::{Deserialize, Serialize};

// ─── Server ──────────────────────────────────────────────────────

/// Server listening configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
        }
    }
}

// ─── Extraction (LLM document parser) ────────────────────────────

/// Document knowledge extraction configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtractionConfig {
    pub api_base_url: String,
    pub model: String,
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub max_retries: u32,
    pub concurrent_sections: usize,
    pub pass_section_context: bool,
    pub batch_size: usize,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            api_base_url: "https://api.deepseek.com/v1".to_string(),
            model: "deepseek-v4-flash".to_string(),
            context_window: 65536,
            max_output_tokens: 16384,
            max_retries: 3,
            concurrent_sections: 1,
            pass_section_context: true,
            batch_size: 5,
        }
    }
}

// ─── Storage (disk-backed graph) ─────────────────────────────────

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

// ─── Graph (in-memory builder defaults) ──────────────────────────

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

// ─── Neural (spreading activation defaults) ──────────────────────

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

/// Complete application settings, deserialized from
/// `~/.config/bionic-graph/settings.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub server: ServerConfig,
    pub extraction: ExtractionConfig,
    pub storage: StorageConfig,
    pub graph: GraphConfig,
    pub neural: NeuralConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            extraction: ExtractionConfig::default(),
            storage: StorageConfig::default(),
            graph: GraphConfig::default(),
            neural: NeuralConfig::default(),
        }
    }
}
