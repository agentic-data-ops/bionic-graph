# Bionic-Graph

> **Ultral fast graph indexed with bionic neural net**
>
> Pure Rust | CPU inference | Zero external NN deps | Gremlin-compatible API | React frontend

---

## What it is

Bionic-Graph is a **low-cost AI memory system** that combines a knowledge graph with a bio-inspired neural index layer, served with a chat-based AI interface. It is designed for scenarios where you need a fast, explainable, always-up-to-date graph index — without GPU costs, without pre-training, and without black-box inference.

### System Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                    React Frontend (vis-network)                │
│  Chat interface  |  Knowledge Base  |  Graph Visualization    │
│  LLM Chat (SSE)  |  Semantic Search  |  Document Extraction   │
├──────────────────────────────────────────────────────────────┤
│                    REST API + MaaS Proxy (axum, embedded)       │
│  /gremlin  |  /vertices  |  /edges  |  /documents  |  /search  │
│  /maas/openai/v1/models | /maas/openai/v1/chat/completions     │
│  /settings | /extract  | /neurons                              │
├──────────────────────────────────────────────────────────────┤
│              Neural Index (spreading activation)               │
│  keyword → neuron activation → spread → entity find           │
│  EntityType(Vertex|Edge)  |  auto-synapse on edge add         │
│  Hebbian learning  |  auto-persist to disk                    │
├──────────────────────────────────────────────────────────────┤
│              Storage Engine (disk-backed)                      │
│  DiskGraph + SubgraphCache (LRU, on-demand loading)           │
│  Subgraph partitioning  |  WAL (RedologWal + RedoLog)         │
│  Incremental checkpoint (CRC32 change detection)              │
│  Version log (.vlog) with sparse index for time travel        │
│  Compaction: archive old history, max_history pruning         │
└──────────────────────────────────────────────────────────────┘
```

### Layers

| Layer | Module | What it does |
|-------|--------|-------------|
| **Frontend** | `src/ui/` | React 19 + Vite 8 + Tailwind CSS 4. Chat interface, knowledge base management, graph visualization via vis-network (Canvas 2D, no WebGL). All LLM calls (chat, semantic search, document extraction) are frontend-side. |
| **Graph** | `src/graph/` | Directed property graph with dual adjacency lists. MVCC versioning (`_version`, `_updated_at`, `_is_deleted`, `_history`). Soft-delete. Time-travel `at_time()`. Optional time-travel per graph. |
| **Neural Index** | `src/neuron/` | Spreading activation network — each neuron represents a concept or graph entity (`EntityType::Vertex`/`Edge`), fires when activation exceeds a threshold, spreads via synapses. Hebbian learning. Auto-synapse on edge creation. |
| **Gremlin API** | `src/gremlin/` | JSON pipeline over HTTP. 16 steps: V, E, has, hasNot, hasKey, hasValue, hasLabel, hasText, out(depth), in, both, outE, inE, bothE, values, limit, count, dedup, repeat, timeTravel, compact, search, expand. |
| **Storage** | `src/storage/` | Subgraph partitioning + LRU cache. WAL (CRC32, checkpoint, crash recovery). Version log (.vlog) with sparse index for archived history. Compaction orchestrator. |
| **Documents** | `src/documents.rs` | Markdown file management with JSON index. CRUD via REST API. |
| **Extraction** | `src/extract/` | Async extraction pipeline: Markdown → LLM → entities/relations. Frontend calls `POST /documents/:id/extract` (backend) or LLM directly. |
| **Graph Manager** | `src/graph_manager.rs` | Multiple named graphs, each persisted to `data/graphs/{name}/`. Manage via REST API. |
| **Config** | `src/config/` | `~/.config/bionic-graph/settings.json` with env var overrides. Auto-generates defaults. |

### How it works — a search flow

```
User query: "AI engineer"
       │
       ▼
  Layer 1 — Keyword matching (neuron level)
       │  "AI" → Neuron("Artificial Intelligence")  score = 1.0
       │  "engineer" → Neuron("Engineering")        score = 1.0
       ▼
  Layer 2 — Spreading activation & collection
       │  Exact mode: only directly-matched neurons contribute
       │  Greedy mode: + spread-activated neurons with activation ≥ 0.3
       │  tick 1: Neuron("AI") fires → spreads to Neuron("ML") via synapse
       │  tick N: no more firing → stabilize
       ▼
  Layer 3 — Vertex-level relevance filter
       │  Vertex name/keywords/labels must contain query token
       │  Filters out cross-domain noise from contaminated neuron keywords
       ▼
  Gremlin traversal from starting vertices
       │  timeTravel("2024-06-10") → out("works_at", depth=3)
       │  hasText("name", "ali") → limit(10)
       ▼
  Return ranked results (time-travel filtered if specified)
```

---

## How to

### Clone & build

```bash
git clone <repo-url>
cd bionic-graph

# The frontend is built and embedded during cargo build
cargo build --release
```

### Run

```bash
cargo run --release
# → Open http://127.0.0.1:8080 to access the chat UI
# → API available at the same address
```

After frontend changes, `touch src/ui_serve.rs` is required to force Rust recompilation (rust-embed doesn't auto-detect `dist/` changes).

### Commands

| Command | Description |
|---------|-------------|
| `cargo run` | Start server (API + frontend) |
| `cargo test` | Rust unit tests (151+) |
| `npm --prefix src/ui run dev` | Frontend dev server (standalone Vite, proxies to port 8080) |
| `npm --prefix src/ui run build` | Build frontend only |
| `npm --prefix src/ui run test` | Frontend tests (vitest) |

### CLI flags

| Flag | Default | Description |
|------|---------|-------------|
| `-d, --data-dir` | from settings | Data directory (overrides config) |
| `-H, --host` | from settings | HTTP bind address |
| `-P, --port` | from settings | HTTP port |
| `-i, --auto-index` | `true` | Auto-create neurons on startup |
| `--no-auto-save` | off | Disable auto-save thread |
| `--config` | `~/.config/bionic-graph/settings.json` | Config file path |

### Settings

Auto-created at `~/.config/bionic-graph/settings.json` if not present. Neural config is nested under `activate`/`search`/`learn` groups. Full reference in `REASONIX.md`.

Required: `BGRAPH_LLM_API_KEY` env var for backend LLM features.

### Use the API

#### Graph management

```bash
# List graphs
curl localhost:8080/graphs

# Create a graph
curl -X POST localhost:8080/graphs \
  -H 'Content-Type: application/json' \
  -d '{"name":"mygraph","time_travel":true}'

# Delete a graph
curl -X DELETE localhost:8080/graphs/mygraph

# All data endpoints support X-Graph-Name header (default: "default")
curl -X POST localhost:8080/vertices \
  -H 'Content-Type: application/json' \
  -H 'X-Graph-Name: audit' \
  -d '{"name":"Alice","keywords":["engineer","manager"],"labels":["person"],"properties":{"department":"Engineering"}}'

# Update vertex
curl -X PUT localhost:8080/vertices/1 \
  -H 'Content-Type: application/json' \
  -H 'X-Graph-Name: default' \
  -d '{"name":"Alice Smith","tags":["engineer","lead"],"labels":["person","employee"]}'

# Update edge
curl -X PUT localhost:8080/edges/1 \
  -H 'Content-Type: application/json' \
  -H 'X-Graph-Name: default' \
  -d '{"label":"manages","properties":{"since":"2024"}}'
```

#### Neural search + traversal

```bash
# search: neural index search (returns vertices + edges)
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"search","keywords":["AI","engineer"]},
    {"step":"out","label":"works_at","depth":2},
    {"step":"hasText","key":"name","pattern":"ali"},
    {"step":"limit","count":10}
  ]}'
```

#### Document management

```bash
# Add a document
curl -X POST localhost:8080/documents \
  -H 'Content-Type: application/json' \
  -d '{"title":"my-doc","content":"# Hello\nWorld","tags":["test"]}'

# List documents
curl localhost:8080/documents

# Get document content
curl localhost:8080/documents/{id}/content
```

#### Other endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | System health |
| `GET` | `/extract/tasks` | List extraction tasks |
| `POST` | `/documents/:id/extract` | Trigger backend document extraction (async) |
| `GET` | `/extract/task/:task_id` | Poll extraction task progress |
| `PUT` | `/vertices/:id` | Update vertex name/keywords/document/labels/properties |
| `PUT` | `/edges/:id` | Update edge label/properties |
| `DELETE` | `/vertices/:id` | Delete vertex + connected edges (supports `?force=true`) |
| `DELETE` | `/edges/:id` | Delete edge (supports `?force=true`) |
| `POST` | `/reindex` | Re-index edges into neural network |
| `POST` | `/compact` | History compaction |
| `GET/PUT` | `/settings` | LLM providers config |
| `GET/PUT` | `/settings/neural` | Neural activation/search/learn config |
| `GET` | `/maas/openai/v1/models` | List models (`provider/model` format, `x-default-model` header) |
| `POST` | `/maas/openai/v1/chat/completions` | OpenAI-compatible chat completion proxy (SSE streaming) |

### Supported Gremlin steps

| Step | Parameters | Description |
|------|-----------|-------------|
| `search` | `keywords: [string], mode?: "greedy"\|"exact", at?: int` | 🔥 Neural index search (vertices + edges). `at` enables time-travel filter: neurons deleted before `at` are excluded |
| `V` | `ids?: [number]` | All or specific vertices |
| `E` | `ids?: [number]` | All or specific edges |
| `has` | `key, value` | Exact property filter |
| `hasNot` | `key, value` | Negated property filter |
| `hasKey` | `key` | Filter by property existence |
| `hasValue` | `value` | Filter by any property value |
| `hasLabel` | `labels: [string]` | Label filter |
| `hasText` | `key, pattern` | Case-insensitive substring match |
| `out` | `label?, depth?` | Outgoing edges (depth=N for BFS) |
| `in` | `label?, depth?` | Incoming edges |
| `both` | `label?, depth?` | Both directions |
| `outE` | `label?` | Outgoing edges as EdgeResult |
| `inE` | `label?` | Incoming edges as EdgeResult |
| `bothE` | `label?` | Both-direction edges as EdgeResult |
| `values` | `key` | Extract property values |
| `limit` | `count` | Cap results |
| `count` | — | Count results |
| `dedup` | — | Deduplicate by ID |
| `repeat` | `times, steps[]` | Repeat sub-pipeline N times |
| `timeTravel` | `at` (int μs or ISO 8601) | Set query time point |
| `compact` | `before` (int μs or ISO 8601) | Archive old history to vlog |
| `expand` | `depth, label` | Expand vertex: returns neighbor vertices + connected edges |

---

## Project structure

```
src/
├── main.rs                    # CLI entry + HTTP server
├── lib.rs                     # Library exports
├── config/                    # File-based configuration
├── graph/                     # Knowledge graph core
├── neuron/                    # Bio-inspired neural index
├── storage/                   # Disk-backed storage engine (DiskGraph + SubgraphCache)
│   ├── disk_graph.rs          # Disk-backed graph with LRU subgraph cache
│   ├── subgraph_cache.rs      # LRU write-back cache
│   ├── subgraph.rs            # Subgraph data unit
│   ├── partition.rs           # BFS graph partitioning
│   ├── index.rs               # VertexIndex, SubgraphIndex, LabelIndex
│   ├── redolog_wal.rs         # Unified WAL (graph + neuron)
│   └── redo_log.rs            # Subgraph-level WAL
├── persistence/               # Persistence helpers (neuron_store, graph_store)
├── gremlin/                   # REST API (axum)
│   ├── query.rs               # Gremlin query types
│   ├── steps.rs               # Step execution engine
│   └── server.rs              # Routes + handlers
├── extract/                   # (Legacy) Document extraction
├── documents.rs               # Document CRUD manager
├── graph_manager.rs           # Multi-graph management
├── ui_serve.rs                # Embedded frontend serving
├── memory_system.rs           # Top-level unified API
└── ui/                        # React frontend
    ├── src/
    │   ├── components/
    │   │   ├── Sidebar.jsx        # Navigation + conversation list
    │   │   ├── ChatArea.jsx       # Chat orchestration
    │   │   ├── MessageList.jsx    # Message rendering
    │   │   ├── ChatInput.jsx      # Input + controls
    │   │   ├── GraphViewer.jsx    # vis-network graph visualization
    │   │   ├── KnowledgeBase.jsx  # Document management dialog
    │   │   ├── SettingsDialog.jsx # Settings panel
    │   │   └── PropertyPanel.jsx  # Node/edge property inspector
    │   ├── api.js              # API client + LLM streaming
    │   ├── App.jsx             # Root component
    │   └── locales/            # i18n (en/zh)
    └── dist/                   # Compiled frontend (embedded in binary)
```

---

## Design principles

1. **Single binary** — frontend embedded via rust-embed, one `cargo run` to start
2. **All LLM on frontend** — chat, semantic search, document extraction all call LLM directly from browser
3. **Pure Rust backend** — zero external neural network libraries
4. **CPU inference** — all computation in memory, no GPU
5. **Bio-inspired** — spreading activation mimics biological neurons
6. **Low cost** — lightweight memory index for edge/embedded scenarios
7. **Gremlin-compatible** — standard graph query interface
8. **Time travel** — per-vertex MVCC, soft-delete, point-in-time queries
9. **Multi-graph** — multiple named graphs, isolated data directories

---

## License

MIT
