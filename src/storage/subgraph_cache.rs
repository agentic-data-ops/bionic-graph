use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::index::SubgraphIndex;
use super::subgraph::{Subgraph, SubgraphId};
use crate::storage::SubgraphMeta;

/// Default maximum number of subgraphs to keep in memory.
pub const DEFAULT_CACHE_CAPACITY: usize = 1000;

// ─── Entry Tracking ──────────────────────────────────────────────

/// Wrapper for a cached subgraph with metadata.
struct CachedEntry {
    subgraph: Subgraph,
    dirty: bool,
    size_bytes: u64,
}

impl CachedEntry {
    fn new(subgraph: Subgraph) -> Self {
        let size_bytes = subgraph.estimated_size();
        Self {
            subgraph,
            dirty: false,
            size_bytes,
        }
    }
}

// ─── SubgraphCache ───────────────────────────────────────────────

/// An LRU cache of subgraphs with on-demand loading and dirty write-back.
///
/// - `get()`: returns a subgraph — loads from disk on miss
/// - `get_mut()`: same, but marks the subgraph dirty
/// - On eviction: dirty subgraphs are serialized and written to disk first
///
/// Thread safety: intended to be used behind `Arc<Mutex<SubgraphCache>>`.
pub struct SubgraphCache {
    /// Cached entries by subgraph ID.
    entries: HashMap<SubgraphId, CachedEntry>,
    /// LRU order — front = most recently used.
    order: VecDeque<SubgraphId>,
    /// Maximum number of subgraphs to cache.
    capacity: usize,
    /// Base directory for subgraph files.
    data_dir: PathBuf,
    /// Global stats.
    stats: CacheStats,
}

/// Cache statistics.
#[derive(Debug, Default)]
pub struct CacheStats {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub evictions: AtomicU64,
    pub writes: AtomicU64,
}

impl SubgraphCache {
    /// Create a new cache backed by the given data directory.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            entries: HashMap::with_capacity(DEFAULT_CACHE_CAPACITY),
            order: VecDeque::with_capacity(DEFAULT_CACHE_CAPACITY),
            capacity: DEFAULT_CACHE_CAPACITY,
            data_dir: data_dir.into(),
            stats: CacheStats::default(),
        }
    }

    /// Set a custom capacity.
    pub fn with_capacity(mut self, cap: usize) -> Self {
        self.capacity = cap;
        self.order = VecDeque::with_capacity(cap);
        self.entries = HashMap::with_capacity(cap);
        self
    }

    // ─── Public API ─────────────────────────────────────────────

    /// Get a subgraph (immutable). Loads from disk on cache miss.
    ///
    /// Returns `None` if the subgraph doesn't exist on disk.
    pub fn get(
        &mut self,
        id: SubgraphId,
        subgraph_index: &SubgraphIndex,
    ) -> Option<&Subgraph> {
        let was_cached = self.entries.contains_key(&id);
        self.load_if_missing(id, subgraph_index);
        if self.entries.contains_key(&id) {
            self.promote(id);
            if was_cached {
                self.stats.hits.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.entries.get(&id).map(|e| &e.subgraph)
    }

    /// Get a subgraph (mutable). Marks it dirty so it'll be written back on eviction.
    pub fn get_mut(
        &mut self,
        id: SubgraphId,
        subgraph_index: &SubgraphIndex,
    ) -> Option<&mut Subgraph> {
        let was_cached = self.entries.contains_key(&id);
        self.load_if_missing(id, subgraph_index);
        if self.entries.contains_key(&id) {
            self.promote(id);
            if was_cached {
                self.stats.hits.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.entries.get_mut(&id).map(|e| {
            e.dirty = true;
            &mut e.subgraph
        })
    }

    /// Insert a subgraph directly into the cache (for newly created subgraphs).
    /// Marks it dirty so it'll get written to disk.
    pub fn insert(&mut self, subgraph: Subgraph) {
        let id = subgraph.id;
        if !self.entries.contains_key(&id) {
            self.evict_if_needed();
        }
        self.entries.insert(id, CachedEntry::new(subgraph));
        self.promote(id);
    }

    /// Check if a subgraph is currently in cache.
    pub fn contains(&self, id: SubgraphId) -> bool {
        self.entries.contains_key(&id)
    }

    /// Check if a subgraph is dirty.
    pub fn is_dirty(&self, id: SubgraphId) -> bool {
        self.entries.get(&id).map(|e| e.dirty).unwrap_or(false)
    }

    /// Flush a single dirty subgraph to disk.
    /// Returns `true` if a write actually happened.
    pub fn flush(&mut self, id: SubgraphId) -> bool {
        if let Some(entry) = self.entries.get_mut(&id) {
            if entry.dirty {
                let path = subgraph_path(&self.data_dir, id);
                if let Err(e) = std::fs::write(&path, entry.subgraph.to_bytes()) {
                    log::error!("Failed to write subgraph {}: {}", id, e);
                    return false;
                }
                entry.dirty = false;
                self.stats.writes.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Flush all dirty subgraphs to disk.
    pub fn flush_all(&mut self) -> usize {
        let ids: Vec<SubgraphId> = self.entries.iter()
            .filter(|(_, e)| e.dirty)
            .map(|(&id, _)| id)
            .collect();

        let mut count = 0;
        for id in ids {
            if self.flush(id) {
                count += 1;
            }
        }
        count
    }

    /// Evict a specific subgraph (write back if dirty, then remove from cache).
    /// Returns `true` if eviction happened.
    pub fn evict(&mut self, id: SubgraphId) -> bool {
        if let Some(mut entry) = self.entries.remove(&id) {
            if entry.dirty {
                let path = subgraph_path(&self.data_dir, id);
                if let Err(e) = std::fs::write(&path, entry.subgraph.to_bytes()) {
                    log::error!("Failed to write subgraph {} on eviction: {}", id, e);
                }
                self.stats.writes.fetch_add(1, Ordering::Relaxed);
            }
            self.order.retain(|&x| x != id);
            self.stats.evictions.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Remove a subgraph from cache without writing back (for deletion).
    pub fn discard(&mut self, id: SubgraphId) -> bool {
        if self.entries.remove(&id).is_some() {
            self.order.retain(|&x| x != id);
            true
        } else {
            false
        }
    }

    /// Number of subgraphs currently in cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of dirty subgraphs.
    pub fn dirty_count(&self) -> usize {
        self.entries.values().filter(|e| e.dirty).count()
    }

    /// Reference to cache stats.
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// IDs of all subgraphs currently in cache.
    pub fn cached_ids(&self) -> Vec<SubgraphId> {
        self.entries.keys().copied().collect()
    }

    // ─── Internal ───────────────────────────────────────────────

    /// Load a subgraph from disk if not already in cache.
    fn load_if_missing(&mut self, id: SubgraphId, index: &SubgraphIndex) {
        if self.entries.contains_key(&id) {
            return;
        }
        self.evict_if_needed();

        // Find the file path from the index
        let file_path = match index.get(id) {
            Some(meta) => meta.file_path.clone(),
            None => subgraph_path(&self.data_dir, id),
        };

        match std::fs::read(&file_path) {
            Ok(data) => {
                match Subgraph::from_bytes(&data) {
                    Some((subgraph, _version)) => {
                        self.entries.insert(id, CachedEntry::new(subgraph));
                        self.stats.misses.fetch_add(1, Ordering::Relaxed);
                    }
                    None => {
                        log::error!("Corrupted subgraph file: {:?}", file_path);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Subgraph doesn't exist yet — not an error
                log::trace!("Subgraph {} not found on disk", id);
            }
            Err(e) => {
                log::error!("Failed to read subgraph {}: {}", id, e);
            }
        }
    }

    /// Evict the least recently used *clean* subgraph if at capacity.
    /// If no clean subgraph exists, evict the LRU dirty one (write-back).
    fn evict_if_needed(&mut self) {
        while self.entries.len() >= self.capacity {
            // Find the LRU entry from the back of the order
            let lru_id = match self.order.back().copied() {
                None => break,
                Some(id) => id,
            };

            // If it's dirty, write it back first
            if self.entries.get(&lru_id).map(|e| e.dirty).unwrap_or(false) {
                self.flush(lru_id);
            }

            self.entries.remove(&lru_id);
            self.order.retain(|&x| x != lru_id);
            self.stats.evictions.fetch_add(1, Ordering::Relaxed);

            log::trace!("Evicted subgraph {} from cache", lru_id);
        }
    }

    /// Move a subgraph ID to the front of the LRU order (most recently used).
    fn promote(&mut self, id: SubgraphId) {
        // Remove from current position (O(n) but n ≤ capacity)
        if let Some(pos) = self.order.iter().position(|&x| x == id) {
            self.order.remove(pos);
        }
        // Push to front
        self.order.push_front(id);
    }
}

/// Build the expected file path for a subgraph on disk.
fn subgraph_path(data_dir: &Path, id: SubgraphId) -> PathBuf {
    // Format: data/subgraph/00000042.bin
    let filename = format!("{:08x}.bin", id);
    data_dir.join("subgraph").join(filename)
}

// ─── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::index::{SubgraphIndex, SubgraphMeta};
    use crate::storage::subgraph::Subgraph;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn make_cache() -> (SubgraphCache, SubgraphIndex, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let cache = SubgraphCache::new(dir.path()).with_capacity(3);
        (cache, SubgraphIndex::new(), dir)
    }

    fn seed_subgraph_on_disk(dir: &Path, id: SubgraphId, index: &mut SubgraphIndex) -> Subgraph {
        let mut sg = Subgraph::new(id);
        sg.add_vertex(vec!["test".to_string()]);
        let bytes = sg.to_bytes();
        let path = subgraph_path(dir, id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &bytes).unwrap();

        let meta = SubgraphMeta {
            id,
            file_path: path,
            vertex_count: sg.vertices.len() as u32,
            edge_count: sg.edges.len() as u32,
            cross_edge_count: sg.cross_edges.len() as u32,
            size_bytes: bytes.len() as u64,
            checksum: 0,
        };
        index.insert(meta);
        sg
    }

    #[test]
    fn test_cache_miss_loads_from_disk() {
        let (mut cache, mut index, dir) = make_cache();
        seed_subgraph_on_disk(dir.path(), 1, &mut index);

        let sg = cache.get(1, &index);
        assert!(sg.is_some(), "Should load subgraph 1 from disk");
        assert_eq!(sg.unwrap().id, 1);
        assert_eq!(cache.stats.misses.load(Ordering::Relaxed), 1);
        assert_eq!(cache.stats.hits.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_cache_hit_after_load() {
        let (mut cache, mut index, dir) = make_cache();
        seed_subgraph_on_disk(dir.path(), 1, &mut index);

        // First access: miss
        cache.get(1, &index);
        // Second access: hit
        let sg = cache.get(1, &index);
        assert!(sg.is_some());
        assert_eq!(cache.stats.misses.load(Ordering::Relaxed), 1);
        assert_eq!(cache.stats.hits.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_missing_subgraph_returns_none() {
        let (mut cache, index, _dir) = make_cache();
        let sg = cache.get(99, &index);
        assert!(sg.is_none());
    }

    #[test]
    fn test_get_mut_marks_dirty() {
        let (mut cache, mut index, dir) = make_cache();
        seed_subgraph_on_disk(dir.path(), 1, &mut index);

        {
            let sg = cache.get_mut(1, &index).unwrap();
            sg.add_vertex(vec!["new".to_string()]);
        }
        assert!(cache.is_dirty(1));
    }

    #[test]
    fn test_insert_and_retrieve() {
        let (mut cache, index, _dir) = make_cache();
        let mut sg = Subgraph::new(42);
        sg.add_vertex(vec!["new".to_string()]);
        cache.insert(sg);

        let retrieved = cache.get(42, &index);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().vertices.len(), 1);
    }

    #[test]
    fn test_lru_eviction() {
        let (mut cache, mut index, dir) = make_cache();
        // Capacity is 3
        for id in 1..=3 {
            seed_subgraph_on_disk(dir.path(), id, &mut index);
            cache.get(id, &index); // Load into cache
        }

        assert_eq!(cache.len(), 3);

        // Access id=1 to make it most recently used
        cache.get(1, &index);

        // Load id=4 — should evict the LRU (which is now id=2, never accessed again)
        seed_subgraph_on_disk(dir.path(), 4, &mut index);
        cache.get(4, &index);

        assert_eq!(cache.len(), 3, "Should stay at capacity 3");
        // id=2 should have been evicted
        assert!(!cache.contains(2), "LRU id=2 should have been evicted");
        assert!(cache.contains(1), "id=1 should still be in cache");
        assert!(cache.contains(3), "id=3 should still be in cache");
        assert!(cache.contains(4), "id=4 should be in cache");
    }

    #[test]
    fn test_flush_dirty() {
        let (mut cache, mut index, dir) = make_cache();
        seed_subgraph_on_disk(dir.path(), 1, &mut index);

        // Load, modify, flush
        cache.get_mut(1, &index).unwrap();
        assert!(cache.is_dirty(1));
        assert!(cache.flush(1));
        assert!(!cache.is_dirty(1));

        // Verify file exists on disk
        let path = subgraph_path(dir.path(), 1);
        assert!(path.exists(), "Flushed subgraph should exist on disk");
    }

    #[test]
    fn test_flush_all() {
        let (mut cache, mut index, dir) = make_cache();
        for id in 1..=3 {
            seed_subgraph_on_disk(dir.path(), id, &mut index);
            cache.get_mut(id, &index).unwrap();
        }
        assert_eq!(cache.dirty_count(), 3);
        assert_eq!(cache.flush_all(), 3);
        assert_eq!(cache.dirty_count(), 0);
    }

    #[test]
    fn test_eviction_writes_dirty_back() {
        let (mut cache, mut index, dir) = make_cache();
        for id in 1..=3 {
            seed_subgraph_on_disk(dir.path(), id, &mut index);
            cache.get(id, &index);
        }

        // Make id=3 dirty
        cache.get_mut(3, &index).unwrap();
        // Evict id=3
        assert!(cache.evict(3));
        assert!(!cache.contains(3));

        // Should have been written to disk
        let path = subgraph_path(dir.path(), 3);
        assert!(path.exists(), "Dirty evicted subgraph should be on disk");
    }

    #[test]
    fn test_discard_without_write() {
        let (mut cache, mut index, dir) = make_cache();
        seed_subgraph_on_disk(dir.path(), 1, &mut index);
        cache.get_mut(1, &index).unwrap();
        assert!(cache.discard(1));
        assert!(!cache.contains(1));
        // File should NOT have been updated (discard skips write-back)
    }

    #[test]
    fn test_lru_order_on_access() {
        let (mut cache, mut index, dir) = make_cache();
        for id in 1..=3 {
            seed_subgraph_on_disk(dir.path(), id, &mut index);
        }

        cache.get(1, &index);
        cache.get(2, &index);
        cache.get(3, &index);

        // Now promote 1 again
        cache.get(1, &index);

        // Load id=4 — should evict LRU (2, since order is now 1, 3, 2)
        seed_subgraph_on_disk(dir.path(), 4, &mut index);
        cache.get(4, &index);

        assert_eq!(cache.len(), 3);
        assert!(!cache.contains(2), "2 should have been evicted as LRU");
        assert!(cache.contains(1), "1 was most recently used");
        assert!(cache.contains(3), "3 was second most recently used");
        assert!(cache.contains(4), "4 should be in cache");
    }
}
