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
│   │                        #   checkpoint protocol, CRC32, replay
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
    ├── test_cli.py         # 57 CLI mock tests (all topics, all actions)
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
| GET | `/proxy/openai/v1/models` | Model listing |
| POST | `/proxy/openai/v1/chat/completions` | Chat proxy (SSE) |
| POST | `/proxy/web-search` | Web search proxy |
| POST | `/extract` | Submit extraction task |
| POST | `/documents/:id/extract` | Extract from document by ID |
| GET | `/tasks/:task_id` | Task polling |
| GET | `/tasks` | List tasks |
| POST | `/batch/load` | Batch import vertices/edges (upsert by name) |
| POST | `/batch/delete` | Batch delete vertices/edges by name |

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
Update → update_vertex/edge: rank += 1, atime = now
         DataHeader updated in-place (block cache, mark dirty) ──┐
                                                                 │
Read → execute_gremlin → process_touch ───► get_vertex_locked    │
       (async, via settings.auto_inc_rank_when_read)              │
              │                                                   │
              ▼                                                   │
         update_rank_and_atime → in-place DataHeader chunk update │
              │                      (no WAL entry needed)        │
              ▼                                                   │
         broadcast touch to workers (cluster mode)               │
                                                                  ▼
Decay ←─ spawn_rank_decay (background, every period secs)   checkpoint flushes
       └── atime_index.range_up_to(threshold)               dirty blocks to disk
           └── rank = rank.saturating_sub(1)
               └── update_header_in_place (no WAL)
```

## Watch out for
- **Route params**: axum 0.7.9 requires `:param` syntax.
- **Data dir**: `<data_dir>/graphs/<name>/` with files: `data`, `bitmap`, `config.json`, `redo_*`. No separate index file — metadata embedded in DataHeader.
- **Default graph**: `"graph0"` when `?graph=` omitted.
- **POST /vertices**: top-level `name` (String), optional `keywords`, `labels`, `properties`. Properties must be flat (no nested dicts, arrays of strings/numbers/booleans only).
- **POST /edges**: requires `source`, `target`, `name` (String). Optional `labels`, `keywords`, `strength` (f32, default 1.0), `properties`.
- **DELETE ?force=true**: hard delete; without force: soft delete.
- **Search step**: takes `text` (raw string), tokenized by jieba-rs.
- **`/gremlin` auto-injects**: `match_mode` from SearchSettings + optionally appends `traverse` step.
- **Time travel**: via `X-Time-Travel` HTTP header (microsecond timestamp). Applies to all Gremlin steps and search. No longer a dedicated step.
- **traverse step**: BFS via score * decay * edge_strength; stops when score < activate.
- **rank step**: source mode iterates rank index descending; filter mode sorts input by rank.
- **Memory index rebuilt at startup** — scans data file blocks (bitmap → DataHeader → payload), populates vertices, edges, tokens, ranks, atime_index, adjacency.
- **Lock order**: metadata → block → vertex → edge (enforced by helpers).
- **Properties as JSON strings** inside binary blob (bincode incompatibility).
- **`touch src/ui_serve.rs`** needed after frontend changes.
- **Extraction**: uses `crate::graph::batch::batch_import()` internally — upserts vertices by name, edges by (source_name, target_name, name). SYSTEM_PROMPT tells LLM to output `name`, `labels`, `keywords`, `properties` for entities; and `source`, `target`, `name`, `labels`, `keywords`, `strength`, `properties` for relations.
- **WAL batch writer**: `append()` sends via `mpsc::channel` to background thread. Caller blocks on Condvar until durability confirmed.
- **SIGINT/SIGTERM**: server calls `GraphManager::close_all()` → flushes dirty blocks + checkpoints all WALs.
- **`Graph::close()`**: calls `flush()` + `sync()` + `renew()`.
- **Cluster mode**: requires `"role": "master"` or `"role": "worker"` in settings. Heartbeat every 5s by default.
- **Document lifecycle**: created without graph association. Graph assigned during extraction via `X-Graph-Name` header.
- **Batch API**: `/batch/load` upserts vertices by `name`, edges by `(source_name, target_name, name)`. `update_existing` (default true) controls upsert vs append. `/batch/delete` cascades to connected edges.
- **ID isolation**: each graph has independent ID space. Counters computed from index max at startup (no longer in config.json).
- **Graph name resolution**: via `X-Graph-Name` header on all CRUD/Gremlin/search/batch/document endpoints. No `?graph=` query parameter.

## Implemented Plans
- `100-graph-rearch-design.md` — Block-based storage architecture
- `101-graph-rearch-plan.md` — Re-architecture coding plan (Phase 1-8)
- `--- edge-data-structure-update.md` — EdgePayload label→name, +labels
- `201-sdk-python-test-plan.md` — Python SDK CLI full test coverage (57 tests)
- `--- task-module.md` — Generic task module extracted from extraction pipeline
- `--- proxy-api-restructure.md` — Unified `/proxy/*` API paths, CLI theme restructure
- `300-self-awareness-plan.md` — Self-awareness KG Python CLI pipeline
- `301-example-social-activity-plan.md` — Social activities KG Python CLI pipeline

## TODO
1. **前端测试** — 使用 Playwright 对前端交互进行端到端测试
2. **构建个体自我意识图谱模板** — 设计并实现个体自我意识的知识图谱模板（已完成示例）
3. **设计个体自我行为机制** — 在 GraphAgent 框架中实现个体自主行为决策机制
4. **构建社会图谱** — 构建多个体间的社会关系图谱（已完成示例）
5. **设计社会行为机制** — 实现群体层面的社会行为机制
