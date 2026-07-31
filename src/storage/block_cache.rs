//! LRU block cache with dirty-page tracking and writeback.
//! Split across N independent shards for parallel access.

use std::{
    collections::{HashMap, VecDeque},
    sync::RwLock,
};
use crate::storage::types::{BlockIdx, BLOCK_SIZE, StorageError, StorageResult};

pub const DEFAULT_CACHE_CAPACITY: usize = 4096;
pub const DEFAULT_SHARD_COUNT: usize = 64;

#[derive(Clone, Debug, Default)]
pub struct CacheStats {
    pub hits: u64, pub misses: u64, pub evictions: u64, pub dirty_flushes: u64,
}
impl std::ops::Add for CacheStats {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self { hits: self.hits + rhs.hits, misses: self.misses + rhs.misses,
               evictions: self.evictions + rhs.evictions, dirty_flushes: self.dirty_flushes + rhs.dirty_flushes }
    }
}

struct CachedBlock { data: Box<[u8; BLOCK_SIZE]>, is_dirty: bool }

struct BlockShard {
    blocks: HashMap<BlockIdx, CachedBlock>,
    lru: VecDeque<BlockIdx>,
    capacity: usize,
    stats: CacheStats,
}

impl BlockShard {
    fn new(capacity: usize) -> Self {
        Self { blocks: HashMap::with_capacity(capacity), lru: VecDeque::with_capacity(capacity), capacity, stats: CacheStats::default() }
    }
    fn ensure_loaded<F, G>(&mut self, idx: BlockIdx, loader: F, flusher: &G) -> StorageResult<()>
    where F: FnOnce(BlockIdx) -> StorageResult<[u8; BLOCK_SIZE]>,
          G: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        if self.blocks.contains_key(&idx) { self.touch(&idx); self.stats.hits += 1; return Ok(()); }
        self.stats.misses += 1;
        while self.blocks.len() >= self.capacity { if !self.evict_one(flusher)? { break; } }
        let raw = loader(idx)?;
        self.blocks.insert(idx, CachedBlock { data: Box::new(raw), is_dirty: false });
        self.lru.push_front(idx);
        Ok(())
    }
    fn with_block_mut<R>(&mut self, idx: BlockIdx, f: impl FnOnce(&mut [u8; BLOCK_SIZE]) -> R) -> R {
        let b = self.blocks.get_mut(&idx).expect("block not loaded"); b.is_dirty = true; f(&mut *b.data)
    }
    fn read_block<R>(&self, idx: BlockIdx, f: impl FnOnce(Option<&[u8; BLOCK_SIZE]>) -> R) -> R {
        f(self.blocks.get(&idx).map(|b| &*b.data as &[u8; BLOCK_SIZE]))
    }
    fn flush_dirty<F>(&mut self, flusher: &F) -> StorageResult<usize>
    where F: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        let dirty: Vec<BlockIdx> = self.blocks.iter().filter(|(_, b)| b.is_dirty).map(|(i, _)| *i).collect();
        for &idx in &dirty { if let Some(b) = self.blocks.get(&idx) { flusher(idx, &b.data)?; } }
        for &idx in &dirty { if let Some(b) = self.blocks.get_mut(&idx) { b.is_dirty = false; } }
        self.stats.dirty_flushes += dirty.len() as u64; Ok(dirty.len())
    }
    fn remove(&mut self, idx: BlockIdx) { self.blocks.remove(&idx); self.lru.retain(|&i| i != idx); }
    fn len(&self) -> usize { self.blocks.len() }
    fn touch(&mut self, idx: &BlockIdx) { self.lru.retain(|i| i != idx); self.lru.push_front(*idx); }
    fn evict_one<F>(&mut self, flusher: &F) -> StorageResult<bool>
    where F: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        let victim = self.lru.iter().rev().find_map(|idx| {
            let b = self.blocks.get(idx)?;
            if !b.is_dirty { Some(*idx) } else { None }
        }).or_else(|| self.lru.back().copied().filter(|idx| self.blocks.get(idx).map_or(false, |b| b.is_dirty)));
        match victim {
            Some(idx) => {
                if let Some(b) = self.blocks.get(&idx) { if b.is_dirty { flusher(idx, &b.data)?; self.stats.dirty_flushes += 1; } }
                self.blocks.remove(&idx); self.lru.retain(|&i| i != idx); self.stats.evictions += 1; Ok(true)
            }
            None => Ok(false),
        }
    }
}

/// Sharded block cache with independent RwLocks per shard.
pub struct ShardedBlockCache { shards: Vec<RwLock<BlockShard>>, shard_count: usize }

impl ShardedBlockCache {
    pub fn new(total_capacity: usize, shard_count: usize) -> Self {
        let n = shard_count.max(1); let per = (total_capacity + n - 1) / n;
        Self { shards: (0..n).map(|_| RwLock::new(BlockShard::new(per))).collect(), shard_count: n }
    }
    fn shard(&self, idx: BlockIdx) -> &RwLock<BlockShard> { &self.shards[(idx as usize) & (self.shard_count - 1)] }

    /// Read block data into a local buffer. Fetches from cache or loads from disk.
    pub fn read_block_data<F, G>(&self, idx: BlockIdx, loader: F, flusher: &G) -> StorageResult<[u8; BLOCK_SIZE]>
    where F: FnOnce(BlockIdx) -> StorageResult<[u8; BLOCK_SIZE]>,
          G: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        let s = self.shard(idx);
        let mut guard = s.write().map_err(|e| StorageError::Other(format!("cache lock: {e}")))?;
        guard.ensure_loaded(idx, loader, flusher)?;
        let mut buf = [0u8; BLOCK_SIZE];
        guard.read_block(idx, |opt| { if let Some(data) = opt { buf.copy_from_slice(data); } });
        Ok(buf)
    }

    /// Modify a block and mark it dirty. Loads from disk if not cached.
    pub fn with_block<F, G, R>(&self, idx: BlockIdx, loader: F, flusher: &G, f: impl FnOnce(&mut [u8; BLOCK_SIZE]) -> R) -> StorageResult<R>
    where F: FnOnce(BlockIdx) -> StorageResult<[u8; BLOCK_SIZE]>,
          G: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        let s = self.shard(idx);
        let mut guard = s.write().map_err(|e| StorageError::Other(format!("cache lock: {e}")))?;
        guard.ensure_loaded(idx, loader, flusher)?;
        Ok(guard.with_block_mut(idx, f))
    }

    /// Write block data back to cache. Loads from disk if not cached, marks dirty.
    pub fn write_block_data<F, G>(&self, idx: BlockIdx, data: &[u8; BLOCK_SIZE], loader: F, flusher: &G) -> StorageResult<()>
    where F: FnOnce(BlockIdx) -> StorageResult<[u8; BLOCK_SIZE]>,
          G: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        let s = self.shard(idx);
        let mut guard = s.write().map_err(|e| StorageError::Other(format!("cache lock: {e}")))?;
        guard.ensure_loaded(idx, loader, flusher)?;
        guard.with_block_mut(idx, |block| block.copy_from_slice(data));
        Ok(())
    }

    /// Read-only peek (returns data in a closure, no lock held after return).
    pub fn peek_block<R>(&self, idx: BlockIdx, f: impl FnOnce(Option<&[u8; BLOCK_SIZE]>) -> R) -> R {
        match self.shard(idx).read() {
            Ok(guard) => guard.read_block(idx, |opt| f(opt)),
            Err(_) => f(None),
        }
    }

    pub fn flush_dirty<F>(&self, flusher: &F) -> StorageResult<usize>
    where F: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        let mut total = 0;
        for s in &self.shards {
            let mut g = s.write().map_err(|e| StorageError::Other(format!("cache lock: {e}")))?;
            total += g.flush_dirty(flusher)?;
        }
        Ok(total)
    }

    pub fn remove(&self, idx: BlockIdx) { if let Ok(mut g) = self.shard(idx).write() { g.remove(idx); } }
    pub fn len(&self) -> usize { self.shards.iter().filter_map(|s| s.read().ok()).map(|g| g.len()).sum() }
    pub fn stats(&self) -> CacheStats {
        self.shards.iter().filter_map(|s| s.read().ok()).fold(CacheStats::default(), |a, g| a + g.stats.clone())
    }
}
