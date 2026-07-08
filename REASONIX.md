# Bionic-Graph — REASONIX.md

## Stack
- **Language**: Rust 2021 edition
- **Web framework**: axum 0.7 (REST API, JSON, CORS) — uses `:param` path syntax
- **Serialization**: serde + serde_json + bincode (binary persistence)
- **CLI**: clap 4 (derive)
- **Async runtime**: tokio (full)
- **Concurrency**: parking_lot 0.12 (striped RwLock pools)
- **Tokenizer**: jieba-rs 0.10 (CJK + English segmentation)
- **Config**: `~/.config/bionic-graph/settings.json`, auto-generated on first run
- **Frontend**: React 19 + Vite 8 + Tailwind CSS 4 + vis-network (Canvas 2D)
- **Frontend embedding**: rust-embed (compile-time embedding into Rust binary)

## Layout

### Backend (Rust)

```
src/
├── main.rs                  # CLI entry + HTTP server bootstrap
├── lib.rs                   # Crate root — 11 pub mod declarations
├── config/                  # Settings structs + JSON file loader
│   ├── mod.rs               # Re-exports
│   ├── loader.rs            # ~/.config/bionic-graph/settings.json load/save
│   └── settings.rs          # ServerConfig, LlmConfig, StorageConfig,
│                            #   ClusterConfig, SearchSettings (greedy/exact)
├── storage/                 # Block-based storage engine (16KB blocks, 64B chunks)
│   ├── mod.rs               # Re-exports 9 submodules
│   ├── types.rs             # Fundamental types, constants, binary layouts
│   ├── data_file.rs         # Raw 16KB block I/O (Mutex<File>)
│   ├── bitmap_file.rs       # Block-level free/used tracking
│   ├── block_allocator.rs   # Chunk-level allocator within a 16KB block
│   ├── block_cache.rs       # LRU cache with dirty tracking (default 4096 blocks = 64MB)
│   ├── redo_log.rs          # WAL: FIFO queue + background batch writer (≤128 entries),
│   │                        #   size (64MB) + time (15min, configurable) rotation,
│   │                        #   checkpoint protocol, CRC32, replay
│   ├── index_file.rs        # On-disk index (64B fixed records: Vertex/Edge/Token)
│   ├── memory_index.rs      # In-memory BTreeMap/HashMap indexes
│   └── memory_index_builder.rs  # Rebuild in-memory index at startup
├── lock/                    # Striped RwLock pools for concurrency
│   ├── mod.rs
│   └── lock_manager.rs      # LockManager: metadata → block → vertex → edge
├── graph/                   # Graph engine: CRUD + Gremlin pipeline + tokenizer
│   ├── mod.rs               # Re-exports
│   ├── graph.rs             # Graph struct (facade), GraphConfig, lifecycle
│   ├── crud.rs              # Vertex/Edge CRUD with WAL + token extraction + rank
│   ├── gremlin.rs           # Gremlin pipeline step engine (24 steps)
│   ├── locked.rs            # Lock-safe CRUD wrappers
│   ├── serialize.rs         # Bincode serialization with JSON properties
│   ├── tokenizer.rs         # jieba-rs tokenizer, stop-words, min length 2
│   └── tests.rs             # #[cfg(test)] integration tests (90+)
├── gremlin/                 # REST API routes + handlers (axum)
│   ├── mod.rs               # AppState, build_router (29 routes), handlers
│   └── settings.rs          # GET/PUT /settings/search + legacy /settings/neural
├── graph_manager.rs         # Multi-graph manager (HashMap<String, Arc<Graph>>), close_all()
├── documents.rs             # Document CRUD (file storage + JSON index)
├── extract/                 # LLM-based document extraction pipeline
│   ├── mod.rs               # Re-exports
│   ├── config.rs            # ExtractionConfig, ExtractedEntity(name,labels,keywords,properties),
│   │                        #   ExtractedRelation(source,target,name,labels,keywords,strength,properties)
│   ├── document.rs          # Markdown section parser + token budget
│   ├── extraction.rs        # LLM prompt templates (full-field format) + response parsers
│   ├── llm_client.rs        # OpenAI-compatible HTTP client with retry
│   └── task_manager.rs      # Async task lifecycle
├── maas/                    # MaaS OpenAI-compatible proxy
│   ├── mod.rs
│   └── openai.rs            # GET /v1/models + POST /v1/chat/completions (SSE)
├── cluster/                 # Master-worker cluster mode
│   ├── mod.rs
│   ├── config.rs            # ClusterConfig
│   ├── node.rs              # NodeRegistry (master/worker)
│   ├── forward.rs           # Write forwarding (worker → master)
│   └── replication.rs       # Redo-log replication
└── ui_serve.rs              # Embedded static file serving (rust-embed)
```

### Frontend (React)

```
src/ui/
├── src/
│   ├── App.jsx              # Root component
│   ├── api.js               # API client + LLM streaming
│   ├── components/
│   │   ├── Sidebar.jsx      # Navigation + conversation list
│   │   ├── ChatArea.jsx     # Chat orchestration
│   │   ├── MessageList.jsx  # Message rendering
│   │   ├── ChatInput.jsx    # Input + controls
│   │   ├── GraphViewer.jsx  # vis-network Canvas 2D visualization
│   │   ├── GraphManagerDialog.jsx  # Graph library management
│   │   ├── KnowledgeBase.jsx       # Document management dialog
│   │   ├── SettingsDialog.jsx      # Settings panel
│   │   └── PropertyPanel.jsx       # Node/edge property inspector
│   └── locales/             # i18n (en/zh)
├── test/
│   └── e2e/                 # Playwright end-to-end tests
└── dist/                    # Compiled frontend (embedded in binary)
```

## Commands
- **build**: `cargo build` (runs `npm --prefix src/ui run build` first)
- **release**: `cargo build --release`
- **test**: `cargo test` + `npm --prefix src/ui run test`
- **run**: `cargo run` → `http://127.0.0.1:8080`
- **frontend dev**: `npm --prefix src/ui run dev`
- **frontend build**: `npm --prefix src/ui run build`
- **frontend test**: `npm --prefix src/ui run test`
- **frontend e2e**: `node src/ui/test/e2e/<name>.mjs`

## Data Directory Structure

```
<data_dir>/                      (default: "data")
├── graphs/
│   └── <graph_name>/
│       ├── data                — Data file (16KB blocks)
│       ├── bitmap              — Bitmap (block-level free space tracking)
│       ├── index               — Index file (64B fixed records)
│       ├── config.json         — Per-graph config (cache_capacity, rotation_thresholds, etc.)
│       └── redo_<yyyymmddHHMMss>_<######>  — WAL files (size + time-based rotation)
└── documents/
    ├── index.json              — Document metadata index
    └── YYMMDD/
        └── <id>.md
```

## Frontend Architecture

### Stack
- React 19, Vite 8, Tailwind CSS 4
- `vis-network` + `vis-data` (Canvas 2D)
- `i18next` (EN/ZH)

### Layout
```
App.jsx
├── Sidebar.jsx          — 对话列表 + 知识库/图库/设置入口
├── ChatArea.jsx         — 聊天主区域
│   ├── MessageList.jsx  — 消息列表
│   └── ChatInput.jsx    — 输入框 + 模式控制栏
├── KnowledgeBase.jsx    — 知识库弹窗
├── GraphManagerDialog.jsx — 图库管理弹窗
└── SettingsDialog.jsx   — 设置弹窗
```

## Gremlin Steps (24 total)

| Step | Parameters | Description |
|------|-----------|-------------|
| `search` | `text`, `mode?`, `match_mode?`, `at?`, `limit?`, `min_rank?` | Full-text search via token index. Auto-injects `match_mode` + optional `traverse` step. |
| `V` | `ids?`, `at?` | Vertices by ID |
| `E` | `ids?`, `at?` | Edges by ID |
| `has` / `hasNot` / `hasKey` / `hasValue` / `hasLabel` / `hasText` | (6 filter steps) | Property/label filters |
| `out` / `in` / `both` | `depth?`, `labels?` | Vertex traversal (BFS) |
| `outE` / `inE` / `bothE` | `labels?` | Edge traversal |
| `values` / `limit` / `count` / `dedup` | — | Result processing |
| `repeat` | `steps`, `times` | Loop sub-pipeline |
| `timeTravel` | `at` | Set query time point |
| `compact` | `before` | Passthrough stub |
| `expand` | `depth?` | Add neighbors + edges |
| `traverse` | `decay?`, `activate?`, `max_depth?`, `min_score?` | BFS activation spread |

## REST API Endpoints (29 routes)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | System health |
| GET/POST/DELETE | `/graphs`, `/graphs/:name` | Graph lifecycle |
| GET/PUT | `/graphs/:name/config` | Per-graph config |
| POST | `/gremlin` | Gremlin pipeline query |
| GET | `/search` | Token search shortcut |
| POST/PUT/DELETE | `/vertices`, `/vertices/:id` | Vertex CRUD |
| POST/PUT/DELETE | `/edges`, `/edges/:id` | Edge CRUD |
| GET/PUT | `/settings/search` | Search settings |
| GET/PUT | `/settings/neural` | Legacy compat wrapper |
| GET | `/documents` | List documents |
| POST | `/documents` | Create document |
| GET | `/documents/:id` | Get document metadata |
| PUT | `/documents/:id` | Update document |
| DELETE | `/documents/:id` | Delete document |
| GET | `/documents/:id/content` | Document body |
| POST | `/extract` | Submit extraction |
| GET | `/extract/task/:task_id` | Task polling |
| GET | `/extract/tasks` | List extraction tasks |
| GET | `/maas/openai/v1/models` | Model listing |
| POST | `/maas/openai/v1/chat/completions` | Chat proxy (SSE) |

> Graph name via `?graph=` query param (default `"default"`). DELETE supports `?force=true`.

## WAL Write Path Architecture

The WAL uses a FIFO queue + background batch writer:

```
CRUD → encode_entry() → send(WriterMessage::Entry) → wait(Condvar, epoch)
       Background writer thread:
       recv() → accumulate batch (≤128 entries, 10ms timeout)
             → check rotation (size 64MB OR age 15min)
             → write_all(batch) → fsync → advance_epoch → notify_all
```

Caller blocks until the writer confirms durability. Features:
- **Batching**: up to 128 entries per fsync (vs. 1 entry per fsync before)
- **Ordering**: FIFO channel guarantees operation sequence
- **Crash safety**: WAL file is a real on-disk file (not orphaned FD)
- **Checkpoint on rotation**: flush dirty data blocks before deleting old WAL
- **SIGINT grace**: `GraphManager::close_all()` flushes all graphs + checkpoints WALs

## GraphStorageConfig

Per-graph config in `config.json`:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `cache_capacity` | usize | 4096 | LRU block cache entries (×16KB = 64 MB) |
| `max_dirty_age_secs` | Option<u64> | 60 | Auto-flush dirty blocks after N seconds |
| `rotation_threshold_mb` | u64 | 64 | WAL size threshold for rotation (MB) |
| `rotation_max_age_secs` | Option<u64> | 900 | WAL age threshold (seconds, 15 min) |
| `free_list_target` | usize | 128 | Bitmap free-list pre-fill count |

## Watch out for
- **Route params**: axum 0.7.9 requires `:param` syntax.
- **Data dir**: `<data_dir>/graphs/<name>/` with files: `data`, `bitmap`, `index`, `config.json`, `redo_*`.
- **Default graph**: `"default"` when `?graph=` omitted.
- **POST /vertices**: top-level `name` (String), optional `keywords`, `labels`, `properties`. `properties.name` NOT used.
- **POST /edges**: requires `source`, `target`, `name` (String). Optional `labels` (Vec<String>), `keywords` (Vec<String>), `strength` (f32, default 1.0), `properties` (map).
- **DELETE ?force=true**: hard delete; without force: soft delete (DataStatus::Deleted).
- **Search step**: takes `text` (raw string), tokenized by jieba-rs. `mode`="greedy"|"exact", `match_mode`="prefix"|"word".
- **`/gremlin` auto-injects**: `match_mode` from SearchSettings + optionally appends `traverse` step.
- **Time travel**: `at` on steps; `timeTravel` step sets global timestamp.
- **traverse step**: BFS via score * decay * edge_strength; stops when score < activate.
- **Memory index rebuilt at startup** — no incremental persistence.
- **Lock order**: metadata → block → vertex → edge (enforced by helpers).
- **Properties as JSON strings** inside binary blob (bincode incompatibility).
- **Token strings**: `[u8; 43]` inline — >43 chars truncated.
- **compact step**: no-op passthrough.
- **`touch src/ui_serve.rs`** needed after frontend changes.
- **`document_extractor.rs`, `pipeline.rs`**: orphaned dead code (not in mod.rs).
- **EdgePayload fields**: `name` (relationship name), `labels` (relation type categories), `keywords`, `strength`, `properties`, `source`, `target`.
- **VertexPayload fields**: `name`, `labels` (entity types), `keywords`, `properties`.
- **Extraction**: SYSTEM_PROMPT tells LLM to output `name`, `labels`, `keywords`, `properties` for entities; and `source`, `target`, `name`, `labels`, `keywords`, `strength`, `properties` for relations. Parsing skips entries without a valid `name`.
- **WAL batch writer**: `append()` sends via `mpsc::channel` to background thread. Caller blocks on Condvar until durability confirmed. Batch ≤128 entries, 10ms timeout.
- **Time-based WAL rotation**: `rotation_max_age_secs` in per-graph `config.json`. Default 900s (15 min).
- **SIGINT/SIGTERM**: server calls `GraphManager::close_all()` → flushes dirty blocks + checkpoints all WALs.
- **WAL file naming**: `redo_<yyyymmddHHMMss>_<######>` (zero-padded seq for intra-second disambiguation).
- **`Graph::close()`**: calls `flush()` + `sync()` + `renew()`. No longer uses the old `checkpoint()` closure API.

## Implemented Plans
- `011-diskgraph-integration-incremental-persistence.md`
- `2024-06-23-search-mode-theme-doc-fields.md`
- `007-settings-neural-config-search-ui.md`
- `008-chat-input-toolbar-layout.md`
- `001-arch-verify.md`
- `002-section-paragraph-graph.md`
- `003-keyword-semantic-search.md`
- `005-ui-rewrite-knowledgebase-visnetwork.md`
- `2024-06-23-vertex-redolog-overhaul.md`
- `009-maas-proxy-neural-fix-frontend-polish.md`
- `010-session-comprehensive-refactor.md`
- `012-neural-activation-spread-enhancements.md`
- `100-graph-rearch-design.md` — Block-based storage architecture
- `101-graph-rearch-plan.md` — Re-architecture coding plan (Phase 1-8)
- `--- edge-data-structure-update.md` — EdgePayload label→name, +labels; token hit_key uses property keys; extraction prompt updated
