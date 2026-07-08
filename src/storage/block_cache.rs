//! LRU block cache with dirty-page tracking and writeback.
//!
//! Manages a fixed-capacity set of in-memory 16 KB blocks. On a cache miss,
//! the block is loaded from disk via a user-provided loader closure. When a
//! dirty block is evicted (or explicitly flushed), its contents are written
//! back via a user-provided flusher closure.

use std::{
    collections::{HashMap, VecDeque},
    time::Instant,
};

use crate::storage::types::{BlockIdx, BLOCK_SIZE, StorageResult};

/// Default cache capacity: 4096 blocks × 16 KB = 64 MB.
pub const DEFAULT_CACHE_CAPACITY: usize = 4096;

/// Statistics about cache performance.
#[derive(Clone, Debug, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub dirty_flushes: u64,
}

/// A single cached block.
pub struct CachedBlock {
    /// The raw 16 KB block data.
    pub data: Box<[u8; BLOCK_SIZE]>,
    /// Whether the block has been modified since last flush.
    pub is_dirty: bool,
    /// Timestamp of the most recent access (for LRU ordering).
    pub last_access: Instant,
}

/// Fixed-capacity LRU cache for data/index blocks.
pub struct BlockCache {
    blocks: HashMap<BlockIdx, CachedBlock>,
    /// LRU ordering: front = most recently used, back = least recently used.
    lru_order: VecDeque<BlockIdx>,
    capacity: usize,
    stats: CacheStats,
    /// Maximum age (in seconds) for a dirty block before it is auto-flushed.
    /// `None` disables time-based flush.
    max_dirty_age_secs: Option<u64>,
}

impl BlockCache {
    /// Create a new cache with the given capacity.
    ///
    /// `max_dirty_age_secs`: if `Some`, dirty blocks older than this are
    /// eligible for time-based flush during `get_or_load` and `flush_dirty`.
    pub fn new(capacity: usize, max_dirty_age_secs: Option<u64>) -> Self {
        Self {
            blocks: HashMap::with_capacity(capacity),
            lru_order: VecDeque::with_capacity(capacity),
            capacity,
            stats: CacheStats::default(),
            max_dirty_age_secs,
        }
    }

    /// Return the number of blocks currently cached.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Return `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Return a reference to the cache statistics.
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Get a block from cache, loading it from `loader` on miss.
    ///
    /// If the cache is full, the least-recently-used clean block is evicted.
    /// If no clean block exists (all dirty), the oldest dirty block is flushed
    /// via `flusher` and then evicted.
    ///
    /// # Important
    ///
    /// The returned reference is valid until the next mutating call on this
    /// cache (eviction, flush, or another `get_or_load`). We avoid returning
    /// references into the `HashMap` to satisfy the borrow checker — instead,
    /// this returns a raw pointer that gets reborrowed. The caller must not
    /// hold the reference across another cache mutation.
    pub fn get_or_load<F, G>(
        &mut self,
        idx: BlockIdx,
        loader: F,
        flusher: &G,
    ) -> StorageResult<&mut [u8; BLOCK_SIZE]>
    where
        F: FnOnce(BlockIdx) -> StorageResult<[u8; BLOCK_SIZE]>,
        G: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        // Fast path: already cached — touch and return via raw pointer.
        if self.blocks.contains_key(&idx) {
            self.touch(&idx);
            self.stats.hits += 1;
            // Safety: we hold &mut self, the entry is stable as long as we
            // don't remove it, and we just confirmed it exists.
            let ptr = self.blocks.get_mut(&idx).unwrap() as *mut CachedBlock;
            let data = unsafe { &mut (*ptr).data };
            return Ok(data);
        }

        // Miss — need to load and possibly evict.
        self.stats.misses += 1;

        // Evict if at capacity.
        while self.blocks.len() >= self.capacity {
            if !self.evict_one(flusher)? {
                break;
            }
        }

        // Load from disk.
        let raw_data = loader(idx)?;
        let boxed = Box::new(raw_data);

        let block = CachedBlock {
            data: boxed,
            is_dirty: false,
            last_access: Instant::now(),
        };

        self.blocks.insert(idx, block);
        self.lru_order.push_front(idx);

        // Return via raw pointer again.
        let ptr = self.blocks.get_mut(&idx).unwrap() as *mut CachedBlock;
        let data = unsafe { &mut (*ptr).data };
        Ok(data)
    }

    /// Mark a cached block as dirty (modified).
    ///
    /// Has no effect if the block is not in the cache.
    pub fn mark_dirty(&mut self, idx: BlockIdx) {
        if let Some(block) = self.blocks.get_mut(&idx) {
            block.is_dirty = true;
        }
    }

    /// Mark a set of blocks as dirty (batch variant).
    pub fn mark_dirty_batch(&mut self, indices: &[BlockIdx]) {
        for &idx in indices {
            self.mark_dirty(idx);
        }
    }

    /// Check whether a block is dirty.
    pub fn is_dirty(&self, idx: BlockIdx) -> bool {
        self.blocks.get(&idx).map_or(false, |b| b.is_dirty)
    }

    /// Flush all dirty blocks via the `flusher` callback.
    ///
    /// Returns the number of blocks flushed.
    pub fn flush_dirty<F>(&mut self, flusher: &F) -> StorageResult<usize>
    where
        F: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        let dirty_indices: Vec<BlockIdx> = self
            .blocks
            .iter()
            .filter(|(_, b)| b.is_dirty)
            .map(|(idx, _)| *idx)
            .collect();

        let count = dirty_indices.len();
        for idx in &dirty_indices {
            if let Some(block) = self.blocks.get(idx) {
                flusher(*idx, &block.data)?;
            }
        }
        // Mark clean after successful flush.
        for idx in &dirty_indices {
            if let Some(block) = self.blocks.get_mut(idx) {
                block.is_dirty = false;
            }
        }
        self.stats.dirty_flushes += count as u64;
        Ok(count)
    }

    /// Flush dirty blocks that exceed the maximum age threshold.
    ///
    /// Returns the number of blocks flushed.
    pub fn flush_aged_dirty<F>(&mut self, flusher: &F) -> StorageResult<usize>
    where
        F: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        let Some(max_age) = self.max_dirty_age_secs else {
            return Ok(0);
        };
        let cutoff = Instant::now() - std::time::Duration::from_secs(max_age);

        let aged: Vec<BlockIdx> = self
            .blocks
            .iter()
            .filter(|(_, b)| b.is_dirty && b.last_access < cutoff)
            .map(|(idx, _)| *idx)
            .collect();

        let count = aged.len();
        for idx in &aged {
            if let Some(block) = self.blocks.get(idx) {
                flusher(*idx, &block.data)?;
            }
        }
        for idx in &aged {
            if let Some(block) = self.blocks.get_mut(idx) {
                block.is_dirty = false;
            }
        }
        self.stats.dirty_flushes += count as u64;
        Ok(count)
    }

    /// Access a block without loading (returns `None` if not cached).
    /// This is useful for the index scanner which may check cache first.
    pub fn peek(&self, idx: BlockIdx) -> Option<&[u8; BLOCK_SIZE]> {
        self.blocks.get(&idx).map(|b| &*b.data)
    }

    /// Check if a block is in the cache.
    pub fn contains(&self, idx: BlockIdx) -> bool {
        self.blocks.contains_key(&idx)
    }

    /// Execute a closure with mutable access to a block, handling load/evict
    /// and automatically marking the block dirty after the closure returns.
    ///
    /// This is the safe alternative to `get_or_load` + `mark_dirty` — the
    /// closure receives `&mut [u8; BLOCK_SIZE]` and the dirty flag is set
    /// automatically, avoiding borrow-checker conflicts.
    pub fn with_block<F, G, R>(
        &mut self,
        idx: BlockIdx,
        loader: F,
        flusher: &G,
        f: impl FnOnce(&mut [u8; BLOCK_SIZE]) -> R,
    ) -> StorageResult<R>
    where
        F: FnOnce(BlockIdx) -> StorageResult<[u8; BLOCK_SIZE]>,
        G: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        let result = {
            let block = self.get_or_load(idx, loader, flusher)?;
            f(block)
            // block dropped here → borrow released
        };
        self.mark_dirty(idx);
        Ok(result)
    }

    /// Remove a block from the cache without flushing.
    pub fn remove(&mut self, idx: BlockIdx) {
        self.blocks.remove(&idx);
        self.lru_order.retain(|&i| i != idx);
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Move `idx` to the front of the LRU list.
    fn touch(&mut self, idx: &BlockIdx) {
        if let Some(pos) = self.lru_order.iter().position(|i| i == idx) {
            self.lru_order.remove(pos);
        }
        self.lru_order.push_front(*idx);
    }

    /// Evict the least-recently-used block. Returns `false` if nothing to evict.
    fn evict_one<F>(&mut self, flusher: &F) -> StorageResult<bool>
    where
        F: Fn(BlockIdx, &[u8; BLOCK_SIZE]) -> StorageResult<()>,
    {
        let to_evict = {
            let mut clean_candidate = None;
            let mut dirty_candidate = None;

            for idx in self.lru_order.iter().rev() {
                if let Some(block) = self.blocks.get(idx) {
                    if !block.is_dirty {
                        clean_candidate = Some(*idx);
                        break;
                    } else if dirty_candidate.is_none() {
                        dirty_candidate = Some(*idx);
                    }
                }
            }

            clean_candidate.or(dirty_candidate)
        };

        match to_evict {
            Some(idx) => {
                if let Some(block) = self.blocks.get(&idx) {
                    if block.is_dirty {
                        flusher(idx, &block.data)?;
                        self.stats.dirty_flushes += 1;
                    }
                }
                self.blocks.remove(&idx);
                self.lru_order.retain(|&i| i != idx);
                self.stats.evictions += 1;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_loader(idx: BlockIdx) -> StorageResult<[u8; BLOCK_SIZE]> {
        let mut buf = [0u8; BLOCK_SIZE];
        buf[0..4].copy_from_slice(&idx.to_le_bytes());
        Ok(buf)
    }

    fn noop_flusher(_idx: BlockIdx, _data: &[u8; BLOCK_SIZE]) -> StorageResult<()> {
        Ok(())
    }

    #[test]
    fn test_hit_and_miss() {
        let mut cache = BlockCache::new(10, None);
        let _ = cache
            .get_or_load(0, test_loader, &noop_flusher)
            .unwrap();
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().hits, 0);

        let _ = cache
            .get_or_load(0, test_loader, &noop_flusher)
            .unwrap();
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn test_dirty_marked_and_flushed() {
        let mut cache = BlockCache::new(10, None);
        let data = cache
            .get_or_load(5, test_loader, &noop_flusher)
            .unwrap();
        data[0] = 0xFF;
        cache.mark_dirty(5);
        assert!(cache.is_dirty(5));

        let flushed = cache.flush_dirty(&noop_flusher).unwrap();
        assert_eq!(flushed, 1);
        assert!(!cache.is_dirty(5));
    }

    #[test]
    fn test_eviction_evicts_lru() {
        let mut cache = BlockCache::new(2, None);
        cache
            .get_or_load(1, test_loader, &noop_flusher)
            .unwrap();
        cache
            .get_or_load(2, test_loader, &noop_flusher)
            .unwrap();
        // Cache is full. Access 1 to make it MRU.
        cache
            .get_or_load(1, test_loader, &noop_flusher)
            .unwrap();
        // Now access 3 — should evict 2 (LRU).
        cache
            .get_or_load(3, test_loader, &noop_flusher)
            .unwrap();
        assert!(!cache.contains(2));
        assert!(cache.contains(1));
        assert!(cache.contains(3));
    }
}
