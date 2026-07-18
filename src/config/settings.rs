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
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: "data".to_string(),
        }
    }
}

// ─── Cluster ─────────────────────────────────────────────────────

/// Role of this node in the cluster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    /// Single master — handles reads + writes.
    #[serde(rename = "master")]
    Master,
    /// Read replica — proxies writes to the master.
    #[serde(rename = "worker")]
    Worker,
}

impl Default for NodeRole {
    fn default() -> Self {
        Self::Master
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClusterConfig {
    /// 是否启用集群模式
    pub enabled: bool,
    /// 节点角色: "master" 或 "worker"
    pub role: NodeRole,
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
            role: NodeRole::Master,
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

// ─── Rank ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RankConfig {
    /// 更新顶点和边时自增 Rank
    pub auto_inc_rank_when_update: bool,
    /// 读取顶点和边时自增 Rank
    pub auto_inc_rank_when_read: bool,
    /// 不活跃时递减 Rank
    pub auto_dec_rank_when_inactive: bool,
    /// 访问多长时间（秒）之后变为不活跃
    pub inactive_after_accessed_secs: u64,
    /// 不活跃扫描间隔（秒）
    pub inactive_rank_update_period: u64,
}

impl Default for RankConfig {
    fn default() -> Self {
        Self {
            auto_inc_rank_when_update: true,
            auto_inc_rank_when_read: true,
            auto_dec_rank_when_inactive: true,
            inactive_after_accessed_secs: 1_296_000, // 15 days
            inactive_rank_update_period: 86_400,    // 1 day
        }
    }
}

// ─── Web Search ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchProvider {
    pub id: String,
    pub name: String,
    /// URL template with {text} placeholder, e.g. "https://cn.bing.com/search?q={text}"
    pub search_url: String,
    /// HTTP method: GET or POST (default "GET")
    #[serde(default)]
    pub method: String,
    /// JSON body template for POST, with {text} placeholder
    #[serde(default)]
    pub body_template: Option<String>,
    #[serde(default)]
    pub params: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebSearchConfig {
    pub providers: Vec<WebSearchProvider>,
    pub default_provider: String,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        let mut headers = std::collections::HashMap::new();
        headers.insert(
            "Content-Type".to_string(),
            "application/json".to_string(),
        );
        Self {
            providers: vec![WebSearchProvider {
                id: "baidu".to_string(),
                name: "百度搜索".to_string(),
                search_url: "https://qianfan.baidubce.com/v2/ai_search/web_search".to_string(),
                method: "POST".to_string(),
                body_template: Some(r#"{"messages":[{"content":"{text}","role":"user"}],"search_source":"baidu_search_v2","resource_type_filter":[{"type":"web","top_k":5}],"search_recency_filter":"year"}"#.to_string()),
                params: std::collections::HashMap::new(),
                headers,
            }],
            default_provider: "baidu".to_string(),
        }
    }
}

// ─── Graph (storage, search, rank) ────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphConfig {
    pub storage: StorageConfig,
    pub search: SearchSettings,
    pub rank: RankConfig,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            storage: StorageConfig::default(),
            search: SearchSettings::default(),
            rank: RankConfig::default(),
        }
    }
}

// ─── Top-level Settings ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub server: ServerConfig,
    pub llm: LlmConfig,
    pub cluster: ClusterConfig,
    pub web_search: WebSearchConfig,
    pub graph: GraphConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            llm: LlmConfig::default(),
            cluster: ClusterConfig::default(),
            web_search: WebSearchConfig::default(),
            graph: GraphConfig::default(),
        }
    }
}
