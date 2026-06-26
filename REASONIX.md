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
- `src/storage/` — Disk-backed storage: subgraph partitioning, LRU cache (`SubgraphCache`), WAL (`RedologWal` + `RedoLog`), `DiskGraph` (incremental persistence with on-demand subgraph loading via `SubgraphCache`), version log (vlog), compaction
- `src/gremlin/` — REST API routes + Gremlin JSON pipeline step engine (15 steps)
- `src/maas/` — MaaS (Model as a Service) OpenAI-compatible proxy: model listing + chat completions
- `src/extract/` — Backend document extraction
  - `document_extractor.rs` — LLM-based entity/relation extraction with auto-split, dedup, GraphManager API
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
- **frontend e2e**: `node src/ui/test/e2e/<name>.mjs` (Playwright, starts Vite dev server first)

## Frontend Architecture

### Stack
- React 19, Vite 8, Tailwind CSS 4
- `vis-network` + `vis-data` (Canvas 2D graph visualization)
- `i18next` (i18n EN/ZH)
- All LLM calls go through backend MaaS proxy (`/maas/openai/v1/chat/completions`), which forwards to configured providers using stored API keys

### Layout
```
App.jsx
├── Sidebar.jsx          — 对话列表 + 知识库入口 + 图库入口 + 设置入口
├── ChatArea.jsx         — 聊天主区域
│   ├── MessageList.jsx  — 消息列表 (用户/助手/搜索进度/图谱结果)
│   └── ChatInput.jsx    — 输入框 + 模型选择 + 图谱/时间旅行/搜索模式控制栏
├── KnowledgeBase.jsx    — 知识库弹窗 (文件管理 + LLM 提取)
├── GraphManagerDialog.jsx — 图库管理弹窗 (创建/删除/归档/默认)
└── SettingsDialog.jsx   — 设置弹窗 (供应商/搜索/通用)
```

### Conversation Flow
- **LLM Chat**: User input → `chatCompletionProxy()` (SSE streaming via MaaS proxy) → streaming display
- **Keyword Search**: User input → split keywords → `graphSearch` → graph result
- **Semantic Search**: User input → LLM extract keywords (via MaaS proxy) → `graphSearch` → LLM filter results (via MaaS proxy) → graph result
- **Document Extraction**: Markdown file → LLM generate title/tags → LLM extract entities/relations → `POST /vertices` + `POST /edges`

### Data Persistence
- Conversations → `localStorage('bgraph-convs')`
- Settings (providers, graphs, search mode, chatModel) → `localStorage('bgraph-settings')`
- Documents → Backend `data/documents/YYMMDD/<id>.md` + `index.json`
- Graph data → Backend `data/graphs/<name>/` (graph.bin + neural.bin + redolog.wal)

## Gremlin Steps (17 total)
| Step | Description |
|------|-------------|
| `search` | Neural index search — returns vertices from matched/activated neurons. Supports `mode: "greedy"` (match ANY keyword) or `"exact"` (match ALL keywords). Optional `at` (Unix μs) for time-travel aware search. Default greedy. Capped at 100 results. |
| `V` / `E` | All or specific vertices / edges |
| `has` / `hasNot` / `hasKey` / `hasValue` / `hasLabel` / `hasText` | Property filters |
| `out` / `in` / `both` | Vertex traversal (supports depth) |
| `outE` / `inE` / `bothE` | Edge traversal (returns EdgeResult) |
| `values` / `limit` / `count` / `dedup` | Result processing |
| `repeat` | Loop sub-steps N times |
| `timeTravel` | Point-in-time query |
| `compact` | Archive old history to vlog |
| `expand` | Expand vertex: returns neighbor vertices + connected edges in one step |

## REST API Endpoints
| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | System health + aggregate stats |
| GET/POST/DELETE | `/graphs` | List / create / delete graphs |
| POST | `/gremlin` | Gremlin pipeline query |
| POST | `/search` | Neural keyword search |
| POST | `/vertices`, `/edges` | Add vertex/edge (auto-creates neurons) |
| PUT | `/vertices/:id` | Update vertex name/keywords/labels/properties |
| PUT | `/edges/:id` | Update edge label/properties |
| DELETE | `/vertices/:id` | Delete vertex + connected edges (supports `?force=true`) |
| DELETE | `/edges/:id` | Delete edge (supports `?force=true`) |
| POST | `/neurons`, `/neurons/:id/link`, `/neurons/:id/synapse` | Neural network management |
| POST | `/extract` | Submit async extraction (legacy, backend-side) |
| GET | `/extract/task/:task_id` | Poll extraction task |
| GET | `/extract/tasks` | List extraction tasks |
| POST | `/compact` | History compaction |
| POST | `/reindex` | Re-index edges into neural network |
| GET/PUT | `/settings` | LLM providers config |
| GET/PUT | `/settings/neural` | Neural activation/search/learn config |
| GET/POST/PUT/DELETE | `/documents` | Document CRUD |
| GET | `/documents/:id/content` | Document content |
| GET | `/maas/openai/v1/models` | List available models (format `provider/model`) with `x-default-model` header |
| POST | `/maas/openai/v1/chat/completions` | OpenAI-compatible chat completion proxy (supports streaming SSE) |

## Watch out for
- **`edit_file` SEARCH must match byte-for-byte** — the Rust source has no trailing whitespace convention, and SEARCH is whitespace-sensitive.
- **`.` — `.reasonix/` is committed** — plans in `.reasonix/plans/` and outputs in `.reasonix/output/` are part of the repo.
- **`Vertex::update_properties(props, record_history)`** — second boolean param controls whether the old state is pushed to `_history`. Call sites must pass the graph's `time_travel_enabled` flag.
- **`Graph::remove_vertex(id, force)`** — when `force=false` and `time_travel_enabled=true`, performs soft-delete. Otherwise hard-delete.
- **`POST /vertices`** now requires `name` (String), accepts optional `keywords` (Vec\<String\>) as built-in fields. `properties.name` is no longer used — name is top-level.
- **`Vertex` built-in fields**: `name` (required), `keywords` (additional search terms), `document` (source doc ID). Neuron keywords = labels + name + keywords.
- **`POST /vertices`** requires `name`, optional `keywords` and `document`. `properties.name` is no longer used.
- **Search mode** — Gremlin `search` step and `/search` API accept `mode: "greedy"` (default, match ANY keyword) or `"exact"` (match ALL keywords). Frontend toggles via dropdown.
- **Theme system** — CSS variables in `index.css` with `:root` (dark) and `.light` classes. Theme toggled in `App.jsx` via `document.documentElement.classList.toggle()`.
- **Frontend KnowledgeBase extraction** — switched from frontend-side LLM calls to backend async task via `POST /documents/:id/extract`, with step progress polling.
- **Sidebar collapse persisted** — collapsed state saved to `localStorage('bgraph-sidebar-collapsed')`.
- **Language switcher dropdown** — replaced EN/中文 toggle with LANG dropdown showing 中文/English.
- **ChatInput forwardRef** — exposes `focus()` method, called after response completes.
- **`POST /vertices` and `POST /edges` auto-create neurons** — HTTP handlers call `Neuron::for_vertex` / `Neuron::for_edge` + `auto_synapse`.
- **Atomic WAL via `RedologWal`** — single file `redolog.wal` logs both graph + neuron mutations in one write+fsync call. Crash between entries cannot leave inconsistent state.
- **Graph data dir is now `data/graphs/<name>/`** (was `data/<name>/`). Document files stored under `data/documents/YYMMDD/`.
- **Route params use `:param` syntax** — axum 0.7.9 requires `:param` (not `{param}`) for path parameters in `.route()`.
- **`search` step filters inactive neurons** — `activation.rs` only collects vertex refs from neurons with `activation > 0`.
- **`search` step returns full vertex data** — no longer creates synthetic empty VertexResult; looks up from graph via `g.get_vertex(vid)`.
- **MaaS proxy uses `x-default-model` header** — `GET /maas/openai/v1/models` returns the default model in the `x-default-model` response header.
- **`Neuron::with_keywords()` has CJK bug** — This generic method can lose CJK keyword strings. Use direct field assignment `neuron.keywords = keywords` instead.
- **Document delete with `?clean=true`** — Deletes vertices, edges, AND their neurons (both Vertex and Edge entity types). Default in frontend.
- **ChatInput model saved to localStorage** — Key `bgraph-last-model`. On init, prefers saved model, falls back to `x-default-model` header. Re-fetches when settings change.
- **`GET /settings` no longer returns `api_key`** — API keys are stripped for security. Frontend uses MaaS proxy instead of calling providers directly.
- **`document_extractor.rs` nid bug** — `(nn.neuron_count()+1)` in separate lock scopes returned same value. Fixed by pre-computing `start_nid` before loop.
- **`cargo build` needs `touch src/ui_serve.rs` after frontend changes** — rust-embed doesn't detect `src/ui/dist/` file changes for recompilation.
- **`semanticSearch` removed from backend** — all semantic search logic now runs on the frontend (LLM calls + graphSearch).
- **`graph_result` message type deprecated** — search results are now stored as `search_progress` messages with `graphData` field.
- **GraphViewer uses vis-network** — Canvas 2D, no WebGL required. Nodes/edges stored in DataSet with `_original` field for full data preservation.
- **Maximize uses dual GraphViewer instances** — inline card and fullscreen overlay share data via `getSnapshot()` / `applySnapshot()` pattern.
- **`NeuralConfig` is nested** — settings.json uses `activate`/`search`/`learn` groups. `NeuralConfig::default()` auto-populates missing groups (`#[serde(default)]`).
- **`Neuron::match_keywords()` takes `&ScoreConfig`** — not `&SearchMode`. ScoreConfig carries search mode, exact/partial scores, and fuzzy matching params.
- **`fuzzy_match_enabled` defaults to `true`** — Levenshtein-distance fuzzy matching is on by default for greedy searches.
- **Message action icons** — always visible below each message. User msgs: copy (SVG, 2s checkmark feedback) + edit. Assistant msgs: copy + save-to-KB.
- **`ChatInput` exposes `setText()`** — via `useImperativeHandle` for the edit-message feature.
- **ChatInput toolbar layout** — [模型选择器] [图谱开关] [图库选择] [语义|关键词] [贪婪搜索▼] [时间旅行✓] [📅日期选择]
- **Search mode default is `semantic`** — persisted to localStorage. Semantic mode forces greedy for API call.
- **Time travel datetime picker** — checkbox + `<input type="datetime-local">` for snapshot point.

- **`Graph` auto-manages neurons** — `graph_manager.add_vertex_to_graph()` / `add_edge_to_graph()` atomically create graph entity + neuron + WAL. All callers (HTTP handlers + extraction) must use these methods, not direct graph+neural manipulation.
- **Soft-delete marks neurons** — `neuron.mark_deleted(now)` instead of `nn.remove_neuron(nid)`. Idempotent. Vertex/edge `soft_delete()` methods also idempotent.
- **Time-aware search via `search_at`** — `nn.search(query, search_at)`. When set, deleted neurons with `_deleted_at > search_at` participate; otherwise filtered. Gremlin pipeline auto-injects timestamp from `timeTravel` step.
- **`expand` Gremlin step** — returns neighbor vertices + connected edges in one query. Used by frontend double-click expansion. Not a standard Gremlin step.
- **Search step no longer emits empty VertexResult** — uses `filter_map` to skip vertices where `get_vertex` returns `None`, preventing soft-deleted vertices from appearing as name="" entries.
- **`DELETE /edges/{id}`** — standalone edge deletion with `?force=true` support. Soft-deletes edge + marks neuron.
- **Document extraction auto-splits** — by chapter headings when content exceeds LLM context window. Entities deduped by name (merge keywords, merge property keys). Uses `GraphManager` API.
- **Default graph `graph0`** — time-travel enabled by default. Cannot be deleted. Old name `"default"` is deprecated.
- **`DiskGraph` replaces `Graph` for persistence** — `GraphHandle` has `disk_graph: Arc<Mutex<DiskGraph>>`. Gremlin queries snapshot DiskGraph to in-memory Graph via `snapshot()`.
- **`RedologWal` is neuron-only now** — graph ops are handled by DiskGraph's own `RedoLog`. The RedologWal replays only neuron ops (0x11-0x1F) on startup.
- **Edge ID override** — `DiskGraph::add_edge()` registers global edge ID in `edge_index`, but `Subgraph::add_edge()` returns a local ID. After calling `sg.add_edge()`, the edge's ID in the subgraph is overridden with the global ID.
- **WAL rotation** — `save_graph_snapshot()` calls `wal.rotate()` instead of `wal.truncate_after_checkpoint()`. Old WAL files are archived as `redolog.wal.{seq:04}`.
- **Subgraph checkpoint** — `graph.bin` is no longer written. Checkpoint writes `subgraphs/{id:08x}.bin` files with CRC32-based change detection.
- **Neural search 3-layer filtering** — (1) keyword match → (2) spread activation with mode-aware collection → (3) vertex-level name/keywords/labels filter against query tokens. Prevents cross-domain contamination.
- **`VertexSearchSelect`** — UI component for searching vertices in Edge creation dialog. Filters visible nodes by name substring match. No backend call.

## Implemented Plans
- `011-diskgraph-integration-incremental-persistence.md` — DiskGraph integration, subgraph checkpoint, WAL rotation, on-demand loading, 3-layer neural search filtering, edge ID fix, light theme macaron colors
- `2024-06-23-search-mode-theme-doc-fields.md` — Search modes (greedy/exact), CSS theme system, `document` built-in field, Vis-network light/dark options, Playwright e2e test, Playwright install
- `007-settings-neural-config-search-ui.md` — NeuralConfig → activate/search/learn groups, configurable search scores + fuzzy matching, /settings/neural API, settings "搜索" tab, message action icons, chat UX fixes
- `008-chat-input-toolbar-layout.md` — ChatInput toolbar reorg, message action SVG icons, semantic default, auto-focus fix, time travel datetime picker
- `001-arch-verify.md` — Full feature verification (151 tests, 0 failed)
- `002-section-paragraph-graph.md` — Section/paragraph graph structure
- `003-keyword-semantic-search.md` — keywordSearch + semanticSearch + global LLM config
- `005-ui-rewrite-knowledgebase-visnetwork.md` — Frontend rewrite + knowledge base + vis-network migration
- `2024-06-23-vertex-redolog-overhaul.md` — Vertex built-in name/keywords, RedologWal atomic WAL, directory restructure, graceful shutdown, frontend improvements
- `009-maas-proxy-neural-fix-frontend-polish.md` — MaaS OpenAI proxy, `with_keywords()` CJK fix, edge neuron cleanup on doc delete, semantic search prompt optimization, Light mode UI polish
- `010-session-comprehensive-refactor.md` — Soft-delete with time-travel, unified vertex/edge+neuron creation, extraction pipeline refactoring (split+dedup+GraphManager API), frontend graph viewer features (search, add V/E, edge edit/delete, expand step), default graph renamed to graph0
