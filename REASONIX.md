# Bionic-Graph — REASONIX.md

## Stack
- **Language**: Rust 2021 edition
- **Web framework**: axum 0.7 (REST API, JSON, CORS) — uses `:param` path syntax
- **Serialization**: serde + serde_json + bincode (binary persistence)
- **CLI**: clap 4 (derive)
- **Async runtime**: tokio (full)
- **Config**: `~/.config/bionic-graph/settings.json`, auto-generated on first run
- **Frontend**: React 19 + Vite 8 + Tailwind CSS 4 + vis-network (Canvas 2D)
- **Frontend embedding**: rust-embed (compile-time embedding into Rust binary)

## Layout
- `src/graph/` — Vertex/Edge/Graph types, MVCC versioning, BFS/DFS traversal
- `src/neuron/` — Spreading activation network, Hebbian learning, `EntityType` (Vertex/Edge per neuron)
- `src/storage/` — Disk-backed storage: subgraph partitioning, LRU cache, WAL (redo_log), version log (vlog), compaction
- `src/gremlin/` — REST API routes + Gremlin JSON pipeline step engine (15 steps)
- `src/extract/` — Backend document extraction (legacy; extraction moved to frontend)
  - `task_manager.rs` — Async task lifecycle (pending → running → completed/failed), UUID-based task tracking with progress
- `src/config/` — Settings struct (serde) + loader with env override
- `src/persistence/` — graph_store/neuron_store serialization + auto-save thread
- `src/graph_manager.rs` — Multi-graph manager (HashMap<String, GraphHandle>)
- `src/documents.rs` — Document management CRUD (file storage + JSON index)
- `src/ui_serve.rs` — Embedded static file serving (rust-embed)
- `src/memory_system.rs` — Legacy single-graph wrapper (backward compat)
- `src/ui/` — React frontend (Vite + Tailwind + vis-network)

## Commands
- **build**: `cargo build` (also runs `npm --prefix src/ui run build` before Rust compile)
- **release**: `cargo build --release`
- **test**: `cargo test` (Rust unit tests) + `npm --prefix src/ui run test` (frontend tests)
- **run**: `cargo run` → serves both API + frontend at `http://127.0.0.1:8080`
- **frontend dev**: `npm --prefix src/ui run dev` (standalone Vite, proxies API to port 8080)
- **frontend build**: `npm --prefix src/ui run build`
- **frontend test**: `npm --prefix src/ui run test`

## Frontend Architecture

### Stack
- React 19, Vite 8, Tailwind CSS 4
- `vis-network` + `vis-data` (Canvas 2D graph visualization)
- `i18next` (i18n EN/ZH)
- All LLM calls (chat, semantic search, document extraction) are frontend-side

### Layout
```
App.jsx
├── Sidebar.jsx          — 对话列表 + 知识库入口 + 设置入口
├── ChatArea.jsx         — 聊天主区域
│   ├── MessageList.jsx  — 消息列表 (用户/助手/搜索进度/图谱结果)
│   └── ChatInput.jsx    — 输入框 + 模型选择 + 图谱开关 + 搜索模式切换
├── KnowledgeBase.jsx    — 知识库弹窗 (文件管理 + LLM 提取)
└── SettingsDialog.jsx   — 设置弹窗 (供应商/图库/通用)
```

### Conversation Flow
- **LLM Chat**: User input → `chatCompletion()` (SSE streaming) → streaming display
- **Keyword Search**: User input → split keywords → `graphSearch` → graph result
- **Semantic Search**: User input → LLM extract keywords → `graphSearch` → LLM filter results → graph result
- **Document Extraction**: Markdown file → LLM generate title/tags → LLM extract entities/relations → `POST /vertices` + `POST /edges`

### Data Persistence
- Conversations → `localStorage('bgraph-convs')`
- Settings (providers, graphs, search mode) → `localStorage('bgraph-settings')`
- Documents → Backend `data/documents/` (files + JSON index)
- Graph data → Backend `data/` (graph.bin + neural.bin)

## Gremlin Steps (15 total)
| Step | Description |
|------|-------------|
| `search` | Neural index search — returns vertices from matched/activated neurons (inactive filtered out). Capped at 100 results. |
| `V` / `E` | All or specific vertices / edges |
| `has` / `hasNot` / `hasKey` / `hasValue` / `hasLabel` / `hasText` | Property filters |
| `out` / `in` / `both` | Vertex traversal (supports depth) |
| `outE` / `inE` / `bothE` | Edge traversal (returns EdgeResult) |
| `values` / `limit` / `count` / `dedup` | Result processing |
| `repeat` | Loop sub-steps N times |
| `timeTravel` | Point-in-time query |
| `compact` | Archive old history to vlog |

## REST API Endpoints
| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | System health + aggregate stats |
| GET/POST/DELETE | `/graphs` | List / create / delete graphs |
| POST | `/gremlin` | Gremlin pipeline query |
| POST | `/search` | Neural keyword search |
| POST | `/vertices`, `/edges` | Add vertex/edge (auto-creates neurons) |
| DELETE | `/vertices/:id` | Delete vertex + connected edges |
| POST | `/neurons`, `/neurons/:id/link`, `/neurons/:id/synapse` | Neural network management |
| POST | `/extract` | Submit async extraction (legacy, backend-side) |
| GET | `/extract/task/:task_id` | Poll extraction task |
| GET | `/extract/tasks` | List extraction tasks |
| POST | `/compact` | History compaction |
| POST | `/reindex` | Re-index edges into neural network |
| GET/POST/PUT/DELETE | `/documents` | Document CRUD |
| GET | `/documents/:id/content` | Document content |

## Watch out for
- **`edit_file` SEARCH must match byte-for-byte** — the Rust source has no trailing whitespace convention, and SEARCH is whitespace-sensitive.
- **`.` — `.reasonix/` is committed** — plans in `.reasonix/plans/` and outputs in `.reasonix/output/` are part of the repo.
- **`Vertex::update_properties(props, record_history)`** — second boolean param controls whether the old state is pushed to `_history`. Call sites must pass the graph's `time_travel_enabled` flag.
- **`Graph::remove_vertex(id, force)`** — when `force=false` and `time_travel_enabled=true`, performs soft-delete. Otherwise hard-delete.
- **`POST /vertices` and `POST /edges` auto-create neurons** — HTTP handlers call `Neuron::for_vertex` / `Neuron::for_edge` + `auto_synapse`.
- **Route params use `:param` syntax** — axum 0.7.9 requires `:param` (not `{param}`) for path parameters in `.route()`.
- **`search` step filters inactive neurons** — `activation.rs` only collects vertex refs from neurons with `activation > 0`.
- **`cargo build` needs `touch src/ui_serve.rs` after frontend changes** — rust-embed doesn't detect `src/ui/dist/` file changes for recompilation.
- **`semanticSearch` removed from backend** — all semantic search logic now runs on the frontend (LLM calls + graphSearch).
- **`graph_result` message type deprecated** — search results are now stored as `search_progress` messages with `graphData` field.
- **GraphViewer uses vis-network** — Canvas 2D, no WebGL required. Nodes/edges stored in DataSet with `_original` field for full data preservation.
- **Maximize uses dual GraphViewer instances** — inline card and fullscreen overlay share data via `getSnapshot()` / `applySnapshot()` pattern.

## Implemented Plans
- `001-arch-verify.md` — Full feature verification (151 tests, 0 failed)
- `002-section-paragraph-graph.md` — Section/paragraph graph structure
- `003-keyword-semantic-search.md` — keywordSearch + semanticSearch + global LLM config
- `005-ui-rewrite-knowledgebase-visnetwork.md` — Frontend rewrite + knowledge base + vis-network migration
