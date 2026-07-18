# Bionic-Graph

> **Block-based knowledge graph with token-indexed search**
>
> Pure Rust | 16KB block storage | jieba tokenization | Gremlin-compatible API | React frontend

---

## What it is

Bionic-Graph is a **high-performance knowledge graph engine** built entirely in Rust. It combines a custom block-based storage engine, token-indexed full-text search, and a Gremlin-compatible query pipeline — served with a chat-based AI interface and a React frontend.

Unlike relational or document databases, Bionic-Graph is optimized for **graph traversal, multi-hop queries, and hybrid search** (keywords + graph topology). It is designed for scenarios where you need a fast, explainable, always-up-to-date graph index — without GPU costs, without pre-training, and without black-box inference.

### System Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                    React Frontend (vis-network)               │
│  Chat interface  |  Knowledge Base  |  Graph Visualization   │
│  LLM Chat (SSE)  |  Semantic Search  |  Document Extraction  │
├──────────────────────────────────────────────────────────────┤
│                   REST API + MaaS Proxy (axum, embedded)      │
│  /gremlin  |  /vertices  |  /edges  |  /documents  |  /search │
│  /maas/openai/v1/models | /maas/openai/v1/chat/completions   │
│  /settings | /extract  | /graphs                             │
├──────────────────────────────────────────────────────────────┤
│              Graph Engine (token-indexed query)                │
│  Gremlin pipeline (25 steps)  |  BFS+DFS traversal            │
│  Lock-safe CRUD  |  jieba-rs tokenizer  |  rank/atime tracking│
├──────────────────────────────────────────────────────────────┤
│              In-Memory Index (rebuild at startup)              │
│  BTreeMap (vertex/edge by ID)  |  TokenMap (prefix + word)    │
│  RankIndex (rank-ordered)  |  AdjacencyIndex (bidirectional)  │
├──────────────────────────────────────────────────────────────┤
│              Storage Engine (block-based, 16KB blocks)         │
│  DataFile + Bitmap  |  IndexFile (64B records)                 │
│  LRU BlockCache (64MB)  |  WAL (FIFO queue + batch writer, CRC32, rotation, checkpoint)    │
│  LockManager (striped RwLock pools, deadlock-free ordering)   │
└──────────────────────────────────────────────────────────────┘
```

### Layers

| Layer | Module | What it does |
|-------|--------|-------------|
| **Frontend** | `src/ui/` | React 19 + Vite 8 + Tailwind CSS 4. Chat interface, knowledge base management, graph visualization via vis-network (Canvas 2D, no WebGL). All LLM calls go through backend MaaS proxy. |
| **Graph Engine** | `src/graph/` | `Graph` struct (facade), CRUD operations, Gremlin pipeline (25 steps), jieba-rs tokenizer, bincode serialize. Lock-safe wrappers in `locked.rs`. |
| **Gremlin API** | `src/gremlin/` | REST routes (44+ endpoints). Auto-injects `match_mode` and `traverse` step from graph search config. |
| **Web Search** | `src/gremlin/settings.rs` | Backend proxy for web search (`/web-search/proxy`, `POST`). Configurable providers (Bing, Baidu, etc.) with custom URL, method, headers, body template. |
| **Python SDK** | `sdk/python/` | Full REST API client library (`pip install git+...`). CLI tool `bgcli` with interactive chat mode supporting web + graph search. |

### How it works — a search flow

```
User query: "AI engineer"
       │
       ▼
  Step 1 — Tokenization (jieba-rs)
       │  "AI" → lookup TokenMap → vertex/edge refs
       │  "engineer" → lookup TokenMap → vertex/edge refs
       ▼
  Step 2 — Score & rank (greedy or exact)
       │  Greedy: union of ALL matched entities, scored by frequency
       │  Exact: intersection of entities matching EVERY token
       ▼
  Step 3 — Optional traverse (configurable via SearchSettings)
       │  BFS from search results: score = score * decay * edge_strength
       │  Stop when score < activate. Collect when score >= min_score.
       ▼
  Step 4 — Return ranked results (time-travel filtered if specified)
       │  Soft-deleted entities before `at` timestamp are excluded
       │  Entities created after `at` timestamp are excluded
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
| `cargo test` | Rust unit tests (84+) |
| `npm --prefix src/ui run dev` | Frontend dev server (standalone Vite, proxies to port 8080) |
| `npm --prefix src/ui run build` | Build frontend only |
| `npm --prefix src/ui run test` | Frontend tests (vitest) |

### CLI flags

| Flag | Default | Description |
|------|---------|-------------|
| `-d, --data-dir` | from settings | Data directory (overrides config) |
| `-H, --host` | from settings | HTTP bind address |
| `-P, --port` | from settings | HTTP port |
| `--config` | `~/.config/bionic-graph/settings.json` | Config file path |

### Settings

Auto-created at `~/.config/bionic-graph/settings.json` if not present. Full reference in `REASONIX.md`.

```json
{
  "server": { "host": "127.0.0.1", "port": 8080 },
  "llm": {
    "providers": [{
      "name": "DeepSeek",
      "api_base_url": "https://api.deepseek.com/v1",
      "api_key": "",
      "models": ["deepseek-v4-flash", "deepseek-v4-pro"]
    }],
    "default_model": "DeepSeek/deepseek-v4-flash",
    "context_window": 65536,
    "max_output_tokens": 16384,
    "max_retries": 3
  },
  "storage": { "data_dir": "data" },
  "cluster": {
    "enabled": false,
    "bind_addr": "0.0.0.0:9090",
    "heartbeat_interval_secs": 5,
    "worker_timeout_secs": 30,
    "forward_writes": true
  },
  "web_search": {
    "default_provider": "baidu",
    "providers": [{
      "id": "baidu",
      "name": "\u767e\u5ea6\u641c\u7d22",
      "search_url": "https://qianfan.baidubce.com/v2/ai_search/web_search",
      "method": "POST",
      "body_template": "{\"messages\":[{\"content\":\"{text}\",\"role\":\"user\"}],\"search_source\":\"baidu_search_v2\",\"resource_type_filter\":[{\"type\":\"web\",\"top_k\":5}],\"search_recency_filter\":\"year\"}"
    }]
  },
  "graph": {
    "storage": { "data_dir": "data" },
    "search": {
      "greedy": {
        "traverse": true, "match_mode": "prefix",
        "activate": 0.2, "decay": 0.95, "depth": 16, "score": 0.1
      },
      "exact": {
        "traverse": true, "match_mode": "word",
        "activate": 0.6, "decay": 0.8, "depth": 4, "score": 0.2
      }
    },
    "rank": {
      "auto_inc_rank_when_update": true,
      "auto_inc_rank_when_read": true,
      "auto_dec_rank_when_inactive": true,
      "inactive_after_accessed_secs": 1296000,
      "inactive_rank_update_period": 86400
    }
  }
}
```

### Use the API

#### Graph management

```bash
# List graphs
curl localhost:8080/graphs

# Create a graph
curl -X POST localhost:8080/graphs \
  -H 'Content-Type: application/json' \
  -d '{"name":"mygraph"}'

# Delete a graph
curl -X DELETE localhost:8080/graphs/mygraph

# Per-graph config
curl localhost:8080/graphs/mygraph/config
```

#### Vertex & Edge CRUD

```bash
# Create a vertex
curl -X POST localhost:8080/vertices \
  -H 'Content-Type: application/json' \
  -d '{"name":"Alice","keywords":["engineer","manager"],"labels":["person"],"properties":{"department":"Engineering"}}'

# With explicit graph name
curl -X POST 'localhost:8080/vertices?graph=mygraph' \
  -H 'Content-Type: application/json' \
  -d '{"name":"Bob","labels":["person"]}'

# Update vertex
curl -X PUT localhost:8080/vertices/1 \
  -H 'Content-Type: application/json' \
  -d '{"name":"Alice Smith","keywords":["engineer","lead"],"labels":["person","employee"]}'

# Delete vertex (soft delete)
curl -X DELETE localhost:8080/vertices/1

# Hard delete
curl -X DELETE 'localhost:8080/vertices/1?force=true'

# Create edge
curl -X POST localhost:8080/edges \
  -H 'Content-Type: application/json' \
  -d '{"source":1,"target":2,"name":"works_with","labels":["relationship"],"keywords":["colleague"],"strength":0.8,"properties":{"since":"2024"}}'
```

#### Token search + traversal

```bash
# Search with auto traverse (based on SearchSettings)
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"search","text":"AI engineer"}
  ]}'

# Advanced pipeline with explicit traverse
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"search","text":"AI engineer","mode":"greedy"},
    {"step":"out","labels":["works_at"],"depth":2},
    {"step":"limit","count":10}
  ]}'

# Time travel query
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"timeTravel","at":1718000000000000},
    {"step":"search","text":"project"}
  ]}'

# Expand vertex (neighbors + edges)
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"V","ids":[1]},
    {"step":"expand","depth":1}
  ]}'

# Shorthand search via GET
curl 'localhost:8080/search?text=AI+engineer&mode=greedy&graph=default'
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
| `GET/PUT` | `/settings/search` | Search settings (greedy/exact config) |
| `GET/PUT` | `/settings/rank` | Rank decay config |
| `GET` | `/settings/tokenizer` | Tokenizer custom dictionary config |
| `POST/DELETE` | `/settings/tokenizer/words` | Add / remove custom tokenizer words |
| `POST` | `/documents/:id/extract` | Extract from document by ID |
| `GET` | `/maas/openai/v1/models` | List models (`provider/model` format) |
| `POST` | `/maas/openai/v1/chat/completions` | OpenAI-compatible chat proxy (SSE) |
| `POST` | `/extract` | Submit document extraction (async) |
| `GET` | `/extract/task/:task_id` | Poll extraction task |

### Supported Gremlin steps

| Step | Parameters | Description |
|------|-----------|-------------|
| `search` | `text`, `mode?`, `match_mode?`, `at?`, `limit?`, `min_rank?` | Token-indexed full-text search. `mode` = `"greedy"` (union of any token match) or `"exact"` (intersection — must match all tokens). `match_mode` = `"prefix"` or `"word"`. Auto-injects `match_mode` from SearchSettings + optional `traverse` step. |
| `V` | `ids?`, `at?` | All vertices or filtered by ID array. `at` enables time-travel. |
| `E` | `ids?`, `at?` | All edges or filtered by ID array. `at` enables time-travel. |
| `has` | `key`, `value` | Filter results by exact property key-value match (Vertex and Edge properties). |
| `hasNot` | `key`, `value` | Negated property filter — exclude if property matches. |
| `hasKey` | `key` | Filter by property key existence. |
| `hasValue` | `value` | Filter by any property value match. |
| `hasLabel` | `label` | Filter by labels array (checks both Vertex.labels and Edge.labels). |
| `hasText` | `text` | Case-insensitive substring match against name, labels, keywords, and string properties. |
| `out` | `depth?`, `labels?` | BFS traversal to outgoing neighbor vertices. `labels` filters by target vertex labels. `depth` controls BFS depth (default 1). |
| `in` | `depth?`, `labels?` | BFS traversal to incoming neighbor vertices. |
| `both` | `depth?`, `labels?` | Bidirectional BFS traversal (out + in, deduplicated). |
| `outE` | `labels?` | Outgoing edges as Edge results. `labels` filters by Edge.labels array. |
| `inE` | `labels?` | Incoming edges as Edge results. `labels` filters by Edge.labels array. |
| `bothE` | `labels?` | Both-direction edges as Edge results (outE + inE, deduplicated). |
| `values` | `keys?` | Extract specific property keys from each result (filters to listed keys). |
| `limit` | `count` | Cap number of results to `count`. |
| `count` | — | Replace results with a single `{count: N}` item. |
| `dedup` | — | Deduplicate results by ID (removes duplicate vertices/edges). |
| `repeat` | `steps`, `times` | Execute sub-pipeline `steps` iteratively `times` times. |
| `timeTravel` | `at` (μs) | Set global query timestamp. Subsequent steps only see data as it existed at `at`. |
| `compact` | `before` (μs) | Passthrough stub (no-op). |
| `expand` | `depth?`, `label?` | From each vertex, add its neighbor vertices + connecting edges to results (both directions). Optional `label` filters by edge label. |
| `traverse` | `decay?`, `activate?`, `max_depth?`, `min_score?` | BFS activation spread from input vertices. Score = parent_score × `decay` × edge_strength. Stops when score < `activate`. Collects results with score >= `min_score`. Defaults: decay=0.95, activate=0.2, max_depth=16, min_score=0.1. |
| `rank` | `limit?`, `min?` | Return top results by rank (source or filter step). |

---

## Project structure

```
src/
├── main.rs                    # CLI entry + HTTP server
├── lib.rs                     # Library exports
├── config/                    # File-based configuration
│   ├── loader.rs              # JSON load/save with env overrides
│   └── settings.rs            # Settings structs (server, llm, storage, cluster, search)
├── storage/                   # Block-based storage engine
│   ├── types.rs               # Constants, enums, binary layouts
│   ├── data_file.rs           # 16KB block I/O
│   ├── bitmap_file.rs         # Block-level free space tracking
│   ├── block_allocator.rs     # Chunk-level allocator
│   ├── block_cache.rs         # LRU cache with dirty tracking
│   ├── redo_log.rs            # WAL: FIFO queue + batch writer, rotation, CRC32, replay
│   ├── index_file.rs          # On-disk 64B record index
│   ├── memory_index.rs        # In-memory BTreeMap/HashMap indexes
│   └── memory_index_builder.rs # Index rebuild at startup
├── lock/                      # Concurrency lock manager
│   └── lock_manager.rs        # Striped RwLock pools (parking_lot)
├── graph/                     # Graph engine
│   ├── graph.rs               # Graph struct (facade), open/close
│   ├── graph_registry.rs      # Graph metadata registry
│   ├── crud.rs                # Vertex/Edge CRUD + WAL + tokenize
│   ├── gremlin.rs             # Gremlin pipeline (25 steps)
│   ├── locked.rs              # Lock-safe CRUD wrappers
│   ├── serialize.rs           # Bincode + JSON properties
│   ├── tokenizer.rs           # jieba-rs tokenizer
│   └── tests.rs               # Integration tests
├── gremlin/                   # REST API (axum)
│   ├── mod.rs                 # 45+ route handlers
│   ├── settings.rs            # /settings/graph/search, /settings/llm, /settings/graph/rank, /settings/web-search, /web-search/proxy
│   └── tokenizer_settings.rs  # /settings/tokenizer + /settings/tokenizer/words
├── extract/                   # Document extraction pipeline
│   ├── config.rs, document.rs, extraction.rs
│   ├── llm_client.rs, task_manager.rs
├── documents.rs               # Document CRUD manager
├── graph_manager.rs           # Multi-graph lifecycle
├── maas/                      # MaaS OpenAI-compatible proxy
├── cluster/                   # Master-worker cluster
├── ui_serve.rs                # Embedded frontend serving
└── ui/                        # React frontend
    ├── src/
    │   ├── components/
    │   │   ├── Sidebar.jsx, ChatArea.jsx, MessageList.jsx
    │   │   ├── ChatInput.jsx, GraphViewer.jsx
    │   │   ├── GraphManagerDialog.jsx, KnowledgeBase.jsx
    │   │   ├── SettingsDialog.jsx, PropertyPanel.jsx
    │   └── api.js, App.jsx, locales/
    └── dist/                  # Compiled (embedded in binary)
```

---

## Design principles

1. **Single binary** — frontend embedded via rust-embed, one `cargo run` to start
2. **All LLM proxied** — chat, semantic search, document extraction go through MaaS proxy
3. **Pure Rust backend** — zero external NN libraries, custom block-based storage
4. **CPU inference** — all computation in memory, no GPU
5. **Token-indexed search** — jieba-rs tokenization replaces old neural network index
6. **Custom storage engine** — 16KB blocks, 64B chunks, LRU cache, WAL with crash recovery
7. **Gremlin-compatible** — standard graph query interface with 25 pipeline steps
8. **Time travel** — per-vertex MVCC via soft-delete, point-in-time queries
9. **Multi-graph** — multiple named graphs, isolated `data/graphs/<name>/` directories
10. **Fine-grained concurrency** — striped RwLock pools with deadlock-free ordering
11. **Web Search** — backend proxy for web search, configurable providers (Bing, Baidu API). LLM extracts keywords before searching for better results.
12. **Python SDK** — `pip install git+https://github.com/agentic-data-ops/bionic-graph.git#subdirectory=sdk/python`, full REST API client with CLI tool `bgcli` and interactive chat mode.

## Python SDK & CLI

A complete Python client library and CLI tool are available in `sdk/python/`:

```bash
# Install from GitHub
pip install git+https://github.com/agentic-data-ops/bionic-graph.git#subdirectory=sdk/python

# CLI usage
bgcli --base-url http://127.0.0.1:8080 health check
bgcli vertex create --name "Eddard Stark" --labels '["person"]' --graph got
bgcli gremlin search --text "Stark" --graph got

# Interactive chat with web + graph search
bgcli chat --model "DeepSeek/deepseek-v4-flash"

# From Python code
from bionic_graph import Client
client = Client()
resp = client.create_vertex("Jon Snow", labels=["person", "stark"])
print(f"Created vertex {resp.id}")
```

See `sdk/python/SKILL.md` for full documentation.

---

## License

MIT
