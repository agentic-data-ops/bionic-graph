//! In-memory index structures for fast lookups during query execution.
//!
//! These structures are rebuilt from the data file at graph startup and
//! updated in lockstep with mutations.
//!
//! # Structures
//!
//! | Type | Key | Value | Purpose |
//! |------|-----|-------|---------|
//! | `VertexBTree` | `VertexId` | `MetaPointer` | O(log n) vertex lookup → data file location |
//! | `EdgeBTree` | `EdgeId` | `MetaPointer` | O(log n) edge lookup → data file location |
//! | `TokenMap` | token string | `Vec<MetaPointer>` | Full-text search |
//! | `RankIndex` | rank | `Vec<MetaPointer>` | Rank-ordered retrieval |

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::storage::types::{BlockIdx, ChunkOffset, StorageError, StorageResult};

/// Points to the DataHeader of a vertex/edge/token record in the data file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MetaPointer {
    pub block_idx: BlockIdx,
    pub chunk_offset: ChunkOffset,
}

impl MetaPointer {
    pub fn new(block_idx: BlockIdx, chunk_offset: ChunkOffset) -> Self {
        Self {
            block_idx,
            chunk_offset,
        }
    }
}

// ── Vertex index ─────────────────────────────────────────────────────────────

/// B-tree mapping `VertexId` → `MetaPointer`.
///
/// Backed by `BTreeMap<u32, MetaPointer>` for O(log n) lookups and
/// efficient range scans.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VertexBTree {
    inner: BTreeMap<u32, MetaPointer>,
}

impl VertexBTree {
    pub fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    /// Insert or update a mapping.
    pub fn insert(&mut self, vertex_id: u32, ptr: MetaPointer) {
        self.inner.insert(vertex_id, ptr);
    }

    /// Look up a vertex by ID.
    pub fn get(&self, vertex_id: u32) -> Option<&MetaPointer> {
        self.inner.get(&vertex_id)
    }

    /// Remove a vertex mapping.
    pub fn remove(&mut self, vertex_id: u32) -> Option<MetaPointer> {
        self.inner.remove(&vertex_id)
    }

    /// Return `true` if the vertex exists.
    pub fn contains(&self, vertex_id: u32) -> bool {
        self.inner.contains_key(&vertex_id)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = (&u32, &MetaPointer)> {
        self.inner.iter()
    }

    /// Iterate over vertex IDs in ascending order.
    pub fn keys(&self) -> impl Iterator<Item = &u32> {
        self.inner.keys()
    }
}

// ── Edge index ───────────────────────────────────────────────────────────────

/// B-tree mapping `EdgeId` → `MetaPointer`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EdgeBTree {
    inner: BTreeMap<u32, MetaPointer>,
}

impl EdgeBTree {
    pub fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, edge_id: u32, ptr: MetaPointer) {
        self.inner.insert(edge_id, ptr);
    }

    pub fn get(&self, edge_id: u32) -> Option<&MetaPointer> {
        self.inner.get(&edge_id)
    }

    pub fn remove(&mut self, edge_id: u32) -> Option<MetaPointer> {
        self.inner.remove(&edge_id)
    }

    pub fn contains(&self, edge_id: u32) -> bool {
        self.inner.contains_key(&edge_id)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u32, &MetaPointer)> {
        self.inner.iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &u32> {
        self.inner.keys()
    }
}

// ── Token map (BTreeMap, O(log N) lookup + prefix search) ─────────────────

/// Token map backed by `BTreeMap` for prefix search support.
///
/// - exact match: `BTreeMap::get()` — O(log N)
/// - prefix match: `BTreeMap::range()` — O(log N + M) where M = result count
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TokenMap {
    inner: BTreeMap<String, Vec<MetaPointer>>,
}

impl TokenMap {
    pub fn new() -> Self {
        Self { inner: BTreeMap::new() }
    }

    /// Add a token → pointer mapping.
    pub fn insert(&mut self, token: String, ptr: MetaPointer) {
        self.inner.entry(token).or_default().push(ptr);
    }

    /// Exact match lookup (O(log N)).
    pub fn get(&self, token: &str) -> Option<&Vec<MetaPointer>> {
        self.inner.get(token)
    }

    /// Prefix search via BTreeMap range scan (O(log N + M)).
    /// Iterates from the first key ≥ `prefix`, stopping when key no longer starts with prefix.
    pub fn search_prefix(&self, prefix: &str) -> Vec<(String, Vec<MetaPointer>)> {
        let mut results = Vec::new();
        for (stored, ptrs) in self.inner.range(prefix.to_string()..) {
            if stored.starts_with(prefix) {
                results.push((stored.clone(), ptrs.clone()));
            } else {
                break;
            }
        }
        results
    }

    /// Check if a token exists.
    pub fn contains(&self, token: &str) -> bool {
        self.inner.contains_key(token)
    }

    /// Number of unique tokens.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterate over all (token, pointers) entries.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<MetaPointer>)> {
        self.inner.iter()
    }

    /// Remove a specific pointer for a token.
    pub fn remove_pointer(&mut self, token: &str, ptr: &MetaPointer) {
        if let Some(ptrs) = self.inner.get_mut(token) {
            ptrs.retain(|p| p != ptr);
            if ptrs.is_empty() {
                self.inner.remove(token);
            }
        }
    }

    /// Remove all pointers for a token.
    pub fn remove_token(&mut self, token: &str) {
        self.inner.remove(token);
    }
}

// ── Rank index ───────────────────────────────────────────────────────────────

/// B-tree mapping rank → list of index pointers.
///
/// Rank auto-increments on access/update and auto-decrements over time.
/// This index enables "most popular" / "most relevant" queries.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RankIndex {
    /// Maps rank value → set of index pointers at that rank.
    inner: BTreeMap<u32, Vec<MetaPointer>>,
}

impl RankIndex {
    pub fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    /// Add a pointer at a given rank.
    pub fn insert(&mut self, rank: u32, ptr: MetaPointer) {
        self.inner.entry(rank).or_default().push(ptr);
    }

    /// Remove a pointer from the rank index.
    pub fn remove(&mut self, rank: u32, ptr: &MetaPointer) {
        if let Some(ptrs) = self.inner.get_mut(&rank) {
            ptrs.retain(|p| p != ptr);
            if ptrs.is_empty() {
                self.inner.remove(&rank);
            }
        }
    }

    /// Get all pointers at or above a minimum rank (descending order).
    pub fn get_above(&self, min_rank: u32) -> Vec<&MetaPointer> {
        let mut result = Vec::new();
        for (_rank, ptrs) in self.inner.range(min_rank..).rev() {
            result.extend(ptrs);
        }
        result
    }

    /// Get all pointers sorted by rank descending.
    pub fn all_by_rank(&self) -> Vec<&MetaPointer> {
        let mut result = Vec::new();
        for (_rank, ptrs) in self.inner.iter().rev() {
            result.extend(ptrs);
        }
        result
    }

    /// Get up to `limit` pointers from the highest ranks.
    pub fn top_pointers(&self, limit: usize, min_rank: Option<u32>) -> Vec<MetaPointer> {
        if limit == 0 {
            return vec![];
        }
        let mut result = Vec::with_capacity(limit.min(128));
        for (_rank, ptrs) in self.inner.iter().rev() {
            for ptr in ptrs {
                if result.len() >= limit {
                    return result;
                }
                if min_rank.map_or(true, |mr| *_rank >= mr) {
                    result.push(*ptr);
                }
            }
        }
        result
    }

    /// Number of distinct rank values.
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

// ── Atime index ──────────────────────────────────────────────────────────────

/// B-tree mapping atime (microsecond timestamp) → list of index pointers.
///
/// Enables efficient range scans for inactive entity detection:
/// `range(..threshold)` finds all entities not accessed since `threshold`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AtimeIndex {
    inner: BTreeMap<u64, Vec<MetaPointer>>,
}

impl AtimeIndex {
    pub fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    /// Insert a pointer at the given atime.
    pub fn insert(&mut self, atime: u64, ptr: MetaPointer) {
        self.inner.entry(atime).or_default().push(ptr);
    }

    /// Remove a pointer from the atime index.
    pub fn remove(&mut self, atime: u64, ptr: &MetaPointer) {
        if let Some(ptrs) = self.inner.get_mut(&atime) {
            ptrs.retain(|p| p != ptr);
            if ptrs.is_empty() {
                self.inner.remove(&atime);
            }
        }
    }

    /// Get all pointers with atime ≤ threshold (oldest first).
    pub fn range_up_to(&self, threshold: u64) -> Vec<(u64, MetaPointer)> {
        let mut result = Vec::new();
        for (&atime, ptrs) in self.inner.range(..=threshold) {
            for &ptr in ptrs {
                result.push((atime, ptr));
            }
        }
        result
    }

    /// Number of distinct atime values.
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

// ── Edge adjacency index (for traversal) ────────────────────────────────────

/// Maps a vertex ID to its outgoing and incoming edges.
///
/// This is built at startup by scanning edge records and registering
/// each edge's source → target and target → source.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AdjacencyIndex {
    /// forward[v] = list of (edge_id, target_vertex_id, edge_ptr) for outgoing edges.
    forward: HashMap<u32, Vec<(u32, u32, MetaPointer)>>,
    /// backward[v] = list of (edge_id, source_vertex_id, edge_ptr) for incoming edges.
    backward: HashMap<u32, Vec<(u32, u32, MetaPointer)>>,
}

impl AdjacencyIndex {
    pub fn new() -> Self {
        Self {
            forward: HashMap::new(),
            backward: HashMap::new(),
        }
    }

    /// Register an edge in the adjacency index.
    pub fn add_edge(&mut self, edge_id: u32, source: u32, target: u32, edge_ptr: MetaPointer) {
        self.forward
            .entry(source)
            .or_default()
            .push((edge_id, target, edge_ptr));
        self.backward
            .entry(target)
            .or_default()
            .push((edge_id, source, edge_ptr));
    }

    /// Remove an edge.
    pub fn remove_edge(&mut self, source: u32, target: u32, edge_ptr: &MetaPointer) {
        if let Some(edges) = self.forward.get_mut(&source) {
            edges.retain(|(_, t, p)| t != &target || p != edge_ptr);
        }
        if let Some(edges) = self.backward.get_mut(&target) {
            edges.retain(|(_, s, p)| s != &source || p != edge_ptr);
        }
    }

    /// Get outgoing edges from a vertex: (edge_id, target_vertex_id, edge_ptr).
    pub fn out_edges(&self, vertex_id: u32) -> &[(u32, u32, MetaPointer)] {
        self.forward.get(&vertex_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get incoming edges to a vertex: (edge_id, source_vertex_id, edge_ptr).
    pub fn in_edges(&self, vertex_id: u32) -> &[(u32, u32, MetaPointer)] {
        self.backward.get(&vertex_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// All vertices that have at least one edge.
    pub fn all_vertices(&self) -> Vec<u32> {
        let mut keys: Vec<u32> = self
            .forward
            .keys()
            .chain(self.backward.keys())
            .copied()
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }
}

// ── Composite in-memory index ────────────────────────────────────────────────

/// All in-memory index structures for a single graph.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemoryIndex {
    pub vertex_id: VertexBTree,
    pub edge_id: EdgeBTree,
    pub token: TokenMap,
    pub rank: RankIndex,
    pub atime: AtimeIndex,
    pub vertex_adjacency: AdjacencyIndex,
    /// Reverse index: (ref_type, ref_id) → list of token strings referencing that entity.
    /// ref_type: 0=vertex, 1=edge. Built at startup from token scan and maintained
    /// incrementally by add_token() / remove_entity_token_refs().
    pub token_ref: HashMap<(u8, u32), Vec<String>>,
    /// Name → vertex ID lookup (built at startup).
    pub vertex_name: BTreeMap<String, u32>,
    /// (source_name, target_name, edge_name) → edge ID lookup (built at startup).
    pub edge_name: BTreeMap<String, u32>,

}

impl MemoryIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that a token references an entity.
    pub fn add_entity_token(&mut self, ref_type: u8, ref_id: u32, token_str: &str) {
        self.token_ref
            .entry((ref_type, ref_id))
            .or_default()
            .push(token_str.to_string());
    }

    /// Get all token strings referencing an entity.
    pub fn get_entity_tokens(&self, ref_type: u8, ref_id: u32) -> Vec<String> {
        self.token_ref
            .get(&(ref_type, ref_id))
            .cloned()
            .unwrap_or_default()
    }

    /// Remove all references from an entity (used during hard delete).
    /// Returns the list of token strings that were referencing it.
    pub fn remove_entity_token_refs(&mut self, ref_type: u8, ref_id: u32) -> Vec<String> {
        self.token_ref.remove(&(ref_type, ref_id)).unwrap_or_default()
    }

    /// Look up a vertex ID by name.
    pub fn get_vertex_id_by_name(&self, name: &str) -> Option<u32> {
        self.vertex_name.get(name).copied()
    }

    /// Look up an edge ID by name.
    pub fn get_edge_id_by_name(&self, name: &str) -> Option<u32> {
        self.edge_name.get(name).copied()
    }

    // ── Index persistence ─────────────────────────────────────────────────

    /// Save all indexes to `dir`. Writes `index_state` last as a commit marker.
    pub fn save_to_dir(&self, dir: &Path) -> StorageResult<()> {
        fs::create_dir_all(dir)?;
        save_one(&dir.join("index_vertex_id"), &self.vertex_id)?;
        save_one(&dir.join("index_edge_id"), &self.edge_id)?;
        save_one(&dir.join("index_token"), &self.token)?;
        save_one(&dir.join("index_rank"), &self.rank)?;
        save_one(&dir.join("index_atime"), &self.atime)?;
        save_one(&dir.join("index_vertex_adjacency"), &self.vertex_adjacency)?;
        save_one(&dir.join("index_token_ref"), &self.token_ref)?;
        save_one(&dir.join("index_vertex_name"), &self.vertex_name)?;
        save_one(&dir.join("index_edge_name"), &self.edge_name)?;
        fs::write(dir.join("index_state"), b"1")?;
        Ok(())
    }

    /// Load indexes from `dir`. Returns `None` if the marker file is missing.
    pub fn load_from_dir(dir: &Path) -> StorageResult<Option<MemoryIndex>> {
        let marker = dir.join("index_state");
        if !marker.exists() {
            return Ok(None);
        }
        let mi = MemoryIndex {
            vertex_id: load_one(&dir.join("index_vertex_id"))?,
            edge_id: load_one(&dir.join("index_edge_id"))?,
            token: load_one(&dir.join("index_token"))?,
            rank: load_one(&dir.join("index_rank"))?,
            atime: load_one(&dir.join("index_atime"))?,
            vertex_adjacency: load_one(&dir.join("index_vertex_adjacency"))?,
            token_ref: load_one(&dir.join("index_token_ref"))?,
            vertex_name: load_one(&dir.join("index_vertex_name"))?,
            edge_name: load_one(&dir.join("index_edge_name"))?,
        };
        let _ = fs::remove_file(&marker);
        Ok(Some(mi))
    }

    /// Remove all index files from `dir`.
    pub fn remove_index_files(dir: &Path) {
        for name in &["index_vertex_id", "index_edge_id", "index_token", "index_rank",
                      "index_atime", "index_vertex_adjacency", "index_token_ref",
                      "index_vertex_name", "index_edge_name", "index_state"]
        {
            let _ = fs::remove_file(dir.join(name));
        }
    }
}

// ── Serialization helpers ─────────────────────────────────────────────────

fn save_one<T: serde::Serialize>(path: &Path, data: &T) -> StorageResult<()> {
    let bytes = bincode::serialize(data)
        .map_err(|e| StorageError::Other(format!("index serialize {}: {}", path.display(), e)))?;
    fs::write(path, &bytes)
        .map_err(|e| StorageError::Other(format!("index write {}: {}", path.display(), e)))?;
    Ok(())
}

fn load_one<T: serde::de::DeserializeOwned>(path: &Path) -> StorageResult<T> {
    let bytes = fs::read(path)
        .map_err(|e| StorageError::Other(format!("index read {}: {}", path.display(), e)))?;
    bincode::deserialize(&bytes)
        .map_err(|e| StorageError::Other(format!("index deserialize {}: {}", path.display(), e)))
}
