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
        atomic::{AtomicU32, Ordering},
        Arc, RwLock,
    },
};

use serde::{Deserialize, Serialize};
use crate::lock::lock_manager::LockManager;
use crate::storage::{
    bitmap_file::BitmapFile,
    block_cache::BlockCache,
    data_file::DataFile,
    index_file::IndexFile,
    memory_index::MemoryIndex,
    memory_index_builder,
    redo_log::RedoLog,
    types::{StorageError, StorageResult},
};

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
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            storage: GraphStorageConfig::default(),
            lock: GraphLockConfig::default(),
        }
    }
}

/// 存储引擎配置段
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphStorageConfig {
    /// LRU 块缓存容量（块数 × 16KB = 内存占用）。默认 4096 = 64 MB
    pub cache_capacity: usize,
    /// WAL 文件旋转大小（MB）
    pub rotation_threshold_mb: u64,
    /// WAL 文件旋转时间（秒）。超过此时间自动旋转。null 表示不启用时间旋转
    pub rotation_max_age_secs: Option<u64>,
    /// 位图空闲块列表预填充数量
    pub free_list_target: usize,
}

impl Default for GraphStorageConfig {
    fn default() -> Self {
        Self {
            cache_capacity: 4096,
            rotation_threshold_mb: 64,
            rotation_max_age_secs: Some(900),
            free_list_target: 128,
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
    pub block_cache: RwLock<BlockCache>,
    pub redo_log: RedoLog,
    pub index_file: IndexFile,

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
        let bitmap_file = RwLock::new(BitmapFile::open(graph_dir.join("bitmap"), data_blocks)?);
        let block_cache = RwLock::new(BlockCache::new(config.storage.cache_capacity));
        let redo_log = RedoLog::open_with_config(
            &graph_dir,
            config.storage.rotation_threshold_mb * 1024 * 1024,
            config.storage.rotation_max_age_secs,
        )?;
        let index_file = IndexFile::open(graph_dir.join("index"))?;

        // ── Rebuild in-memory index ──────────────────────────────────────
        let memory_index = RwLock::new(memory_index_builder::build_memory_index(&index_file)?);

        // ── Determine next IDs from the in-memory index ────────────────
        // No need to persist these — on restart, the index is rebuilt from
        // the index file, and max_id + 1 gives the correct next ID.
        let max_vid = {
            let mi = memory_index.read().unwrap_or_else(|e| e.into_inner());
            mi.vertices.keys().last().copied().unwrap_or(0)
        };
        let max_eid = {
            let mi = memory_index.read().unwrap_or_else(|e| e.into_inner());
            mi.edges.keys().last().copied().unwrap_or(0)
        };

        let graph = Arc::new(Self {
            name: name.to_string(),
            dir: graph_dir.clone(),
            data_file,
            bitmap_file,
            block_cache,
            redo_log,
            index_file,
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
        let g = Arc::downgrade(&graph);
        RedoLog::replay(&graph_dir, |entry| {
            let graph = g.upgrade().ok_or_else(|| StorageError::Other("graph dropped during replay".into()))?;
            crate::graph::crud::replay_entry(&graph, &entry)
        })?;

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
        let mut cache = self.block_cache.write().unwrap_or_else(|e| e.into_inner());
        cache.flush_dirty(&|idx, data| {
            self.data_file.write_block(idx, data)?;
            Ok(())
        })?;
        self.index_file.flush_dirty()?;
        self.redo_log.sync()?;
        Ok(())
    }

    /// Close the graph — flush everything and checkpoint the WAL.
    /// Close the graph, persisting current state and counters.
    pub fn close(&self) -> StorageResult<()> {
        self.flush()?;
        self.redo_log.sync()?;
        self.redo_log.renew()?;
        Ok(())
    }
}
