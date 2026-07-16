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
│                            #   ClusterConfig, SearchSettings, RankConfig
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
│   ├── memory_index.rs      # In-memory BTreeMap/HashMap indexes (vertex, edge,
│   │                        #   token, rank, atime, adjacency)
│   └── memory_index_builder.rs  # Rebuild in-memory index at startup
├── lock/                    # Striped RwLock pools for concurrency
│   ├── mod.rs
│   └── lock_manager.rs      # LockManager: metadata → block → vertex → edge
├── graph/                   # Graph engine: CRUD + Gremlin pipeline + tokenizer
│   ├── mod.rs               # Re-exports
│   ├── graph.rs             # Graph struct (facade), GraphConfig, lifecycle
│   ├── graph_registry.rs    # Graph metadata registry (persistent, multi-graph)
│   ├── crud.rs              # Vertex/Edge CRUD with WAL + token extraction + rank
│   ├── gremlin.rs           # Gremlin pipeline step engine (25 steps)
│   ├── locked.rs            # Lock-safe CRUD wrappers
│   ├── serialize.rs         # Bincode serialization with JSON properties
│   ├── tokenizer.rs         # jieba-rs tokenizer, stop-words, min length 2
│   ├── rank_decay.rs        # Periodic rank decay background task
│   └── tests.rs             # #[cfg(test)] integration tests (90+)
├── gremlin/                 # REST API routes + handlers (axum)
│   ├── mod.rs               # AppState, build_router (44 routes), handlers
│   ├── settings.rs          # GET/PUT /settings/search, /settings/llm, /settings/rank, /settings/tokenizer
│   └── tokenizer_settings.rs # Custom tokenizer dictionary words CRUD
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
│   ├── server.rs            # Cluster HTTP server (heartbeat/forward/replicate/touch)
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
│   │   ├── SettingsDialog.jsx      # Settings panel (搜索 + 排序 tabs)
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
└── SettingsDialog.jsx   — 设置弹窗（搜索/排序/LLM 三个页签）
```

## Gremlin Steps (25 total)

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
| `expand` | `depth?`, `label?` | Add neighbors + edges, optionally filtered by edge label |
| `traverse` | `decay?`, `activate?`, `max_depth?`, `min_score?` | BFS activation spread |
| `rank` | `limit?`, `min?` | Return top results by rank (source or filter step) |

## REST API Endpoints (44 routes)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | System health |
| GET/POST/PUT | `/graphs` | List / create / set-default graph |
| DELETE/PUT | `/graphs/:name` | Delete / update graph metadata |
| GET/PUT | `/graphs/:name/config` | Per-graph storage config |
| POST | `/gremlin` | Gremlin pipeline query |
| GET | `/search` | Token search shortcut |
| POST/PUT/DELETE | `/vertices`, `/vertices/:id` | Vertex CRUD |
| GET/PUT | `/vertices/:id/meta` | Vertex metadata (rank/atime/status) |
| POST/PUT/DELETE | `/edges`, `/edges/:id` | Edge CRUD |
| GET/PUT | `/edges/:id/meta` | Edge metadata |
| GET/PUT | `/settings/search` | Search settings (greedy/exact) |
| GET/PUT | `/settings/rank` | Rank decay config |
| GET/PUT | `/settings/llm` | LLM provider config |
| GET | `/settings/tokenizer` | Tokenizer custom dictionary config |
| POST/DELETE | `/settings/tokenizer/words` | Add / remove custom tokenizer words |
| GET/POST | `/documents` | List / create documents |
| GET/PUT/DELETE | `/documents/:id` | Get / update / delete document metadata |
| GET | `/documents/:id/content` | Document body |
| POST | `/extract` | Submit extraction task |
| POST | `/documents/:id/extract` | Extract from document by ID |
| GET | `/extract/task/:task_id` | Task polling |
| GET | `/extract/tasks` | List extraction tasks |
| GET | `/maas/openai/v1/models` | Model listing |
| POST | `/maas/openai/v1/chat/completions` | Chat proxy (SSE) |

> Default graph: `"graph0"` when `?graph=` omitted. DELETE supports `?force=true`.

## RankConfig

Settings under `"rank"` key in settings.json:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `auto_inc_rank_when_update` | bool | true | Increment rank on vertex/edge update |
| `auto_inc_rank_when_read` | bool | true | Increment rank on vertex/edge read |
| `auto_dec_rank_when_inactive` | bool | true | Periodically decay rank for inactive entities |
| `inactive_after_accessed_secs` | u64 | 1296000 | Seconds of inactivity before considered stale (15 days) |
| `inactive_rank_update_period` | u64 | 86400 | Rank decay scan interval in seconds (1 day) |

## MemoryIndex

| Index | Type | Purpose |
|-------|------|---------|
| `vertices` | BTreeMap<u32, IndexPointer> | Vertex ID → pointer |
| `edges` | BTreeMap<u32, IndexPointer> | Edge ID → pointer |
| `tokens` | BTreeMap<String, Vec<IndexPointer>> | Token string → pointers (prefix search) |
| `ranks` | BTreeMap<u32, Vec<IndexPointer>> | Rank → pointers (descending order for hot queries) |
| `atime_index` | BTreeMap<u64, Vec<IndexPointer>> | Atime → pointers (range scan for inactivity decay) |
| `adjacency` | HashMap | Vertex → outgoing/incoming edges |
| `entity_tokens` | HashMap<(u8, u32), Vec<String>> | Entity → token strings (for hard delete cleanup) |

## Cluster Architecture

```
┌─────────┐     ┌─────────┐     ┌─────────┐
│ Worker 1│     │ Master  │     │ Worker 2│
│ (read)  │◄────│(R+W)    │────►│ (read)  │
└────┬────┘     └─────────┘     └────┬────┘
     │               │               │
     └─── writes ────┘               │
          forwarded                  │
                                     │
        Redo log replication ────────┘
```

**Cluster endpoints** (on cluster bind_addr):
| Method | Path | Direction | Description |
|--------|------|-----------|-------------|
| POST | `/cluster/heartbeat` | Worker → Master | Worker registration + heartbeat |
| POST | `/cluster/forward` | Worker → Master | Forwarded write request |
| POST | `/cluster/replicate` | Master → Worker | Redo log entry push |
| POST | `/cluster/touch` | Worker → Master | Read report for rank/atime update |

## Rank Lifecycle

```
Update → update_vertex/edge: rank += 1, atime = now ──────────┐
                                                                │
Read → execute_gremlin → process_touch ───► get_vertex_locked   │
       (async, via settings.auto_inc_rank_when_read)             │
              │                                                  │
              ▼                                                  ▼
         build_touch_entries → IndexUpdate redo log ───────► broadcast to workers
                                                                │
Decay ←─ spawn_rank_decay (background, every period secs)        │
       └── atime_index.range_up_to(threshold)                    │
           └── rank = rank.saturating_sub(1) ◄───────────────────┘
```

## Watch out for
- **Route params**: axum 0.7.9 requires `:param` syntax.
- **Data dir**: `<data_dir>/graphs/<name>/` with files: `data`, `bitmap`, `index`, `config.json`, `redo_*`.
- **Default graph**: `"graph0"` when `?graph=` omitted.
- **POST /vertices**: top-level `name` (String), optional `keywords`, `labels`, `properties`.
- **POST /edges**: requires `source`, `target`, `name` (String). Optional `labels`, `keywords`, `strength` (f32, default 1.0), `properties`.
- **DELETE ?force=true**: hard delete; without force: soft delete.
- **Search step**: takes `text` (raw string), tokenized by jieba-rs.
- **`/gremlin` auto-injects**: `match_mode` from SearchSettings + optionally appends `traverse` step.
- **traverse step**: BFS via score * decay * edge_strength; stops when score < activate.
- **rank step**: source mode iterates rank index descending; filter mode sorts input by rank.
- **Memory index rebuilt at startup** — includes vertices, edges, tokens, ranks, atime_index, adjacency.
- **Lock order**: metadata → block → vertex → edge (enforced by helpers).
- **Properties as JSON strings** inside binary blob (bincode incompatibility).
- **Token strings**: `[u8; 43]` inline — >43 chars truncated.
- **`touch src/ui_serve.rs`** needed after frontend changes.
- **Extraction**: SYSTEM_PROMPT tells LLM to output `name`, `labels`, `keywords`, `properties` for entities; and `source`, `target`, `name`, `labels`, `keywords`, `strength`, `properties` for relations.
- **WAL batch writer**: `append()` sends via `mpsc::channel` to background thread. Caller blocks on Condvar until durability confirmed.
- **SIGINT/SIGTERM**: server calls `GraphManager::close_all()` → flushes dirty blocks + checkpoints all WALs.
- **`Graph::close()`**: calls `flush()` + `sync()` + `renew()`.
- **Cluster mode**: requires `"role": "master"` or `"role": "worker"` in settings. Heartbeat every 5s by default.

## Implemented Plans
- `100-graph-rearch-design.md` — Block-based storage architecture
- `101-graph-rearch-plan.md` — Re-architecture coding plan (Phase 1-8)
- `--- edge-data-structure-update.md` — EdgePayload label→name, +labels

## TODO
1. **前端测试** — 使用 Playwright 对前端交互进行端到端测试，覆盖图可视化、知识库管理、设置面板等核心功能
2. **构建个体自我意识图谱模板** — 设计并实现个体自我意识的知识图谱模板（persona template），包含个性特征、记忆模式、认知偏好等维度
3. **设计个体自我行为机制** — 在 GraphAgent 框架中实现个体基于自身图谱的自主行为决策机制（意图识别 → 目标规划 → 行为执行 → 反馈学习）
4. **构建社会图谱** — 构建多个体间的社会关系图谱，包含信任度、影响力、社交距离等维度
5. **设计社会行为机制** — 实现群体层面的社会行为机制（信息传播、合作博弈、共识形成、社会规范演化）
