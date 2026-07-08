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

// ─── LLM Provider ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProvider {
    pub name: String,
    pub api_base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub providers: Vec<LlmProvider>,
    pub default_model: String,
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub max_retries: u32,
}

impl LlmConfig {
    pub fn parse_default_model(&self) -> (&str, &str) {
        if let Some(slash) = self.default_model.find('/') {
            let provider = &self.default_model[..slash];
            let model = &self.default_model[slash + 1..];
            (provider, model)
        } else {
            ("", &self.default_model)
        }
    }

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
    /// 图数据根目录
    pub data_dir: String,
    /// WAL 文件旋转大小阈值（MB），覆盖 per-graph config.json。null 表示使用代码默认值（64MB）
    pub redolog_rotation_max_size_mb: Option<u64>,
    /// WAL 文件旋转时间阈值（秒），覆盖 per-graph config.json。null 表示使用代码默认值（900s）
    pub redolog_rotation_max_age_secs: Option<u64>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: "data".to_string(),
            redolog_rotation_max_size_mb: None,
            redolog_rotation_max_age_secs: None,
        }
    }
}

// ─── Cluster ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClusterConfig {
    /// 是否启用集群模式
    pub enabled: bool,
    /// 节点间通信监听地址
    pub bind_addr: String,
    /// Worker 专属：Master 的地址
    pub master_addr: Option<String>,
    /// 心跳检测间隔（秒）
    pub heartbeat_interval_secs: u64,
    /// Worker 超时阈值（秒）
    pub worker_timeout_secs: u64,
    /// Worker 是否将写操作转发到 Master
    pub forward_writes: bool,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: "0.0.0.0:9090".to_string(),
            master_addr: None,
            heartbeat_interval_secs: 5,
            worker_timeout_secs: 30,
            forward_writes: true,
        }
    }
}

// ─── Search ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExploreConfig {
    /// 是否从搜索结果自动遍历
    pub traverse: bool,
    /// 关键词匹配模式: "prefix" | "word"
    pub match_mode: String,
    /// 激活传播阈值 (0.0-1.0)
    pub activate: f32,
    /// 每跳衰减值 (0.0-1.0)
    pub decay: f32,
    /// 最大 BFS 遍历深度
    pub depth: u8,
    /// 遍历结果最低分值 (0.0-1.0)
    pub score: f32,
}

impl Default for ExploreConfig {
    fn default() -> Self {
        Self {
            traverse: true,
            match_mode: "prefix".to_string(),
            activate: 0.2,
            decay: 0.95,
            depth: 16,
            score: 0.1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    pub greedy: ExploreConfig,
    pub exact: ExploreConfig,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            greedy: ExploreConfig {
                traverse: true,
                match_mode: "prefix".to_string(),
                activate: 0.2,
                decay: 0.95,
                depth: 16,
                score: 0.1,
            },
            exact: ExploreConfig {
                traverse: true,
                match_mode: "word".to_string(),
                activate: 0.6,
                decay: 0.8,
                depth: 4,
                score: 0.2,
            },
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
    pub cluster: ClusterConfig,
    pub search: SearchSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            llm: LlmConfig::default(),
            storage: StorageConfig::default(),
            cluster: ClusterConfig::default(),
            search: SearchSettings::default(),
        }
    }
}
