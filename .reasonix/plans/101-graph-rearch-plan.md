# Graph Re-architecture Coding Plan

**Goal:** Replace the entire backend graph storage + query engine with the new block-based storage + token-index design, while keeping the frontend and adapting its API calls.

**Scope:** ~15,500 lines of Rust to replace, ~6 React files to adapt.

**Branch:** `dev-snap-neuron-search-mem`

---

## Overview

```
┌─────────────────────────────────────────────────────────┐
│ Phase 1: Block Storage Engine                           │
│   data file, bitmap file, block cache, redo log         │
├─────────────────────────────────────────────────────────┤
│ Phase 2: Index Engine                                   │
│   index file, B+ tree (vertex/edge/rank), FST (token)   │
├─────────────────────────────────────────────────────────┤
│ Phase 3: Query Engine                                   │
│   Graph facade, CRUD, token extraction, gremlin steps   │
├─────────────────────────────────────────────────────────┤
│ Phase 4: Locking & Concurrency                          │
│   RwLock per vertex/edge/block, deadlock-free ordering  │
├─────────────────────────────────────────────────────────┤
│ Phase 5: REST API Layer                                 │
│   axum routes matching old API surface                  │
├─────────────────────────────────────────────────────────┤
│ Phase 6: Frontend Adaptation                            │
│   API.js + GraphViewer response shape changes           │
├─────────────────────────────────────────────────────────┤
│ Phase 7: Cluster (future / optional)                    │
│   Master-worker replication via redo log replay         │
├─────────────────────────────────────────────────────────┤
│ Phase 8: Cleanup & Migration                            │
│   Remove old code, data migration tool, tests           │
└─────────────────────────────────────────────────────────┘
```

---

## Phase 1: Block Storage Engine

### 1.1 Data types & constants

**File:** `src/storage2/types.rs`

Define new fundamental types shared across all storage layers:

```rust
// Block size = 16KB, chunk size = 64B
pub const BLOCK_SIZE: usize = 16384;       // 16 KB
pub const CHUNK_SIZE: usize = 64;          // 64 B
pub const CHUNKS_PER_BLOCK: usize = 256;   // 16384 / 64
pub const BLOCK_HEADER_SIZE: usize = 64;   // first chunk = header

// IDs
pub type VertexId = u32;
pub type EdgeId = u32;
pub type TokenId = u32;
pub type BlockIdx = u32;
pub type Version = u16;

// Timestamps in microseconds
pub type TimestampUs = u64;

// Chunk offset within a block (1-indexed, 0 = invalid)
pub type ChunkOffset = u8;
```

**Status enum:**
```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChunkType { Empty = 0, Vertex = 1, Edge = 2, Token = 3 }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DataStatus { Normal = 0, Deleted = 1 }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlockStatus { Normal = 0, Dirty = 1 }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OpType {
    VertexCreate = 0x00, VertexDelete = 0x01, VertexUpdate = 0x02, VertexIndexUpdate = 0x03,
    EdgeCreate   = 0x04, EdgeDelete   = 0x05, EdgeUpdate   = 0x06, EdgeIndexUpdate   = 0x07,
    TokenCreate  = 0x08, TokenUpdate  = 0x09, TokenDelete  = 0x0A, TokenIndexUpdate  = 0x0B,
}
```

**Data structures (binary layouts):**
```rust
pub struct BlockHeader {
    pub offset: u8,           // alloc offset (1..256)
    pub bitmap: [u8; 32],     // 256 bits, bit 0 reserved for header
    pub status: BlockStatus,
    pub timestamp: u64,
    pub padding: [u8; 22],
}

pub struct VertexPayload {
    pub id: u32,
    pub name: String,
    pub labels: Vec<String>,
    pub keywords: Vec<String>,
    pub properties: HashMap<String, PropertyValue>,
    pub history: Vec<HistoryRecord>,
}

pub struct EdgePayload {
    pub id: u32,
    pub label: String,
    pub keywords: Vec<String>,
    pub strength: f32,
    pub properties: HashMap<String, PropertyValue>,
    pub source: u32,
    pub target: u32,
    pub history: Vec<HistoryRecord>,
}

pub struct TokenPayload {
    pub id: u32,
    pub refs: Vec<TokenRef>,
}

pub struct TokenRef {
    pub ref_type: u8,       // 0=vertex, 1=edge
    pub ref_id: u32,
    pub ref_version: u16,
    pub ref_frequency: u16,
    pub hits: Vec<Hit>,
}

pub struct Hit {
    pub hit_key: String,
    pub hit_offset: u16,
}

pub struct HistoryRecord {
    pub timestamp: u64,
    pub data: Vec<u8>,      // raw serialized VertexPayload/EdgePayload
}
```

### 1.2 Data file

**File:** `src/storage2/data_file.rs`

Responsible for reading/writing 16KB blocks to `data/<graph-name>/data`.

```rust
pub struct DataFile {
    file: Mutex<File>,
    path: PathBuf,
}
```

Methods:
- `open(path) -> Self` — creates/opens data file
- `read_block(idx: BlockIdx) -> Result<[u8; BLOCK_SIZE]>` — read one block at offset `idx * BLOCK_SIZE`
- `write_block(idx: BlockIdx, data: &[u8; BLOCK_SIZE]) -> Result<()>` — write one block
- `allocate_blocks(count: u32) -> Result<(BlockIdx, u32)>` — extend file by N blocks, return start index
- `block_count() -> u64` — file size / BLOCK_SIZE

### 1.3 Block allocator (within-block chunks)

**File:** `src/storage2/block_allocator.rs`

Manages chunk allocation inside a single 16KB block via the header bitmap.

```rust
pub struct BlockAllocator;
```

- `alloc_chunks(bitmap: &mut [u8; 32], count: u8) -> Option<ChunkOffset>` — scan bits left→right, find `count` contiguous free chunks. Returns chunk offset (1-indexed) or None.
- `free_chunks(bitmap: &mut [u8; 32], offset: ChunkOffset, count: u8)` — clear bits
- `is_block_full(bitmap: &[u8; 32]) -> bool` — all bits set (except bit 0)
- `is_block_empty(bitmap: &[u8; 32]) -> bool` — no bits set (except bit 0)
- `chunk_count(bitmap: &[u8; 32]) -> u8` — count of set bits

### 1.4 Bitmap file (between-block allocation)

**File:** `src/storage2/bitmap_file.rs`

Manages block-level allocation across the data file.

```rust
pub struct BitmapFile {
    file: Mutex<File>,
    path: PathBuf,
    bitmap: Vec<u8>,           // loaded into memory, synced immediately on change
    free_blocks: Vec<BlockIdx>, // sorted list of blocks with free space
    last_zero_scan_pos: usize,  // position of last 0-bit found during scan
}
```

- `open(path, data_block_count) -> Self` — read or create bitmap file; scan for free blocks
- `alloc_block() -> Result<BlockIdx>` — pop from `free_blocks`; if empty, allocate new block from DataFile
- `mark_full(idx: BlockIdx)` — set bit → 1, remove from free_blocks
- `mark_free(idx: BlockIdx)` — set bit → 0, insert into free_blocks
- `sync()` — fsync bitmap to disk
- `reserve_free_blocks(count: usize)` — extend data file until free_blocks has enough entries

### 1.5 Block cache (LRU)

**File:** `src/storage2/block_cache.rs`

```rust
pub struct CachedBlock {
    pub data: Box<[u8; BLOCK_SIZE]>,
    pub is_dirty: bool,
    pub last_access: Instant,
    pub block_idx: BlockIdx,
}

pub struct BlockCache {
    blocks: HashMap<BlockIdx, CachedBlock>,
    lru_order: VecDeque<BlockIdx>,
    capacity: usize,           // number of blocks to cache (tunable, default 4096 = 64MB)
    stats: CacheStats,
}

pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub dirty_flushes: u64,
}
```

- `get_or_load(&mut self, idx: BlockIdx, loader: impl Fn(BlockIdx) -> Result<[u8; BLOCK_SIZE]>) -> &mut [u8; BLOCK_SIZE]`
- `mark_dirty(&mut self, idx: BlockIdx)`
- `evict_lru(&mut self) -> Option<CachedBlock>` — evict and return if dirty
- `flush_dirty(&mut self, flusher: impl Fn(BlockIdx, &[u8; BLOCK_SIZE]))`
- `flush_all_dirty(&mut self, flusher: impl Fn(BlockIdx, &[u8; BLOCK_SIZE]))`
- `transfer(dirty_blocks: &[BlockIdx])` — mark given blocks as dirty (used by redo log checkpoint)

### 1.6 Redo log

**File:** `src/storage2/redo_log.rs`

```rust
pub struct RedoLogEntry {
    pub op_type: OpType,
    pub op_id: u64,
    pub data: Vec<u8>,
}

pub struct RedoLog {
    path: PathBuf,
    current_file: Mutex<File>,
    current_size: AtomicU64,
    checkpoint_seq: AtomicU64,
    rotation_threshold: u64,   // 64MB
}
```

- `open(path) -> Self` — find the latest `redo_<timestamp>` file, or create new
- `append(op_type, op_id, data) -> Result<()` — write binary entry + CRC32, fsync
- `rotate() -> Result<()>` — close current file, create new `redo_<new_timestamp>`
- `replay<F>(path, mut callback: F)` where `F: FnMut(RedoLogEntry)` — iterate all redo files in order, calling callback for each entry
- `checkpoint(ready_entries: &[RedoLogEntry], flusher: impl Fn(...))` — flush data blocks corresponding to given entries, then remove consumed redo entries

**Checkpoint process:**
1. Find all redo log entries with seq ≤ checkpoint_seq
2. For each related data block, flush if dirty
3. Remove consumed redo log files
4. Update bitmap sync

### 1.7 Module root

**File:** `src/storage2/mod.rs`

```rust
pub mod types;
pub mod data_file;
pub mod block_allocator;
pub mod bitmap_file;
pub mod block_cache;
pub mod redo_log;
```

**Dependencies:** No dependency on any existing `src/storage/` code. Self-contained.

**Verification:**
- Unit tests: create data file, write/read blocks, verify cache eviction flushes dirty blocks
- Unit tests: bitmap alloc/free cycles, ensure no bit leak
- Unit tests: redo log append + replay produces identical entries

---

## Phase 2: Index Engine

### 2.1 Index file

**File:** `src/storage2/index_file.rs`

Same block-based 16KB file as data file, but each chunk is an index record.

```rust
pub struct IndexBlockHeader {
    pub offset: u8,
    pub bitmap: [u8; 32],
    pub status: BlockStatus,
    pub timestamp: u64,
    pub padding: [u8; 22],
}

pub struct VertexIndexRecord {
    pub chunk_type: u8,
    pub vertex_id: u32,
    pub data_block_idx: u32,
    pub data_chunk_offset: u8,
    pub data_len: u16,
    pub status: u8,
    pub version: u16,
    pub ctime: u64,
    pub mtime: u64,
    pub atime: u64,
    pub rank: u32,
    pub padding: [u8; 21],
}
// total: 64 bytes exactly (1 chunk)

pub struct EdgeIndexRecord {
    pub chunk_type: u8,
    pub edge_id: u32,
    pub data_block_idx: u32,
    pub data_chunk_offset: u8,
    pub data_len: u16,
    pub status: u8,
    pub version: u16,
    pub ctime: u64,
    pub mtime: u64,
    pub atime: u64,
    pub rank: u32,
    pub source: u32,
    pub target: u32,
    pub padding: [u8; 13],
}
// total: 64 bytes exactly (1 chunk)

pub struct TokenIndexRecord {
    pub chunk_type: u8,
    pub token_id: u32,
    pub data_block_idx: u32,
    pub data_chunk_offset: u8,
    pub data_len: u16,
    pub status: u8,
    pub ctime: u64,
    pub token: [u8; 43],       // padded to fill 64-byte chunk
}
// total: 64 bytes exactly (1 chunk)
```

```rust
pub struct IndexFile {
    data_file: DataFile,
    block_allocator: BlockAllocator,
    block_cache: BlockCache,
}
```

- `open(path) -> Self`
- `alloc_vertex_record(record) -> Result<ChunkOffset>` / `alloc_edge_record` / `alloc_token_record`
- `update_vertex_record` / `update_edge_record` / `update_token_record`
- `delete_record(block_idx, chunk_offset)`
- `read_vertex_record(block_idx, chunk_offset) -> VertexIndexRecord`

### 2.2 In-memory index structures

**File:** `src/storage2/memory_index.rs`

These are the in-memory data structures that serve queries. The index file is the backing store; memory index is rebuilt at startup.

```rust
// B+ tree mapping VertexId -> &VertexIndexRecord
pub struct VertexBTree {
    // simple B-tree or sorted Vec<VertexId, RecordPtr> for MVP
    inner: BTreeMap<u32, IndexPointer>,
}

// B+ tree mapping EdgeId -> &EdgeIndexRecord
pub struct EdgeBTree {
    inner: BTreeMap<u32, IndexPointer>,
}

// FST mapping token string -> Vec<&TokenIndexRecord>
pub struct TokenFst {
    // For MVP: use HashMap<String, Vec<IndexPointer>>
    // Future: real FST (fst crate)
    inner: HashMap<String, Vec<IndexPointer>>,
}

// B+ tree ordered by rank -> Vec<IndexPointer>
pub struct RankBTree {
    inner: BTreeMap<u32, Vec<IndexPointer>>,
}

pub struct IndexPointer {
    pub block_idx: u32,
    pub chunk_offset: u8,
}
```

**File:** `src/storage2/memory_index_builder.rs`

- `build_vertex_index(IndexFile) -> VertexBTree` — iterate all blocks, collect VertexIndexRecords
- `build_edge_index(IndexFile) -> EdgeBTree`
- `build_token_index(IndexFile) -> TokenFst`
- `build_rank_index(IndexTree...) -> RankBTree`

**Rebuild happens at graph startup.** The index file is fully scanned and in-memory trees are constructed. For large graphs this could take seconds; add a progress log.

### 2.3 Module root

**File:** `src/storage2/mod.rs` — add:

```rust
pub mod index_file;
pub mod memory_index;
pub mod memory_index_builder;
```

**Verification:**
- Unit test: create index file, insert 1000 vertex records, rebuild memory index from scratch, all found
- Unit test: update record → version/mtime/rank changed → verify after rebuild
- Unit test: delete record → not found after rebuild

---

## Phase 3: Query Engine

### 3.1 Graph facade

**File:** `src/graph2/graph.rs`

The central graph type — replaces both the old in-memory `Graph` and `DiskGraph`.

```rust
pub struct Graph {
    name: String,
    data_file: DataFile,
    bitmap_file: BitmapFile,
    block_cache: BlockCache,
    redo_log: RedoLog,
    index_file: IndexFile,
    vertex_index: VertexBTree,
    edge_index: EdgeBTree,
    token_index: TokenFst,
    rank_index: RankBTree,
    next_vertex_id: AtomicU32,
    next_edge_id: AtomicU32,
    next_token_id: AtomicU32,
    locks: LockManager,
}
```

**Lifecycle:**
- `Graph::open(path, name)` — opens files, loads bitmap into memory, rebuilds memory index, replays redo log
- `Graph::create(path, name)` — calls `open` for a new directory (files created on first write)

### 3.2 Vertex/Edge CRUD

**File:** `src/graph2/crud.rs`

```rust
impl Graph {
    // Create
    pub fn create_vertex(&self, name, labels, keywords, properties) -> Result<VertexId>
    pub fn create_edge(&self, source, target, label, keywords, strength, properties) -> Result<EdgeId>

    // Read
    pub fn get_vertex(&self, id: VertexId, at_time: Option<TimestampUs>) -> Result<Option<VertexPayload>>
    pub fn get_edge(&self, id: EdgeId, at_time: Option<TimestampUs>) -> Result<Option<EdgePayload>>

    // Update
    pub fn update_vertex(&self, id: VertexId, updates: VertexUpdate, record_history: bool) -> Result<()>
    pub fn update_edge(&self, id: EdgeId, updates: EdgeUpdate, record_history: bool) -> Result<()>

    // Delete
    pub fn soft_delete_vertex(&self, id: VertexId) -> Result<()>
    pub fn soft_delete_edge(&self, id: EdgeId) -> Result<()>
    pub fn hard_delete_vertex(&self, id: VertexId) -> Result<()>
    pub fn hard_delete_edge(&self, id: EdgeId) -> Result<()>

    // Traversal
    pub fn get_out_edges(&self, vertex_id: VertexId) -> Result<Vec<EdgeIndexRecord>>
    pub fn get_in_edges(&self, vertex_id: VertexId) -> Result<Vec<EdgeIndexRecord>>

    // Rank
    pub fn update_access_time(&self, id: u32, is_vertex: bool) -> Result<()>
}
```

**Create flow:**
1. Acquire write lock on new ID
2. Serialize payload (bincode) → compute required chunks
3. Allocate chunks from DataFile via BitmapFile
4. Write data blocks (through cache)
5. Allocate index record in IndexFile
6. Tokenize attributes → create/update token entries
7. Append to redo log
8. Update atime and rank in index

**Update flow:**
1. Lookup index record → get old data location
2. Serialize new payload → compute chunks
3. Allocate new chunks, write data
4. Update index record (new location, new version, new mtime)
5. Free old chunks, update bitmap
6. Tokenize → add new token refs with new version
7. Append to redo log
8. If recording history, push old payload to `history` array

**Delete flow:**
- Soft: set `status = Deleted` in index record, append redo log
- Hard: clear chunk type in index record, free data chunks, remove token refs, append redo log

### 3.3 Token extraction & search

**File:** `src/graph2/tokenizer.rs`

```rust
pub struct Tokenizer;

impl Tokenizer {
    /// Extract tokens from vertex/edge attributes
    pub fn extract_tokens(attrs: &[&str]) -> Vec<(String, Vec<Hit>)>;

    /// Tokenize a search query into words
    pub fn tokenize_query(query: &str) -> Vec<String>;
}
```

Token extraction rules:
- Split on whitespace and punctuation
- Lowercase
- Filter stop words (configurable list)
- For each attribute value, compute hit position (hit_key, hit_offset)

**Search:** handled in Gremlin step engine (Phase 3.5).

### 3.4 Gremlin step engine (new)

**File:** `src/graph2/gremlin.rs`

Reimplements the Gremlin pipeline query model on top of the new storage.

```rust
#[derive(Deserialize)]
pub struct GremlinQuery {
    pub steps: Vec<GremlinStep>,
}

#[derive(Deserialize)]
#[serde(tag = "step")]
pub enum GremlinStep {
    Search {
        keywords: Vec<String>,
        mode: Option<String>,          // "greedy" | "exact"
        at: Option<u64>,               // timestamp for time travel
        limit: Option<u32>,
        min_rank: Option<u32>,
    },
    V {
        ids: Option<Vec<u32>>,
        at: Option<u64>,
    },
    E {
        ids: Option<Vec<u32>>,
        at: Option<u64>,
    },
    Has {
        key: String,
        value: serde_json::Value,
    },
    HasNot {
        key: String,
        value: serde_json::Value,
    },
    HasKey {
        key: String,
    },
    HasValue {
        value: serde_json::Value,
    },
    HasLabel {
        label: String,
    },
    HasText {
        text: String,
    },
    Out {
        depth: Option<u8>,
        labels: Option<Vec<String>>,
    },
    In {
        depth: Option<u8>,
        labels: Option<Vec<String>>,
    },
    Both {
        depth: Option<u8>,
        labels: Option<Vec<String>>,
    },
    OutE {
        labels: Option<Vec<String>>,
    },
    InE {
        labels: Option<Vec<String>>,
    },
    BothE {
        labels: Option<Vec<String>>,
    },
    Values {
        keys: Option<Vec<String>>,
    },
    Limit {
        count: u32,
    },
    Count,
    Dedup,
    Repeat {
        steps: Vec<GremlinStep>,
        times: u8,
    },
    TimeTravel {
        at: u64,
    },
    Compact {
        before: u64,
    },
    Expand {
        depth: Option<u8>,
    },
    // New neuron-style activation traversal
    Activate {
        decay: Option<f32>,         // default 1.0
        activate: Option<f32>,      // default 0.0
        max_depth: Option<u8>,      // default 1
        min_score: Option<f32>,     // default 0.0
    },
}
```

**Pipeline execution (`exec(steps, graph) -> Result<Vec<GremlinResult>>`):**

Each step receives a `Vec<GremlinResult>` (current pipeline state) and produces a new `Vec<GremlinResult>`.

```rust
pub enum GremlinResult {
    Vertex {
        id: u32,
        data: VertexPayload,
        name: String,
        labels: Vec<String>,
        keywords: Vec<String>,
        properties: HashMap<String, PropertyValue>,
        score: Option<f32>,             // from search / activation
        rank: Option<u32>,
    },
    Edge {
        id: u32,
        data: EdgePayload,
        label: String,
        source: u32,
        target: u32,
        strength: f32,
        properties: HashMap<String, PropertyValue>,
        score: Option<f32>,
        rank: Option<u32>,
    },
}
```

**Search step implementation:**
1. Tokenize keywords
2. Look up each token in `TokenFst` → get `TokenIndexRecord` → load `TokenPayload` from data file
3. Collect all referenced vertex/edge IDs with ref_frequency and hits
4. If mode=greedy: match ANY keyword → initial score = max hit frequency across matched tokens
5. If mode=exact: match ALL keywords → initial score = 0 if any keyword missing
6. Filter by atime (time travel) — skip records with ctime > at
7. Sort by rank descending or by score descending
8. Apply limit

**Activate step (neuron-style traversal):**
1. Input: set of vertices/edges from previous step, each with score=1.0
2. BFS/DFS traversal with decay: score(next) = score(current) × decay × edge_strength
3. Cutoff: stop propagating if score < `activate` threshold
4. Collect: include in results if score ≥ `min_score`
5. Max depth bound by `max_depth`

### 3.5 Serialization helpers

**File:** `src/graph2/serialize.rs`

```rust
// Encode/decode VertexPayload to/from raw bytes (uses bincode)
pub fn serialize_vertex(v: &VertexPayload) -> Vec<u8>;
pub fn deserialize_vertex(data: &[u8]) -> Result<VertexPayload>;

// Same for EdgePayload, TokenPayload, HistoryRecord
pub fn serialize_edge(e: &EdgePayload) -> Vec<u8>;
pub fn deserialize_edge(data: &[u8]) -> Result<EdgePayload>;
pub fn serialize_token(t: &TokenPayload) -> Vec<u8>;
pub fn deserialize_token(data: &[u8]) -> Result<TokenPayload>;
```

### 3.6 Module root

**File:** `src/graph2/mod.rs`

```rust
pub mod graph;
pub mod crud;
pub mod tokenizer;
pub mod gremlin;
pub mod serialize;
```

**Verification:**
- Create vertex → get vertex by ID → matches
- Create edge → traversal out/in/both → returns correct edges
- Soft delete → get returns None, time-travel at previous time returns the data
- Update → new version has new data, history has old data
- Search by keyword → returns matching vertices/edges
- Activate traversal → scores decay correctly
- Time travel → all steps respect `at` parameter

---

## Phase 4: Locking & Concurrency

### 4.1 Lock types

**File:** `src/storage2/lock_manager.rs`

```rust
pub struct LockManager {
    // Per-block locks (for data + index blocks)
    block_locks: Vec<RwLock<()>>,
    // Per-vertex read/write intent
    vertex_locks: StripedRwLock<VertexId>,
    // Per-edge read/write intent
    edge_locks: StripedRwLock<EdgeId>,
}
```

Design decisions:
- Use `parking_lot::RwLock` for performance (no poisoning, fast path)
- Striped locking with ~1024 stripes (hash(id) % 1024) to avoid per-ID allocation
- Same stripe count for blocks — hash(block_idx) % stripes

**Granularity:**
- Data block write: exclusive lock on block_idx (prevents concurrent block mutations)
- Vertex/edge read: shared lock on hash(ID)
- Vertex/edge write: exclusive lock on hash(ID)
- Deadlock prevention: always acquire locks in a fixed order (block_idx → vertex_id → edge_id)

### 4.2 Lock-aware CRUD wrappers

In `Graph::create_vertex`, the lock sequence is:
1. Lock `block_locks[hash(allocated_block)]` exclusive (for data block write)
2. Lock `vertex_locks[hash(new_id)]` exclusive (for index record write)
3. Perform write
4. Unlock vertex
5. Unlock block

For `Graph::get_vertex(id)`:
1. Lock `vertex_locks[hash(id)]` shared
2. Lock `block_locks[hash(data_block_idx)]` shared
3. Read
4. Unlock block
5. Unlock vertex

### 4.3 Module dependency

`lock_manager` is used by `Graph` in `src/graph2/graph.rs`.

Add to `Cargo.toml`:
```toml
parking_lot = "0.12"
```

**Verification:**
- Concurrent reads on same vertex succeed
- Write blocks until all readers release
- No deadlock in stress test with random CRUD operations

---

## Phase 5: REST API Layer

### 5.1 New gremlin endpoint

**File:** `src/gremlin2/mod.rs`

New REST routes that use `Graph2` instead of the old `GraphManager`.

| Method | Path | Handler | Notes |
|--------|------|---------|-------|
| POST | `/gremlin` | `handle_gremlin2` | Accepts GremlinQuery, executes on the named graph |
| POST | `/search` | `handle_search2` | Shorthand for gremlin `[{"step":"search",...}]` |
| POST | `/vertices` | `create_vertex2` | |
| PUT | `/vertices/:id` | `update_vertex2` | |
| DELETE | `/vertices/:id` | `delete_vertex2` | |
| POST | `/edges` | `create_edge2` | |
| PUT | `/edges/:id` | `update_edge2` | |
| DELETE | `/edges/:id` | `delete_edge2` | |
| GET/POST/DEL | `/graphs` | (from old code, already compatible) | Graph lifecycle — can keep or port |

**Key difference:** The new handlers operate on `Arc<Graph>` stored in `GraphManager2` (a `HashMap<String, Arc<Graph>>`). The old `GraphManager` is replaced.

**Response shape:** Must match old format for frontend compatibility:

```json
// Old /gremlin response shape:
{
  "data": [
    {
      "type": "vertex" | "edge",
      "id": 1,
      "name": "...",
      "labels": ["..."],
      "properties": {...},
      "score": 0.85,
      "_original": { ... }   // full Vertex/Edge struct
    }
  ]
}

// New /gremlin must output the same shape.
```

### 5.2 Graph manager (new)

**File:** `src/graph_manager2.rs`

```rust
pub struct GraphManager2 {
    graphs: RwLock<HashMap<String, Arc<Graph>>>,
    data_dir: PathBuf,
}

impl GraphManager2 {
    pub fn new(data_dir: PathBuf) -> Self;
    pub fn open_or_create(&self, name: &str) -> Result<Arc<Graph>>;
    pub fn get(&self, name: &str) -> Result<Arc<Graph>>;
    pub fn list(&self) -> Result<Vec<String>>;
    pub fn delete(&self, name: &str) -> Result<()>;
    pub fn load_all(&self) -> Result<()>;
}
```

### 5.3 Settings endpoints

The `/settings/neural` endpoint needs to stay — frontend SettingsDialog calls it. In the new architecture, "neural" config becomes **search/traversal config**:

```json
{
  "search_mode": "greedy",
  "greedy_threshold": 0.6,
  "exact_threshold": 0.8,
  "max_results": 100,
  "activate_decay": 1.0,
  "activate_threshold": 0.0,
  "activate_max_depth": 1,
  "activate_min_score": 0.0
}
```

### 5.4 Module dependency

Replace `src/gremlin/` and `src/graph_manager.rs` with new implementations. Keep document/extract/MaaS endpoints unchanged.

**Verification:**
- `curl POST /gremlin` with a search step returns expected results
- `curl POST /vertices` creates a vertex, returns its ID
- Frontend GraphViewer can load and display graph data

---

## Phase 6: Frontend Adaptation

### 6.1 API.js changes

**File:** `src/ui/src/api.js`

Minimal changes expected:
- The gremlin response shape should be backward compatible — same JSON fields
- `/search` becomes a gremlin step (already is in current code)
- `/neurons` endpoints are deprecated (frontend already doesn't call them)
- `/settings/neural` response shape changes → adapt `SettingsDialog` if needed

**Likely changes:**
1. If the gremlin response `data[]._original` field changes shape, update `GraphViewer.jsx` parsing
2. If the vertex/edge creation response changes (old returns full object, new returns `{id}`), update the frontend to re-fetch

### 6.2 GraphViewer.jsx changes

**File:** `src/ui/src/components/GraphViewer.jsx`

- Gremlin response from `expand` / `V` / `search` steps now returns `data[].score` and `data[].rank` fields instead of `data[]._original.neuron_score`
- Vertex `_original.name` → top-level `name` (already the case in current frontend)
- Edge response: `_original.strength` → now available

### 6.3 SettingsDialog.jsx — Neuron tab → Search tab

**File:** `src/ui/src/components/SettingsDialog.jsx`

The existing "Neural" tab shows neuron-level config (Hebbian learning, firing thresholds);
replace it entirely with the new query engine's search + activation parameters.

#### 6.3.1 Backend API contract

The `/settings/neural` endpoint is **replaced by `/settings/Search`** with this new JSON schema:

```json
// GET /settings/Search → PUT /settings/Search
{
  "search_mode": "greedy",              // "greedy" | "exact"
  "greedy_threshold": 0.6,              // activation threshold when mode=greedy (0.0-1.0)
  "greedy_explore": false,              // whether to traverse from greedy search results
  "exact_threshold": 0.8,               // activation threshold when mode=exact (0.0-1.0)
  "exact_explore": false,               // whether to traverse from exact search results
  "max_results": 100,                   // cap on search result count
  "explore_decay": 1.0,                // decay factor for explore traversal (0.0-1.0)
  "explore_activate": 0.0,             // minimum score to continue propagation (0.0-1.0)
  "explore_max_depth": 1,              // max BFS depth for explore traversal (1-255)
  "explore_min_score": 0.0             // minimum score to include in explore result (0.0-1.0)
}
```

#### 6.3.2 New settings route in Rust backend

**File:** `src/gremlin2/settings.rs` (new)

```rust
// GET /settings/Search
pub async fn get_search_settings(
    State(state): State<AppState>,
) -> Json<SearchSettings>;

// PUT /settings/Search
pub async fn update_search_settings(
    State(state): State<AppState>,
    Json(settings): Json<SearchSettings>,
) -> StatusCode;
```

Stored in a new `SearchSettings` struct inside the new `GraphManager2`, serialized to
`data/graphs/<name>/search_settings.json` (per-graph settings) or a global config file.

Also add a **temporary backward-compat route** so the old `/settings/neural` still works:

```rust
// GET /settings/neural → redirects to /settings/Search (or wraps SearchSettings in old shape)
// PUT /settings/neural → unwraps old shape, forwards to /settings/Search
```

This lets the frontend migrate incrementally — deploy backend first, then frontend.

#### 6.3.3 React component changes

Replace the "Neural" tab JSX with a **"Search" tab**. The tab is organized into two collapsible
sections — one for **Greedy mode**, one for **Exact mode** — each with its own
"Explore from results" toggle that reveals traversal parameters when checked.

**Tab layout:**

```
┌─ Search ───────────────────────────────────────────┐
│                                                     │
│  Search Mode: [Greedy ▼]                           │
│  Max Results: [100]                                 │
│                                                     │
│  ── Greedy ─────────────────────────────────────── │
│  Threshold: [═══════●═══════] 0.6                   │
│  ☑ Explore from results                             │
│     │  Decay Rate: [════●═══════] 1.0              │
│     │  Activate Threshold: [●════════] 0.0         │
│     │  Max Depth: [2]                               │
│     │  Min Score: [════●═══════] 0.3              │
│                                                     │
│  ── Exact ──────────────────────────────────────── │
│  Threshold: [═══════════●═══] 0.8                   │
│  ☐ Explore from results                             │
│     (params hidden when unchecked)                   │
│                                                     │
└─────────────────────────────────────────────────────┘
```

**Field mapping:**

| Section | Fields | Mapping |
|---|---|---|
| **Search Mode** | Dropdown: "Greedy" or "Exact" | `search_mode` |
| | Number input: "Max Results" (1-1000) | `max_results` |
| **Greedy** | Slider: "Threshold" (0.0-1.0, step 0.05) | `greedy_threshold` |
| | Checkbox: "Explore from results" | `greedy_explore` |
| | *(when checked)* Slider: "Decay Rate" (0.0-1.0, step 0.05, default 1.0) | `explore_decay` |
| | *(when checked)* Slider: "Activate Threshold" (0.0-1.0, step 0.05, default 0.0) | `explore_activate` |
| | *(when checked)* Number input: "Max Depth" (1-10, default 1) | `explore_max_depth` |
| | *(when checked)* Slider: "Min Score" (0.0-1.0, step 0.05, default 0.0) | `explore_min_score` |
| **Exact** | Slider: "Threshold" (0.0-1.0, step 0.05) | `exact_threshold` |
| | Checkbox: "Explore from results" | `exact_explore` |
| | *(when checked)* Same 4 sub-params as Greedy (shared values) | same `explore_*` fields |

**Behavior:**
- The `explore_*` parameters are **shared** between Greedy and Exact modes — they represent
  the same traversal engine configuration regardless of which search mode triggered it
- When neither `greedy_explore` nor `exact_explore` is checked, the explore sub-params
  section is fully hidden and no `activate` step is appended to gremlin queries
- When the currently active search mode has explore enabled, the gremlin pipeline appends
  an `activate` step with these parameters

**Removed from UI** (old neuron tab):
- Hebbian learning toggle → deleted (no longer applicable)
- Co-fire window → deleted
- Synaptic decay → deleted
- Plasticity → deleted
- Firing history / refractory period → deleted

#### 6.3.4 API call update in SettingsDialog.jsx

Change the fetch/update calls:

```javascript
// BEFORE (old neural endpoint):
import { fetchNeuralConfig, updateNeuralConfig } from '../../api';
// GET  → fetchNeuralConfig()  → /settings/neural
// PUT  → updateNeuralConfig() → /settings/neural

// AFTER (new Search endpoint):
import { fetchSearchConfig, updateSearchConfig } from '../../api';

export async function fetchSearchConfig(graphName) {
  const res = await api(`/settings/Search${graphName ? `?graph=${graphName}` : ''}`);
  return res.json();
}

export async function updateSearchConfig(config, graphName) {
  return api(
    `/settings/Search${graphName ? `?graph=${graphName}` : ''}`,
    { method: 'PUT', body: JSON.stringify(config) }
  );
}
```

#### 6.3.5 i18n update

**File:** `src/ui/src/i18n/` (en.json, zh.json)

Update neural-related translation keys:

| Old key | New key |
|---------|---------|
| `settings.neural.title` | `settings.search.title` ("Search" / "搜索") |
| `settings.neural.hebbian_learning` | (removed) |
| `settings.neural.co_fire_window` | (removed) |
| `settings.neural.synaptic_decay` | (removed) |
| (none) | `settings.search.search_mode` → "Search Mode" / "搜索模式" |
| (none) | `settings.search.greedy_threshold` → "Greedy Threshold" / "贪婪阈值" |
| (none) | `settings.search.greedy_explore` → "Explore from Results" / "从结果探索" |
| (none) | `settings.search.exact_threshold` → "Exact Threshold" / "精确阈值" |
| (none) | `settings.search.exact_explore` → "Explore from Results" / "从结果探索" |
| (none) | `settings.search.max_results` → "Max Results" / "最大结果数" |
| (none) | `settings.search.explore_decay` → "Decay Rate" / "衰减率" |
| (none) | `settings.search.explore_activate` → "Activate Threshold" / "激活阈值" |
| (none) | `settings.search.explore_max_depth` → "Max Depth" / "最大深度" |
| (none) | `settings.search.explore_min_score` → "Min Score" / "最低分数" |

### 6.4 ChatArea.jsx — Conditionally append Activate step from settings

**File:** `src/ui/src/components/ChatArea.jsx`

The ChatArea currently constructs gremlin search steps like:

```javascript
// Current: simple search step
const steps = [
  { step: 'search', keywords: splitKeywords(query), mode: settings.searchMode }
];
```

After the new engine, check the search settings — if the active mode's `explore` flag is on,
append an `Activate` step using the stored explore parameters:

```javascript
// New: optionally append activate step based on settings
const steps = [
  { step: 'search', keywords: splitKeywords(query), mode: settings.searchMode }
];

const isExploreOn = settings.searchMode === 'greedy'
  ? settings.greedy_explore
  : settings.exact_explore;

if (isExploreOn) {
  steps.push({
    step: 'activate',
    decay: settings.explore_decay,
    activate: settings.explore_activate,
    max_depth: settings.explore_max_depth,
    min_score: settings.explore_min_score
  });
}
```

#### 6.4.1 Result display

Search results that include activation now carry `data[].score` — surface this in the
chat result card. Add a "score" badge next to each vertex/edge name in the graph
result display:

```
[🔗 Neuron A · score 0.85] ── [🔗 Neuron B · score 0.72]
                               ╰─ [🔗 Neuron C · score 0.34]
```

#### 6.4.2 API.js additions

**File:** `src/ui/src/api.js`

Add fetch/update functions for the new Search settings endpoint (same as 6.3.4):

```javascript
export async function fetchSearchConfig(graphName) {
  const res = await api(`/settings/Search${graphName ? `?graph=${graphName}` : ''}`);
  return res.json();
}

export async function updateSearchConfig(config, graphName) {
  return api(
    `/settings/Search${graphName ? `?graph=${graphName}` : ''}`,
    { method: 'PUT', body: JSON.stringify(config) }
  );
}
```

Keep old `fetchNeuralConfig` / `updateNeuralConfig` as **deprecated wrappers** for backward compat
during the transition period:

```javascript
// Deprecated — kept for backward compat
export async function fetchNeuralConfig() {
  console.warn('fetchNeuralConfig is deprecated, use fetchSearchConfig');
  return fetchSearchConfig();
}
export async function updateNeuralConfig(config) {
  console.warn('updateNeuralConfig is deprecated, use updateSearchConfig');
  return updateSearchConfig(config);
}
```

### 6.5 Component file changes summary

| File | Changes |
|------|---------|
| `src/ui/src/components/SettingsDialog.jsx` | Replace "Neural" tab JSX → "Search" tab; Greedy/Exact sections each with "Explore from results" checkbox + conditional sub-params; update import |
| `src/ui/src/components/ChatArea.jsx` | Conditionally append `activate` gremlin step based on search settings (explore flag + explore_* params); no inline controls |
| `src/ui/src/api.js` | Add `fetchSearchConfig` / `updateSearchConfig` (`/settings/Search`); deprecate `fetchNeuralConfig` / `updateNeuralConfig` |
| `src/ui/src/i18n/en.json` | Add search/explore translation keys; remove neural keys |
| `src/ui/src/i18n/zh.json` | Same for Chinese |
| `src/gremlin2/settings.rs` | **New file** — `/settings/Search` GET + PUT handlers; `/settings/neural` backward-compat wrapper |
| `src/gremlin2/mod.rs` | Register `/settings/Search` and `/settings/neural` routes |

---

## Phase 7: Cluster (Future / Optional)

**Stub:** `src/cluster/mod.rs`

Reserved for when multi-node support is needed:
- Redo log transfer protocol (master pushes, worker replays)
- Heartbeat / health check
- Write forwarding from worker to master
- Read query queuing on master for distributed processing

Not implemented in this plan. Add a placeholder:

```rust
// src/cluster/mod.rs
// Placeholder for future master-worker replication.
// Design document: .reasonix/plans/100-graph-rearch-design.md#cluster-design
```

---

## Phase 8: Cleanup & Migration

### 8.1 Remove old code

After new stack is verified, delete:

| Path | Lines | Notes |
|------|-------|-------|
| `src/graph/` | 1,357 | Entire module — replaced by `src/graph2/` |
| `src/storage/` | 3,857 | Entire module — replaced by `src/storage2/` |
| `src/neuron/` | 1,284 | Entire module — replaced by token-based index |
| `src/gremlin/` | 3,858 | Replaced by `src/gremlin2/` |
| `src/graph_manager.rs` | ~200 | Replaced by `graph_manager2.rs` |
| `src/memory_system.rs` | ~150 | Legacy single-graph wrapper |
| `src/persistence/` | ~700 | Old bincode snapshot persistence |

**Total removed:** ~11,400 lines.

### 8.2 Rename directories

Optionally rename after removal:
- `src/storage2/` → `src/storage/`
- `src/graph2/` → `src/graph/`
- `src/gremlin2/` → `src/gremlin/`

### 8.3 Data migration tool

**File:** `src/bin/migrate_graph.rs`

One-time CLI tool to read old `data/graphs/<name>/` format and write new format:
1. Load old `graph.bin` (full graph snapshot via bincode)
2. For each vertex: `create_vertex` on new Graph
3. For each edge: `create_edge` on new Graph
4. Verify counts match

### 8.4 Update Cargo.toml

Remove unused deps: `uuid` (maybe), `bincode` (keep for serialization), add `parking_lot`.

### 8.5 Update module declarations

**File:** `src/lib.rs`
```rust
pub mod config;
pub mod documents;
pub mod extract;
// Remove: graph, graph_manager, gremlin, neuron, persistence, storage, memory_system
// Add:
pub mod graph2;
pub mod storage2;
pub mod gremlin2;
pub mod graph_manager2;
// pub mod cluster;
pub mod ui_serve;
pub mod maas;
```

**Verification:**
- `cargo build` succeeds
- `cargo test` passes (old tests removed; new tests added per phase)
- Manual: start server → frontend loads → create vertices/edges → search → traverse

---

## Implementation order & dependencies

```
Phase 1 ──→ Phase 2 ──→ Phase 3 ──→ Phase 4
                                ↓
                            Phase 5 ←── (depends on Phase 3 + 4)
                                ↓
                            Phase 6 (can start after Phase 5 is stable)
                                ↓
                            Phase 8 (all phases complete)
```

**Parallel tracks:**
- Phase 1, 2, 3, 4 must be sequential within each track (each depends on previous)
- Phase 5 & 6 can start once Phase 3 & 4 are functional
- Phase 8 must be last

---

## Risk assessment

| Risk | Impact | Mitigation |
|------|--------|------------|
| Block allocation fragmentation | Storage waste | Defragmentation pass (future phase) |
| FST crate not available in Rust | Token index slow | MVP with `HashMap`, swap to `fst` crate later |
| Frontend response shape mismatch | UI broken | Early integration test with real API calls |
| Performance worse than old | Bad UX | Keep old code until new passes perf benchmarks |
| Redo log replay at startup slow | Long startup | Add progress bar; test with 100K+ vertex graphs |
| Deadlock in concurrent access | Hard to debug | Strict lock ordering + stress test suite |

---

## Files to create / modify

### Create (new files)
| File | Est. lines | Phase |
|------|-----------|-------|
| `src/storage2/types.rs` | 150 | 1 |
| `src/storage2/block_allocator.rs` | 120 | 1 |
| `src/storage2/data_file.rs` | 100 | 1 |
| `src/storage2/bitmap_file.rs` | 250 | 1 |
| `src/storage2/block_cache.rs` | 300 | 1 |
| `src/storage2/redo_log.rs` | 400 | 1 |
| `src/storage2/mod.rs` | 30 | 1 |
| `src/storage2/lock_manager.rs` | 200 | 4 |
| `src/storage2/index_file.rs` | 300 | 2 |
| `src/storage2/memory_index.rs` | 200 | 2 |
| `src/storage2/memory_index_builder.rs` | 200 | 2 |
| `src/graph2/serialize.rs` | 100 | 3 |
| `src/graph2/tokenizer.rs` | 150 | 3 |
| `src/graph2/crud.rs` | 800 | 3 |
| `src/graph2/gremlin.rs` | 1,000 | 3 |
| `src/graph2/graph.rs` | 400 | 3 |
| `src/graph2/mod.rs` | 20 | 3 |
| `src/gremlin2/mod.rs` | 600 | 5 |
| `src/graph_manager2.rs` | 150 | 5 |
| `src/bin/migrate_graph.rs` | 150 | 8 |
| `src/cluster/mod.rs` | 10 | 7 |

~5,230 lines new code

### Modify (existing files)
| File | Est. changes | Phase |
|------|-------------|-------|
| `src/lib.rs` | Module declarations | 8 |
| `Cargo.toml` | Add `parking_lot`, remove unused | 4/8 |
| `src/ui/src/api.js` | Response shape adaptation | 6 |
| `src/ui/src/components/GraphViewer.jsx` | Response field parsing | 6 |
| `src/ui/src/components/SettingsDialog.jsx` | Neural → search config UI | 6 |
| `src/main.rs` | Use GraphManager2 instead of old | 5 |

### Delete
| Path | Phase |
|------|-------|
| `src/graph/` | 8 |
| `src/storage/` | 8 |
| `src/neuron/` | 8 |
| `src/gremlin/` | 8 |
| `src/graph_manager.rs` | 8 |
| `src/memory_system.rs` | 8 |
| `src/persistence/` | 8 |

---

## First step recommendation

Start with **Phase 1 — Block Storage Engine**. Specifically:

1. `src/storage2/types.rs` — all type definitions
2. `src/storage2/block_allocator.rs` — chunk allocator
3. `src/storage2/data_file.rs` — raw block I/O
4. `src/storage2/bitmap_file.rs` — block-level space management
5. `src/storage2/block_cache.rs` — LRU cache with dirty tracking
6. `src/storage2/redo_log.rs` — WAL with rotation + replay
7. Unit tests for each component

Each sub-step builds on the previous one and can be verified independently.
