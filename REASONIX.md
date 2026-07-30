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
│   ├── mod.rs               # Re-exports 8 submodules
│   ├── types.rs             # Fundamental types, constants, DataHeader, binary layouts
│   ├── data_file.rs         # Raw 16KB block I/O (Mutex<File>)
│   ├── bitmap_file.rs       # Block-level free/used tracking
│   ├── block_allocator.rs   # Chunk-level allocator within a 16KB block
│   ├── block_cache.rs       # LRU cache with dirty tracking (default 4096 blocks = 64MB)
│   ├── redo_log.rs          # WAL: FIFO queue + background batch writer (≤128 entries),
│   │                        #   size (64MB) + time (15min, configurable) rotation,
│   │                        #   async background flush on rotation (刷脏块+删旧WAL),
│   │                        #   CRC32, replay
│   ├── memory_index.rs      # In-memory BTreeMap/HashMap indexes (vertex, edge,
│   │                        #   token, rank, atime, adjacency)
│   └── memory_index_builder.rs  # Rebuild in-memory index by scanning data file at startup
├── lock/                    # Striped RwLock pools for concurrency
│   ├── mod.rs
│   └── lock_manager.rs      # LockManager: metadata → block → vertex → edge
├── graph/                   # Graph engine: CRUD + Gremlin pipeline + tokenizer
│   ├── mod.rs               # Re-exports
│   ├── graph.rs             # Graph struct (facade), GraphConfig, lifecycle
│   ├── graph_registry.rs    # Graph metadata registry (persistent, multi-graph)
│   ├── batch.rs             # Batch import/delete (upsert by name)
│   ├── crud.rs              # Vertex/Edge CRUD with WAL + token extraction + rank
│   ├── gremlin.rs           # Gremlin pipeline step engine (24 steps)
│   ├── locked.rs            # Lock-safe CRUD wrappers
│   ├── serialize.rs         # Bincode serialization with JSON properties
│   ├── tokenizer.rs         # jieba-rs tokenizer, stop-words, min length 2
│   ├── rank_decay.rs        # Periodic rank decay background task
│   └── tests.rs             # #[cfg(test)] integration tests (90+)
├── gremlin/                 # REST API routes + handlers (axum)
│   ├── mod.rs               # AppState, build_router (50+ routes), handlers
│   ├── settings.rs          # GET/PUT /settings/graph/search, /settings/llm, /settings/graph/rank, /settings/web-search
│   ├── tokenizer_settings.rs # Custom tokenizer dictionary words CRUD (GET/POST/DELETE /settings/tokenizer/words)
│   └── indices.rs           # Custom property index management (POST/GET/DELETE /indices/vertex|edge/properties)
├── graph_manager.rs         # Multi-graph manager (HashMap<String, Arc<Graph>>), close_all()
├── documents.rs             # Document CRUD (file storage + JSON index)
├── extract/                 # LLM-based document extraction pipeline
│   ├── mod.rs               # Re-exports
│   ├── config.rs            # ExtractionConfig, ExtractedEntity(name,labels,keywords,properties),
│   │                        #   ExtractedRelation(source,target,name,labels,keywords,strength,properties)
│   ├── document.rs          # Markdown section parser + token budget
│   ├── extraction.rs        # LLM prompt templates (full-field format) + response parsers
│   └── llm_client.rs        # OpenAI-compatible HTTP client with retry
├── task/                    # Generic async task tracking (extraction, future gremlin, etc.)
│   └── mod.rs               # TaskManager, Task, TaskStep, TaskResponse, TaskStatus
├── maas/                    # MaaS OpenAI-compatible proxy
│   ├── mod.rs
│   └── openai.rs            # GET /v1/models + POST /v1/chat/completions (SSE)
├── cluster/                 # Master-worker cluster mode
│   ├── mod.rs
│   ├── server.rs            # Cluster HTTP server (heartbeat/forward/replicate/touch)
│   ├── node.rs              # NodeRegistry (master/worker)
│   ├── forward.rs           # Write forwarding (worker → master)
│   └── replication.rs       # Redo-log replication
├── ui_serve.rs              # Embedded static file serving (rust-embed)

### Examples

```
examples/
├── self_awareness/          # Self-awareness KG pipeline (load/plan/act)
│   ├── cli.py, llm.py, prompts.py, graph_utils.py
│   └── self_soul.md         # Detailed self-description document
└── social_activities/       # Social activities KG pipeline
    ├── cli.py, llm.py, prompts.py, graph_utils.py
    └── social_activities.md # Group social activity document
```
```

### Python SDK

```
sdk/python/
├── pyproject.toml          # Build config (setuptools)
├── SKILL.md                # CLI usage guide
├── bionic_graph/
│   ├── __init__.py         # Client + type exports
│   ├── client.py           # Full REST API client (httpx, pydantic) — CRUD, batch, extraction
│   ├── cli.py              # CLI entry point: bgcli (click, 12 topics: health/graph/batch/vertex/edge/...)
│   ├── models.py           # 18 Pydantic data models
│   └── exceptions.py       # Error classes
└── tests/
    ├── test_client.py      # SDK unit tests
    ├── test_cli.py         # 54 CLI mock tests (all topics, all actions)
    └── test_cli_real.sh    # Real backend CLI integration tests

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
- **SDK install**: `pip install git+https://github.com/agentic-data-ops/bionic-graph.git#subdirectory=sdk/python` (or `cd sdk/python && pip install .`)
- **SDK test**: `cd sdk/python && python3 -m pytest tests/`

## Data Directory Structure

```
<data_dir>/                      (default: "data")
├── graphs/
│   └── <graph_name>/
│       ├── data                — Data file (16KB blocks)
│       ├── bitmap              — Bitmap (block-level free space tracking)
│       ├── config.json         — Per-graph config (storage, lock, indices)
│       └── redo_<yyyymmddHHMMss>_<######>  — WAL files (size + time-based rotation)
├── tokenizer/
│   └── words.json             — Custom dictionary words for jieba-rs tokenizer
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

## Gremlin Steps (24 total)

| Step | Parameters | Description |
|------|-----------|-------------|
| `search` | `text`, `mode?`, `match_mode?`, `limit?`, `min_rank?` | Full-text search via token index. Auto-injects `match_mode` + optional `traverse` step. Time travel via `X-Time-Travel` header. |
| `V` | `ids?` | Vertices by ID |
| `E` | `ids?` | Edges by ID |
| `has` / `hasNot` / `hasKey` / `hasValue` / `hasLabel` / `hasText` | (6 filter steps) | Property/label filters |
| `out` / `in` / `both` | `depth?`, `labels?` | Vertex traversal (BFS) |
| `outE` / `inE` / `bothE` | `labels?` | Edge traversal |
| `values` / `limit` / `count` / `dedup` | — | Result processing |
| `repeat` | `steps`, `times` | Loop sub-pipeline |
| `expand` | `depth?`, `label?` | Add neighbors + edges, optionally filtered by edge label |
| `traverse` | `decay?`, `activate?`, `max_depth?`, `min_score?` | BFS activation spread |
| `rank` | `limit?`, `min?` | Return top results by rank (source or filter step) |

> Time travel is no longer a Gremlin step. Use `X-Time-Travel` HTTP header with a microsecond timestamp instead. The header applies to all steps in the query.

## REST API Endpoints (55 routes)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | System health |
| GET/POST/PUT | `/graphs` | List / create / set-default graph |
| DELETE/PUT | `/graphs/:name` | Delete / update graph metadata |
| GET/PUT | `/graphs/:name/config` | Per-graph storage + indices config |
| POST | `/gremlin` | Gremlin pipeline query |
| GET | `/search` | Token search shortcut |
| POST/PUT/DELETE | `/vertices`, `/vertices/:id` | Vertex CRUD |
| GET/PUT | `/vertices/:id/meta` | Vertex metadata (rank/atime/status) |
| POST/PUT/DELETE | `/edges`, `/edges/:id` | Edge CRUD |
| GET/PUT | `/edges/:id/meta` | Edge metadata |
| GET/PUT | `/settings/graph/search` | Search settings (greedy/exact) |
| GET/PUT | `/settings/graph/rank` | Rank decay config |
| GET/PUT | `/settings/llm` | LLM provider config |
| GET/PUT | `/settings/web-search` | Web search provider config |
| GET | `/settings/tokenizer/words` | Tokenizer custom dictionary words (list) |
| POST/DELETE | `/settings/tokenizer/words` | Add / remove custom tokenizer words |
| POST/GET/DELETE | `/indices/vertex/properties` | Register / list / unregister vertex property index keys |
| GET/DELETE | `/indices/vertex/properties/:key` | Show stats / unregister a specific vertex property key |
| POST/GET/DELETE | `/indices/edge/properties` | Register / list / unregister edge property index keys |
| GET/DELETE | `/indices/edge/properties/:key` | Show stats / unregister a specific edge property key |
| GET/POST | `/documents` | List / create documents |
| GET/PUT/DELETE | `/documents/:id` | Get / update / delete document metadata |
| GET | `/documents/:id/content` | Document body |
| POST | `/extract` | Submit extraction task |
| POST | `/documents/:id/extract` | Extract from document by ID |
| POST | `/extract` | Submit extraction task |
| POST | `/documents/:id/extract` | Extract from document by ID |
| GET | `/tasks/:task_id` | Task polling |
| GET | `/tasks` | List tasks |
| POST | `/batch/load` | Batch import vertices/edges (upsert by name) |
| POST | `/batch/delete` | Batch delete vertices/edges by name |
| GET | `/proxy/openai/v1/models` | Model listing |
| POST | `/proxy/openai/v1/chat/completions` | Chat proxy (SSE) |
| POST | `/proxy/web-search` | Web search proxy |

> Graph selection via `X-Graph-Name` header (all CRUD + Gremlin + search + batch + document endpoints).
> No `?graph=` query parameter support. Default graph: `"graph0"` when header omitted.

## WebSearchConfig

Settings under `"web_search"` key in settings.json:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `default_provider` | string | `"Baidu"` | Default search provider name |
| `providers` | array | — | List of search providers |

### WebSearchProvider

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | — | Provider name (used as unique identifier) |
| `search_url` | string | — | URL template with `{text}` for query |
| `method` | string | `"GET"` | HTTP method (GET/POST) |
| `body_template` | string? | null | JSON body for POST, `{text}` replaced |
| `params` | object | `{}` | Query parameters |
| `headers` | object | `{}` | HTTP headers (e.g. Authorization) |

## Python SDK & CLI

```bash
# Install from GitHub
pip install git+https://github.com/agentic-data-ops/bionic-graph.git#subdirectory=sdk/python

# CLI: bgcli <topic> <action> [options]
bgcli health check
bgcli vertex create --name "Eddard Stark" --labels '["person"]'
bgcli search --text "Stark"                              # Full-text search
bgcli gremlin execute --steps '[{"step":"V","ids":[1]}]' # Gremlin pipeline
bgcli document extract d1                                 # Background extraction
bgcli task list                                           # Async tasks
bgcli task get --task-id t1                                # Task status
bgcli task wait --task-id t1                               # Wait for task
bgcli proxy web-search --query "AI" --provider "Baidu"           # Web search
bgcli proxy openai-models                                  # List LLM models
bgcli proxy openai-chat --messages '...'                   # LLM chat

# Interactive chat with web + graph search
bgcli chat --model "DeepSeek/deepseek-v4-flash" \
           --web-search --graph-search
```

```python
from bionic_graph import Client
client = Client()
health = client.health()
print(health.status)
```

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
| `vertices` | BTreeMap<u32, MetaPointer> | Vertex ID → data file pointer |
| `edges` | BTreeMap<u32, MetaPointer> | Edge ID → data file pointer |
| `tokens` | BTreeMap<String, Vec<MetaPointer>> | Token string → pointers (prefix search) |
| `ranks` | BTreeMap<u32, Vec<MetaPointer>> | Rank → pointers (descending order for hot queries) |
| `atime_index` | BTreeMap<u64, Vec<MetaPointer>> | Atime → pointers (range scan for inactivity decay) |
| `adjacency` | HashMap | Vertex → outgoing/incoming edges |
| `entity_tokens` | HashMap<(u8, u32), Vec<String>> | Entity → token strings (for hard delete cleanup) |
| `vertex_names` | BTreeMap<String, u32> | Name → vertex ID |
| `edge_names` | BTreeMap<String, u32> | Name → edge ID |
| `vertex_label` | BTreeMap<String, Vec<MetaPointer>> | Label → vertex pointers |
| `edge_label` | BTreeMap<String, Vec<MetaPointer>> | Label → edge pointers |
| `vertex_properties` | HashMap<String, BTreeMap<String, Vec<MetaPointer>>> | Custom property key → value → vertex pointers (opt-in per key) |
| `edge_properties` | HashMap<String, BTreeMap<String, Vec<MetaPointer>>> | Custom property key → value → edge pointers (opt-in per key) |

## Cluster Architecture

```
┌─────────┐     ┌─────────┐     ┌─────────┐
│ Worker 1│     │ Master  │     │ Worker 2│
│ (R)     │◄────│(R+W)    │────►│ (R)     │
└────┬────┘     └─────────┘     └────┬────┘
     │ ① writes forwarded           │
     └─────────── to master ─────────┘
                 │
     ┌───────────┴───────────┐
     │ ② master broadcasts   │
     │    entries via HTTP    │
     │    to ALL workers      │
     └───────────────────────┘

Touch（rank/atime 读取更新）：
┌─────────┐                     ┌─────────┐     ┌─────────┐
│ Worker 1│ ──── POST ──────►   │ Master  │────►│ Worker 2│
│ (读 V)  │   /cluster/touch    │ relay   │     │ apply   │
└─────────┘                     └─────────┘     └─────────┘
   ↑                                  ↑
   本地 DataHeader 更新    本地 DataHeader 更新
```

**Cluster endpoints** (on cluster bind_addr):
| Method | Path | Direction | Description |
|--------|------|-----------|-------------|
| POST | `/cluster/heartbeat` | Worker → Master | Worker registration + heartbeat |
| POST | `/cluster/forward` | Worker → Master | Forwarded write request |
| POST | `/cluster/replicate` | Master → Worker | Redo log entry push |
| POST | `/cluster/touch` | 双向 | Read报告（Worker→Master）+ 中继广播（Master→所有Worker），直接HTTP无WAL |
| POST | `/cluster/execute` | Master→Worker | 通过 proxy_to_api 在 worker 上执行转发请求（写广播），X-Bionic-Request-Id Header 防止递归 |
| POST | `/cluster/tokenizer-sync` | Master→Worker | 同步 tokenizer 自定义词到所有 worker |

**Node ID 唯一性要求**: worker 的 heartbeat `node_id` 必须唯一（源码中用 `format!("worker@{}", cluster.bind_addr)` 生成）。两个 worker 使用相同 `node_id` 会导致 master 的 `HashMap` 中后注册者覆盖前者，广播只能到达一个 worker。

## Rank Lifecycle

```
Update → update_vertex/edge: rank += 1, atime = now
         WAL: OpType::VertexUpdate / EdgeUpdate (full payload)

Read (implicit touch) → update_rank_and_atime
    via settings.auto_inc_rank_when_read
         ├── DataHeader in-place update (block cache, mark dirty)
         └── WAL: OpType::VertexMetaUpdate / EdgeMetaUpdate (rank, atime)

Explicit PUT meta → update_vertex_meta / update_edge_meta
         ├── DataHeader in-place update
         └── WAL: OpType::VertexMetaUpdate / EdgeMetaUpdate (rank, atime)

Decay → spawn_rank_decay (background, every period secs)
         ├── atime_index.range_up_to(threshold)
         ├── rank = rank.saturating_sub(1)
         ├── update_header_in_place (DataHeader dirty)
         └── WAL: OpType::VertexMetaUpdate / EdgeMetaUpdate (rank, atime)

Cluster touch broadcast (read报告 + 中继):
         Worker→Master POST /cluster/touch (直接HTTP，无WAL)
         Master→所有Worker 中继广播 TouchRequest (直接HTTP，无WAL)

Periodic flush:
         └── block_cache 脏块被以下机制刷盘:
              ├── LRU 驱逐 (shard 满时自动)
              ├── Graph::flush() (手动/close时)
              └── WAL rotation: 后台线程 flush_dirty() → 刷脏块 → 删旧WAL
```

## Cluster Broadcast Replay Prevention (请求 ID 方案)

全局 REPLAYING 标记已被**请求 ID + 中间件 + task-local** 方案取代：

```
handle_execute (cluster server)
  ├─ 生成唯一 req_id = Uuid::new_v4()
  ├─ 注册到 INFLIGHT_REQUESTS (LazyLock<Mutex<HashSet<String>>>)
  └─ proxy_to_api(api_addr, req, Some(&req_id))
       └─ HTTP 请求添加 Header: X-Bionic-Request-Id: <uuid>
            └─ REST API 处理
                 ├─ axum middleware 检查 Header
                 │    └─ 在 INFLIGHT_REQUESTS 中？→ IS_BROADCAST_REPLAY = true (task-local)
                 ├─ try_forward_json → is_broadcast_replay() → true → 跳过转发
                 └─ 正常 handler 处理 (WAL 写入照常)
```

**并发安全原理**：
- 每个广播请求唯一 req_id → 不同请求 ID 不同 → 互不干扰
- `INFLIGHT_REQUESTS` (全局 `HashSet`) 只跟踪正在处理中的广播请求
- 中间件通过 `tokio::task_local!` 设置 `IS_BROADCAST_REPLAY` → 只影响当前请求
- 普通请求没有 `X-Bionic-Request-Id` Header → middleware 不设标志 → 正常转发
- WAL replay 用独立的 `WAL_REPLAYING` (全局 AtomicBool)，HTTP 启动前已结束

**写广播流程**：
```
Master handler (create_vertex, create_document 等)
  └─ broadcast_request_to_workers()
       └─ POST /cluster/execute → 每个 worker
            └─ handle_execute
                 ├─ 注册 req_id → proxy_to_api → X-Bionic-Request-Id Header
                 ├─ worker REST API 处理 (通过 middleware IS_BROADCAST_REPLAY=true)
                 │    ├─ try_forward_json → 跳过 (不转发回 master)
                 │    ├─ broadcast_* → 跳过 (不再次广播)
                 │    └─ 正常执行 + WAL 写入
                 └─ 注销 req_id
```

**读转发绕过**：
- `try_forward_read_json()` 不检查 `is_broadcast_replay()` → 任务轮询等读请求不受 REPLAYING 影响
- 用于 `GET /tasks/:task_id` 和 `GET /tasks`
```

## Watch out for
- **Route params**: axum 0.7.9 requires `:param` syntax.
- **Data dir**: `<data_dir>/graphs/<name>/` with files: `data`, `bitmap`, `config.json`, `redo_*`. No separate index file — metadata embedded in DataHeader.
- **Tokenizer custom words**: stored at `<data_dir>/tokenizer/words.json`. Loaded at startup via `tokenizer::set_data_dir()`. Can be modified at runtime via `GET/POST/DELETE /settings/tokenizer/words`.
- **Default graph**: `"graph0"` when `?graph=` omitted.
- **POST /vertices**: top-level `name` (String), optional `keywords`, `labels`, `properties`. Properties must be flat (no nested dicts, arrays of strings/numbers/booleans only).
- **POST /edges**: requires `source`, `target`, `name` (String). Optional `labels`, `keywords`, `strength` (f32, default 1.0), `properties`.
- **DELETE ?force=true**: hard delete; without force: soft delete.
- **Search step**: takes `text` (raw string), tokenized by jieba-rs.
- **`/gremlin` auto-injects**: `match_mode` from SearchSettings + optionally appends `traverse` step.
- **Time travel**: via `X-Time-Travel` HTTP header (microsecond timestamp). Applies to all Gremlin steps and search. No longer a dedicated step.
- **traverse step**: BFS via score * decay * edge_strength; stops when score < activate.
- **rank step**: source mode iterates rank index descending; filter mode sorts input by rank.
- **Memory index rebuilt at startup** — scans data file blocks (bitmap → DataHeader → payload), populates vertices, edges, tokens, ranks, atime_index, adjacency. When `config.json` has `indices.vertex_properties` / `edge_properties` keys, the builder also populates the custom property index during this single scan (no second pass needed).
- **Custom property indices**: opt-in per key via `POST /indices/vertex|edge/properties`. Each key stores a `HashMap<key, BTreeMap<value, Vec<MetaPointer>>>` for fast property-based lookups. Registered keys are persisted in `config.json` under the `indices` key. After a crash + restart, `build_memory_index()` re-indexes registered properties during the data file scan — no manual re-registration required.
- **PUT /graphs/:name/config syncs indices**: when the `indices.vertex_properties` or `indices.edge_properties` sections of the per-graph config change, the in-memory property index is automatically synchronized — new keys are registered and populated by scanning existing entities, while removed keys are unregistered and their index data is dropped.
- **Lock order**: metadata → block → vertex → edge (enforced by helpers).
- **Properties as JSON strings** inside binary blob (bincode incompatibility).
- **`touch src/ui_serve.rs`** needed after frontend changes.
- **Extraction**: uses `crate::graph::batch::batch_import()` internally — upserts vertices by name, edges by (source_name, target_name, name). SYSTEM_PROMPT tells LLM to output `name`, `labels`, `keywords`, `properties` for entities; and `source`, `target`, `name`, `labels`, `keywords`, `strength`, `properties` for relations.
- **WAL batch writer**: `append()` encodes the entry and sends via `mpsc::channel` to background thread. Caller blocks on Condvar until the writer commits the batch and advances epoch.
- **WAL rotation**: Size (default 64MB) or time (default 900s) triggers rotation. Writer creates new WAL file, syncs old one, spawns **background thread** to flush `block_cache` dirty blocks via `flush_dirty()`, then deletes old WAL file. Writer continues processing new entries immediately (not blocked by flush).
- **Rank/Atime WAL**: All rank/atime update paths (`update_rank_and_atime`, `update_vertex_meta`, `update_edge_meta`, `rank_decay`) now write WAL entries (`OpType::VertexMetaUpdate` / `EdgeMetaUpdate`, bincode-serialized `(rank, atime)`). Crash consistency is guaranteed via WAL replay on startup.
- **SIGINT/SIGTERM**: server calls `GraphManager::close_all()` → flushes dirty blocks + syncs + renews WAL.
- **`Graph::close()`**: calls `flush()` + `sync()` + `renew()`.
- **Cluster mode**: requires `"role": "master"` or `"role": "worker"` in settings. Heartbeat every 5s by default. **Worker `node_id` 必须唯一**：多个 worker 必须使用不同的 node_id（源码生成 `worker@{bind_addr}`），否则 master 的 HashMap 中后注册者覆盖前者。
- **Replay prevention**: 使用 `X-Bionic-Request-Id` header + `INFLIGHT_REQUESTS` set + axum middleware + `tokio::task_local!` `IS_BROADCAST_REPLAY`。无需全局 `REPLAYING` 标志，并发安全。
- **Read forwarding bypass**: `try_forward_read_json()` 不检查 `is_broadcast_replay()`，用于任务轮询等读操作，不受广播 replay 影响。
- **Document broadcast with same UUID**: `CreateDocumentBody` 支持可选 `id` 字段，广播时携带 master UUID，workers 在 REPLAYING 模式下使用指定 ID 创建。`UpdateDocumentBody` 支持可选 `graph_name` 字段。
- **Document delete ?clean**: 后端解析 `?clean=true` 查询参数，控制是否清理关联图谱数据，集群转发和广播时携带该参数。
- **broadcast_request_to_workers**: 新增 `query: Option<&str>` 参数，支持广播时传递查询参数。
- **Query parameter faithful pass-through**: 所有转发和广播的查询参数（`?force`, `?clean`）均**忠实于原始请求**传递，不做默认值推断。`params.force == Some(true)` 时广播 `?force=true`，`Some(false)` 时广播 `?force=false`，`None` 时不加。`delete_document` 的 `clean_query` 使用 `params.clean.map(|c| format!("clean={}", c))` 而非 `unwrap_or(false)` 隐含默认值。
- **Touch broadcast**: 读取触发 rank/atime 更新时，Worker→Master 报告 + Master 中继广播到所有 Worker。直接 HTTP POST `TouchRequest{vertex_ids, edge_ids}` 到各节点的 `/cluster/touch`，**不走 WAL**。
- **Document lifecycle**: created without graph association. Graph assigned during extraction via `X-Graph-Name` header.
- **Batch API**: `/batch/load` upserts vertices by `name`, edges by `(source_name, target_name, name)`. `update_existing` (default true) controls upsert vs append. `/batch/delete` cascades to connected edges.
- **ID isolation**: each graph has independent ID space. Counters computed from index max at startup (no longer in config.json).
- **Graph name resolution**: via `X-Graph-Name` header on all CRUD/Gremlin/search/batch/document endpoints. No `?graph=` query parameter.

## TODO
- [x] 顶点和边被读取到时更新atime和rank元数据：Worker→Master报告，Master中继广播到所有Worker（直接HTTP，无WAL）
- [x] 修复 worker node_id 冲突 bug：两个 worker 硬编码 "worker" 导致 HashMap 覆盖，改为 `worker@{bind_addr}`
- [x] 检查是否仍然有写操作未进行广播 — 所有写操作均已覆盖
- [x] 将tokenizer自定义词典配置文件迁移到数据目录：tokenizer/words.json
- [x] 使用 master.json, worker1.json, worker2.json 启动集群测式
- [x] 测试worker1写入，worker2读取，覆盖所有涉及转发和广播的场景
- [x] 验证master, worker1的前端是否正常
- [x] 刷新代码注释，涉及广播的场景（touch/read），不再使用WAL日志了
- [x] 刷新REASONIX.md 和 README.md
- [ ] master将连接的节点信息进行持久化: cluster/nodes.json
- [ ] worker首次连接到master时检查每个图库的配置与master是否一致，如果不一致，报错退出，不允许加入集群
- [ ] 如果worker离线，master将未被成功处理的节点广播请求持久化到数据目录下的文件：cluster/broadcast.bin，并在节点状态正常时进行重试
- [x] 自定义索引的配置进行持久化，保存到图库的config.json中（indecies.properties），如果数据库崩溃，则需要重配置文件中加载自定义索引配置，并扫描数据文件进行重建
- [x] 检查下顶点和边的元数据更新有没有记录日志，能否保证崩溃一致性 — 已全部添加 WAL（VertexMetaUpdate / EdgeMetaUpdate），SIGKILL 测试通过
- [x] 修复log checkpoint刷盘机制 — log rotation 时后台线程异步刷脏块，不阻塞 writer
- [x] GET 方法支持在 proxy_to_api 中转发（任务轮询）
- [x] proxy_to_api 添加 X-Bionic-Request-Id header 传递广播上下文
- [x] axum middleware 检测 replay header 并设置 task-local IS_BROADCAST_REPLAY
- [x] try_forward_read_json 绕过 replay 检查用于读转发
- [x] 文档生命周期集群同步（创建/更新/提取/删除含 clean 参数）
- [x] 软/硬删除广播路径参数传递 — delete_vertex/delete_edge/delete_document 的 query 参数（force/clean）忠实地按原始请求传递，不做默认值推断
- [x] submit_extraction/extract_document_handler 转发补充 graph_name 参数
- [x] InfoPanel saveEdit 的 setUpdateSuccess 作用域修复（顶层函数无法访问父组件 useState）
- [x] onDataChange 改为 (items, msgId) 按消息ID直接定位，不再遍历匹配/去重合并
- [x] formatGraphContext 增强：顶点含 id/name/labels/keywords/properties；边含 id/name/sourceName/targetName/sourceId/targetId/strength/labels/keywords/properties
- [x] GraphViewer 容器 div 始终渲染（不再条件判断），vis-network 生命周期修复
- [x] 前端添加顶点/边的持久化和 UI 刷新修复
