//! Central graph facade — ties together storage, indexing, and WAL.
//!
//! # Lifecycle
//!
//! 1. `Graph::open(dir, name)` — loads existing graph, replays WAL, rebuilds index
//! 2. CRUD operations — through `crate::graph::crud` methods
//! 3. `Graph::close()` — flushes dirty blocks, syncs all state to disk

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, RwLock,
    },
};

/// Flag set during WAL replay — prevents recursive WAL writes and
/// recursive broadcasting.
pub(crate) static REPLAYING: AtomicBool = AtomicBool::new(false);

use serde::{Deserialize, Serialize};
use crate::lock::lock_manager::LockManager;
use crate::storage::{
    bitmap_file::BitmapFile,
    block_cache::ShardedBlockCache,
    data_file::DataFile,
    memory_index::MemoryIndex,
    memory_index_builder,
    redo_log::RedoLog,
    types::{StorageError, StorageResult},
};

/// 自定义属性索引配置段
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct IndicesConfig {
    /// 已注册的顶点属性 key 列表
    #[serde(default)]
    pub vertex_properties: Vec<String>,
    /// 已注册的边属性 key 列表
    #[serde(default)]
    pub edge_properties: Vec<String>,
}

impl Default for IndicesConfig {
    fn default() -> Self {
        Self {
            vertex_properties: Vec::new(),
            edge_properties: Vec::new(),
        }
    }
}

/// Per-graph configuration, persisted at `<data_dir>/graphs/<name>/config.json`.
///
/// Each graph can independently tune these parameters. Defaults match the
/// engine's built-in constants and can be overridden via `PUT /graphs/:name/config`.

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphConfig {
    /// 存储引擎配置
    #[serde(default)]
    pub storage: GraphStorageConfig,
    /// 锁引擎配置
    #[serde(default)]
    pub lock: GraphLockConfig,
    /// 自定义属性索引配置
    #[serde(default)]
    pub indices: IndicesConfig,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            storage: GraphStorageConfig::default(),
            lock: GraphLockConfig::default(),
            indices: IndicesConfig::default(),
        }
    }
}

/// 存储引擎配置段
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphStorageConfig {
    /// LRU 块缓存容量（MB）。默认 64 MB
    pub lru_cache_size_mb: usize,
    /// WAL 文件旋转大小（MB）
    pub log_rotation_size_mb: u64,
    /// WAL 文件旋转时间（秒）。超过此时间自动旋转。null 表示不启用时间旋转
    pub log_rotation_age_secs: Option<u64>,
    /// 块预分配数量（free list 补货阈值）。
    pub pre_alloc_blocks: usize,
    /// 顶点/边历史记录最大条目数。
    pub time_travel_max_history: usize,
    /// 是否启用日志批量写入（批量导入时合并日志记录）。
    pub log_flush_batch_enable: bool,
    /// WAL 批量 flush 的批次大小。
    pub log_flush_batch_size: usize,
    /// WAL 批量缓存在内存中的最大时长（微秒）。达到该时间时自动 flush，无论批次是否已满。0 表示禁用时间触发。
    pub log_flush_max_age_us: u64,
}

impl Default for GraphStorageConfig {
    fn default() -> Self {
        Self {
            lru_cache_size_mb: 64,
            log_rotation_size_mb: 64,
            log_rotation_age_secs: Some(900),
            pre_alloc_blocks: 128,
            time_travel_max_history: 32,
            log_flush_batch_enable: false,
            log_flush_batch_size: 256,
            log_flush_max_age_us: 1000,
        }
    }
}

impl GraphStorageConfig {
}

/// 锁引擎配置段
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphLockConfig {
    /// 顶点/边锁分片数（必须为 2 的幂）
    pub stripe_count: usize,
    /// 数据块锁分片数（必须为 2 的幂）
    pub block_stripe_count: usize,
}

impl Default for GraphLockConfig {
    fn default() -> Self {
        Self {
            stripe_count: 1024,
            block_stripe_count: 256,
        }
    }
}

impl GraphConfig {
    /// Load per-graph config from `<graph_dir>/config.json`.
    /// If the file doesn't exist, returns default.
    pub fn load(graph_dir: &Path) -> Self {
        let path = graph_dir.join("config.json");
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    /// Save per-graph config to `<graph_dir>/config.json`.
    pub fn save(&self, graph_dir: &Path) -> StorageResult<()> {
        let path = graph_dir.join("config.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| StorageError::Other(format!("serialize config: {}", e)))?;
        std::fs::write(&path, &json)?;
        Ok(())
    }
}

/// A single graph instance backed by block-based storage.
///
/// All mutating operations write through: WAL → cache. At checkpoint / close,
/// dirty blocks are flushed to the data file.
pub struct Graph {
    pub name: String,
    pub dir: PathBuf,

    // ── Storage engine ───────────────────────────────────────────────────
    pub data_file: DataFile,
    pub bitmap_file: RwLock<BitmapFile>,
    pub block_cache: ShardedBlockCache,
    pub redo_log: RedoLog,

    // ── In-memory index ──────────────────────────────────────────────────
    pub memory_index: RwLock<MemoryIndex>,

    // ── Concurrency locks ────────────────────────────────────────────────
    pub locks: LockManager,

    // ── ID counters ──────────────────────────────────────────────────────
    pub next_vertex_id: AtomicU32,
    pub next_edge_id: AtomicU32,
    pub next_token_id: AtomicU32,

    // ── Config ───────────────────────────────────────────────────────────
    pub config: GraphConfig,
}

impl Graph {
    /// Open an existing graph (or create a new one) at `dir / name`.
    ///
    /// This is the main entry point. On first call for a new graph, the
    /// storage files are created. On subsequent calls, the redo log is
    /// replayed and the in-memory index rebuilt.
    pub fn open<P: AsRef<Path>>(dir: P, name: &str) -> StorageResult<Arc<Self>> {
        let graph_dir = dir.as_ref().join(name);
        std::fs::create_dir_all(&graph_dir)?;

        // Load per-graph config (falls back to defaults if no config.json)
        let config = GraphConfig::load(&graph_dir);

        // If no config.json existed, write the defaults to disk so
        // administrators can inspect and tune them via the config API.
        if !graph_dir.join("config.json").exists() {
            let _ = config.save(&graph_dir);
        }

        // ── Open storage files ───────────────────────────────────────────
        let data_file = DataFile::open(graph_dir.join("data"))?;
        let data_blocks = data_file.block_count()?;
        let bitmap_file = RwLock::new(BitmapFile::open(graph_dir.join("bitmap"), data_blocks, config.storage.pre_alloc_blocks)?);
        // Convert MB to block count (each block is 16 KB)
        let block_cache_capacity = config.storage.lru_cache_size_mb * 1024 * 1024 / crate::storage::types::BLOCK_SIZE;
        let block_cache = ShardedBlockCache::new(block_cache_capacity, crate::storage::block_cache::DEFAULT_SHARD_COUNT);
        let redo_log = RedoLog::open_with_config(
            &graph_dir,
            config.storage.log_rotation_size_mb * 1024 * 1024,
            config.storage.log_rotation_age_secs,
        )?;
        // ── Rebuild in-memory index ──────────────────────────────────────
        let memory_index = RwLock::new(
            match MemoryIndex::load_from_dir(&graph_dir.join("index"))? {
                Some(mi) => mi,
                None => memory_index_builder::build_memory_index(&data_file, &config.indices.vertex_properties, &config.indices.edge_properties)?,
            }
        );

        // Ensure all property keys from config are registered (in case index
        // was loaded from file but config was updated with new keys).
        {
            let mut mi = memory_index.write().unwrap_or_else(|e| e.into_inner());
            for key in &config.indices.vertex_properties {
                if !mi.has_vertex_property(key) {
                    mi.register_vertex_property(key);
                }
            }
            for key in &config.indices.edge_properties {
                if !mi.has_edge_property(key) {
                    mi.register_edge_property(key);
                }
            }
        }

        // ── Determine next IDs from the in-memory index ────────────────
        let max_vid = {
            let mi = memory_index.read().unwrap_or_else(|e| e.into_inner());
            mi.vertex_id.keys().last().copied().unwrap_or(0)
        };
        let max_eid = {
            let mi = memory_index.read().unwrap_or_else(|e| e.into_inner());
            mi.edge_id.keys().last().copied().unwrap_or(0)
        };

        let graph = Arc::new(Self {
            name: name.to_string(),
            dir: graph_dir.clone(),
            data_file,
            bitmap_file,
            block_cache,
            redo_log,
            memory_index,
            locks: LockManager::new(),
            next_vertex_id: AtomicU32::new(max_vid + 1),
            next_edge_id: AtomicU32::new(max_eid + 1),
            next_token_id: AtomicU32::new(1),
            config,
        });

        // ── Replay redo log ──────────────────────────────────────────────
        // The WAL replay applies any un-checkpointed operations to the
        // in-memory index and data blocks.
        crate::graph::graph::REPLAYING.store(true, Ordering::Relaxed);
        let g = Arc::downgrade(&graph);
        let replay_result = RedoLog::replay(&graph_dir, |entry| {
            let graph = g.upgrade().ok_or_else(|| StorageError::Other("graph dropped during replay".into()))?;
            crate::graph::crud::replay_entry(&graph, &entry)
        });
        crate::graph::graph::REPLAYING.store(false, Ordering::Relaxed);
        replay_result?;

        // After replay, switch to a fresh WAL file so crash recovery
        // during this session works (the file stays on disk with a real
        // directory entry).
        graph.redo_log.renew()?;

        Ok(graph)
    }

    /// Allocate a new vertex ID atomically.
    pub fn alloc_vertex_id(&self) -> u32 {
        self.next_vertex_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Allocate a new edge ID atomically.
    pub fn alloc_edge_id(&self) -> u32 {
        self.next_edge_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Allocate a new token ID atomically.
    pub fn alloc_token_id(&self) -> u32 {
        self.next_token_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Flush all dirty blocks to disk and sync.
    pub fn flush(&self) -> StorageResult<()> {
        self.block_cache.flush_dirty(&|idx, data| {
            self.data_file.write_block(idx, data)?;
            Ok(())
        })?;
        self.redo_log.sync()?;
        Ok(())
    }

    /// Close the graph — flush everything and checkpoint the WAL.
    pub fn close(&self) -> StorageResult<()> {
        self.flush()?;
        self.redo_log.sync()?;
        self.redo_log.renew()?;
        // Persist memory index for faster startup.
        if let Ok(mi) = self.memory_index.read() {
            let _ = mi.save_to_dir(&self.dir.join("index"));
        }
        Ok(())
    }

    /// Persist the current set of registered property index keys to config.json.
    ///
    /// Call this after registering or unregistering custom indices so the
    /// configuration survives restarts and can be used to rebuild indices
    /// after a full data-file scan.
    pub fn persist_indices_config(&self) -> StorageResult<()> {
        let (vp_keys, ep_keys) = {
            let mi = self.memory_index.read().unwrap_or_else(|e| e.into_inner());
            let mut vp = mi.list_vertex_property_keys();
            let mut ep = mi.list_edge_property_keys();
            vp.sort();
            ep.sort();
            (vp, ep)
        };
        let mut new_config = self.config.clone();
        new_config.indices.vertex_properties = vp_keys;
        new_config.indices.edge_properties = ep_keys;
        new_config.save(&self.dir)
    }

    /// Sync the in-memory property index to match `new_indices`.
    ///
    /// Keys present in `new_indices` but not yet registered are registered and
    /// populated by scanning existing vertices/edges. Keys registered but absent
    /// from `new_indices` are unregistered and their index data is dropped.
    ///
    /// Call this when a new `IndicesConfig` is applied via the config API
    /// (`PUT /graphs/:name/config`).
    pub fn sync_indices_from_config(&self, new_indices: &IndicesConfig) -> StorageResult<()> {
        // 1. Identify keys to add and remove.
        let (current_vp, current_ep) = {
            let mi = self.memory_index.read().unwrap_or_else(|e| e.into_inner());
            (mi.list_vertex_property_keys(), mi.list_edge_property_keys())
        };

        let to_register_vp: Vec<&str> = new_indices.vertex_properties.iter()
            .filter(|k| !current_vp.contains(k))
            .map(|s| s.as_str())
            .collect();
        let to_unregister_vp: Vec<&str> = current_vp.iter()
            .filter(|k| !new_indices.vertex_properties.contains(k))
            .map(|s| s.as_str())
            .collect();
        let to_register_ep: Vec<&str> = new_indices.edge_properties.iter()
            .filter(|k| !current_ep.contains(k))
            .map(|s| s.as_str())
            .collect();
        let to_unregister_ep: Vec<&str> = current_ep.iter()
            .filter(|k| !new_indices.edge_properties.contains(k))
            .map(|s| s.as_str())
            .collect();

        // 2. Unregister removed keys.
        if !to_unregister_vp.is_empty() || !to_unregister_ep.is_empty() {
            let mut mi = self.memory_index.write().unwrap_or_else(|e| e.into_inner());
            for key in &to_unregister_vp {
                mi.unregister_vertex_property(key);
            }
            for key in &to_unregister_ep {
                mi.unregister_edge_property(key);
            }
        }

        // 3. Register new keys and scan existing entities to populate the index.
        for key in &to_register_vp {
            {
                let mut mi = self.memory_index.write().unwrap_or_else(|e| e.into_inner());
                mi.register_vertex_property(key);
            }
            Self::scan_vertex_property(self, key);
        }
        for key in &to_register_ep {
            {
                let mut mi = self.memory_index.write().unwrap_or_else(|e| e.into_inner());
                mi.register_edge_property(key);
            }
            Self::scan_edge_property(self, key);
        }

        Ok(())
    }

    /// Scan all vertices and populate the property index for `key`.
    fn scan_vertex_property(graph: &Graph, key: &str) {
        use crate::storage::types::PropertyValue;
        let pairs: Vec<(u32, crate::storage::memory_index::MetaPointer)> = {
            let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
            mi.vertex_id.iter().map(|(&vid, &p)| (vid, p)).collect()
        };
        for (vid, ptr) in &pairs {
            if let Ok(Some(payload)) = crate::graph::crud::get_vertex(graph, *vid) {
                if let Some(val) = payload.properties.get(key) {
                    let s = Self::prop_val_str(val);
                    if !s.is_empty() {
                        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                        mi.insert_vertex_property(key, &s, *ptr);
                    }
                }
            }
        }
    }

    /// Scan all edges and populate the property index for `key`.
    fn scan_edge_property(graph: &Graph, key: &str) {
        use crate::storage::types::PropertyValue;
        let pairs: Vec<(u32, crate::storage::memory_index::MetaPointer)> = {
            let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
            mi.edge_id.iter().map(|(&eid, &p)| (eid, p)).collect()
        };
        for (eid, ptr) in &pairs {
            if let Ok(Some(payload)) = crate::graph::crud::get_edge(graph, *eid) {
                if let Some(val) = payload.properties.get(key) {
                    let s = Self::prop_val_str(val);
                    if !s.is_empty() {
                        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                        mi.insert_edge_property(key, &s, *ptr);
                    }
                }
            }
        }
    }

    fn prop_val_str(pv: &crate::storage::types::PropertyValue) -> String {
        match pv {
            crate::storage::types::PropertyValue::String(s) => s.clone(),
            crate::storage::types::PropertyValue::Integer(i) => i.to_string(),
            crate::storage::types::PropertyValue::Float(f) => f.to_string(),
            crate::storage::types::PropertyValue::Boolean(b) => b.to_string(),
            crate::storage::types::PropertyValue::List(_) | crate::storage::types::PropertyValue::Null => String::new(),
        }
    }
}
