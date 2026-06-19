# Bionic-Graph

> **Ultral fast graph indexed with bionic neural net**
>
> Pure Rust | CPU inference | Zero external NN deps | Gremlin-compatible API

---

## What it is

Bionic-Graph is a **low-cost AI memory system** that combines a knowledge graph with a bio-inspired neural index layer. It is designed for scenarios where you need a fast, explainable, always-up-to-date graph index — without GPU costs, without pre-training, and without black-box inference.

### Architecture

```
┌──────────────────────────────────────────────────────────┐
│                   Gremlin API (REST)                      │
│  V() / E() / has() / hasNot() / hasKey() / hasValue()    │
│  hasText() / out(depth) / in() / both() / repeat()       │
│  outE() / inE() / bothE() / timeTravel() / compact()     │
│  keywordSearch() / semanticSearch() / extract()           │
├──────────────────────────────────────────────────────────┤
│              Neural Index (spreading activation)          │
│  keyword → neuron activation → spread → entity find      │
│  EntityType(Vertex|Edge)  |  auto-synapse on edge add    │
│  Hebbian learning  |  auto-persist to disk               │
├──────────────────────────────────────────────────────────┤
│              Storage Engine (disk-backed)                 │
│  Subgraph partitioning  |  LRU cache  |  WAL + redo      │
│  Version log (.vlog) with sparse index for time travel   │
│  Compaction: archive old history, max_history pruning    │
└──────────────────────────────────────────────────────────┘
```

### Layers

| Layer | Module | What it does |
|-------|--------|-------------|
| **Graph** | `src/graph/` | Directed property graph with dual adjacency lists. MVCC versioning (`_version`, `_updated_at`, `_is_deleted`, `_history`). Soft-delete. Time-travel `at_time()`. Optional time-travel per graph. |
| **Neural Index** | `src/neuron/` | Spreading activation network — each neuron represents a concept or graph entity (`EntityType::Vertex`/`Edge`), fires when activation exceeds a threshold, spreads via synapses. Hebbian learning. Auto-synapse on edge creation. |
| **Gremlin API** | `src/gremlin/` | JSON pipeline over HTTP. 16 steps: V, E, has, hasNot, hasKey, hasValue, hasLabel, hasText, out(depth), in, both, outE, inE, bothE, values, limit, count, dedup, repeat, timeTravel, compact, keywordSearch, semanticSearch. |
| **Storage** | `src/storage/` | Subgraph partitioning + LRU cache. WAL (CRC32, checkpoint, crash recovery). Version log (.vlog) with sparse index for archived history. Compaction orchestrator. |
| **Extraction** | `src/extract/` | Markdown → section splitting → LLM (OpenAI-compatible) → entities/relations → graph insert. Configurable context window. |
| **Graph Manager** | `src/graph_manager.rs` | Multiple named graphs, each persisted to `data/{name}/`. Manage via REST API. Optional time-travel per graph. |
| **Config** | `src/config/` | `~/.config/bionic-graph/settings.json` with env var overrides. Auto-generates defaults. |

### How it works — a search flow

```
User query: "AI engineer"
       │
       ▼
  Neural Index (keyword matching)
       │  "AI" → Neuron("Artificial Intelligence")  activation = 1.0
       │  "engineer" → Neuron("Engineering")        activation = 1.0
       ▼
  Spreading activation (ticks)
       │  tick 1: Neuron("AI") fires → spreads to Neuron("ML") via synapse
       │  tick 2: fires → spreads / decay / refractory
       │  tick N: no more firing → stabilize
       ▼
  Collect vertex_refs from fired neurons
       │  Neuron("AI") → vertex #42, #88
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
cargo build --release
```

### Run

```bash
cargo run --release

# With auto-index and custom data directory
cargo run --release -- --data-dir ./my-data

# With explicit config file
cargo run --release -- --config ~/.config/bionic-graph/settings.json
```

#### CLI flags

| Flag | Default | Description |
|------|---------|-------------|
| `-d, --data-dir` | from settings | Data directory (overrides config) |
| `-H, --host` | from settings | HTTP bind address |
| `-P, --port` | from settings | HTTP port |
| `-i, --auto-index` | `true` | Auto-create neurons on startup |
| `--no-auto-save` | off | Disable auto-save thread |
| `--config` | `~/.config/bionic-graph/settings.json` | Config file path |

### Settings reference

Auto-created at `~/.config/bionic-graph/settings.json` if not present:

#### `server`

| Field | Default | Description |
|-------|---------|-------------|
| `host` | `"127.0.0.1"` | Bind address |
| `port` | `8080` | TCP port |

#### `extraction` (also used by `semanticSearch`)

| Field | Default | Description |
|-------|---------|-------------|
| `api_base_url` | `"https://api.deepseek.com/v1"` | LLM endpoint |
| `model` | `"deepseek-v4-flash"` | Model name |
| `context_window` | `65536` | Token limit |
| `max_output_tokens` | `8192` | Response token limit |

Required: set `BGRAPH_LLM_API_KEY` env var (or legacy `BGRAPH_EXTRACT_API_KEY`).

#### `storage`

| Field | Default | Description |
|-------|---------|-------------|
| `data_dir` | `"data"` | Data directory |
| `cache_capacity` | `1000` | LRU cache size |
| `checkpoint_interval_entries` | `1000` | WAL checkpoint trigger |
| `auto_save_interval_secs` | `5` | Save interval |

#### `graph`

| Field | Default | Description |
|-------|---------|-------------|
| `default_vertex_labels` | `["entity"]` | Fallback label |
| `max_edges_per_vertex` | `10000` | Safety limit |
| `time_travel_enabled` | `false` | Enable version history & soft-delete |

#### `neural`

| Field | Default | Description |
|-------|---------|-------------|
| `default_threshold` | `0.7` | Firing threshold |
| `default_decay_rate` | `0.1` | Per-tick decay |
| `default_refractory_ticks` | `3` | Refractory period |
| `learning_enabled` | `true` | Hebbian learning |
| `co_fire_window` | `5` | Co-firing window |

#### Environment variable overrides

| Variable | Overrides |
|----------|-----------|
| `BGRAPH_HOST` | `server.host` |
| `BGRAPH_PORT` | `server.port` |
| `BGRAPH_DATA_DIR` | `storage.data_dir` |
| `BGRAPH_LLM_API_KEY` | API key (also `BGRAPH_EXTRACT_API_KEY` for compat) |
| `BGRAPH_LLM_BASE_URL` | `extraction.api_base_url` |
| `BGRAPH_LLM_MODEL` | `extraction.model` |

### Use the API

#### Graph management

```bash
# List graphs
curl localhost:8080/graphs

# Create a graph (time-travel disabled by default)
curl -X POST localhost:8080/graphs \
  -H 'Content-Type: application/json' \
  -d '{"name":"mygraph"}'

# Create with time-travel enabled
curl -X POST localhost:8080/graphs \
  -H 'Content-Type: application/json' \
  -d '{"name":"audit","time_travel":true}'

# Delete a graph
curl -X DELETE localhost:8080/graphs/mygraph

# All data endpoints support X-Graph-Name header (default: "default")
curl -X POST localhost:8080/vertices \
  -H 'Content-Type: application/json' \
  -H 'X-Graph-Name: audit' \
  -d '{"labels":["person"],"properties":{"name":"Alice"}}'
```

#### Health check

```bash
curl localhost:8080/health
```

#### Neural search + traversal

```bash
# keywordSearch: neural index search (returns vertices + edges)
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"keywordSearch","keywords":["AI","engineer"]},
    {"step":"out","label":"works_at","depth":2},
    {"step":"hasText","key":"name","pattern":"ali"},
    {"step":"limit","count":10}
  ]}'

# semanticSearch: LLM extracts keywords, then keywordSearch + LLM result filtering
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"semanticSearch","query":"Find engineers who work on AI projects"}
  ]}'
```

#### Time travel (requires graph with `time_travel: true`)

```bash
# Query data as of a specific point in time
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"timeTravel","at":"2024-06-10T12:00:00Z"},
    {"step":"V"},
    {"step":"out","label":"knows"}
  ]}'

# ISO 8601 or Unix microseconds both work
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"timeTravel","at":1718000000000000},
    {"step":"V"}
  ]}'
```

#### Compaction (archive old history to version log)

```bash
# Compact by timestamp
curl -X POST localhost:8080/compact \
  -H 'Content-Type: application/json' \
  -d '{"before":"2024-01-01T00:00:00Z"}'

# Via Gremlin step
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[{"step":"compact","before":1704067200000000}]}'
```

#### Document extraction

Extracts entities, relations, section hierarchy, and paragraphs from Markdown:

```bash
# Set your LLM API key first
export BGRAPH_LLM_API_KEY=sk-...

# Extract from a Markdown file
curl -X POST localhost:8080/extract \
  -H 'Content-Type: text/markdown' \
  --data-binary @README.md
```

The extraction pipeline:
1. Splits the document into **sections** by headings (inserted as `section` vertices)
2. Splits each section's content into **paragraphs** (inserted as `paragraph` vertices)
3. Calls the LLM to extract **entities** and **relations** (inserted as vertices + edges)
4. Creates **`mentioned_in`** edges linking entities to their source section
5. Maintains section hierarchy via **`has_subsection`** edges
6. Auto-creates **neural synapses** between related entities via `auto_synapse`

Response example:
```json
{"graph":"default","stats":{
  "new_vertices":137, "new_edges":238,
  "processed_sections":21, "total_sections":21
}}
```

> Requires `BGRAPH_LLM_API_KEY` env var (or `BGRAPH_EXTRACT_API_KEY` for backward compat).

#### Supported Gremlin steps

| Step | Parameters | Description |
|------|-----------|-------------|
| `keywordSearch` | `keywords: [string]` | 🔥 Neural index search (vertices + edges) |
| `semanticSearch` | `query: string` | 🔥 LLM → keywordSearch → LLM filter |
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

### Run demo & tests

```bash
cargo run --example demo
cargo test   # 151 unit tests
```

---

## Project structure

```
src/
├── main.rs                    # CLI entry + HTTP server
├── lib.rs                     # Library exports
├── config/                    # File-based configuration
│   ├── settings.rs            # Settings struct (5 sub-configs)
│   └── loader.rs              # Load + env override + default generation
├── graph/                     # Knowledge graph core
│   ├── vertex.rs              # Vertex type + MVCC (version, history, at_time)
│   ├── edge.rs                # Edge type + MVCC
│   ├── graph.rs               # Adjacency list + time-travel flag
│   └── traversal.rs           # BFS/DFS iterators
├── neuron/                    # Bio-inspired neural index
│   ├── neuron.rs              # Neuron/Synapse structs
│   ├── network.rs             # Network orchestration
│   ├── activation.rs          # Spreading activation algorithm
│   └── learning.rs            # Hebbian learning
├── storage/                   # Disk-backed storage engine
│   ├── disk_graph.rs          # Disk-backed Graph (SubgraphCache + WAL)
│   ├── index.rs               # VertexIndex / SubgraphIndex / LabelIndex
│   ├── subgraph.rs            # Subgraph data + serialization
│   ├── subgraph_cache.rs      # LRU cache + on-demand load
│   ├── redo_log.rs            # WAL + Checkpoint + crash recovery
│   ├── version_log.rs         # .vlog format with sparse index
│   ├── compaction.rs          # History compaction orchestrator
│   └── partition.rs           # BFS clustering partitioner
├── persistence/               # Persistence helpers
│   ├── graph_store.rs         # Graph persistence (legacy + disk)
│   ├── neuron_store.rs        # Neural network persistence
│   └── auto_save.rs           # Auto-save / checkpoint thread
├── gremlin/                   # REST API (axum)
│   ├── query.rs               # Gremlin query types
│   ├── steps.rs               # Step execution engine
│   └── server.rs              # Routes + handlers
├── extract/                   # Document knowledge extraction
│   ├── config.rs              # LLM config
│   ├── document.rs            # Markdown reader + section splitter
│   ├── llm_client.rs          # OpenAI-compatible API client
│   ├── extraction.rs          # Prompt templates + response parser
│   └── pipeline.rs            # Orchestrator
├── graph_manager.rs           # Multi-graph management
└── memory_system.rs           # Top-level unified API
```

---

## Design principles

1. **Pure Rust** — zero external neural network libraries
2. **CPU inference** — all computation in memory, no GPU
3. **Bio-inspired** — spreading activation mimics biological neurons
4. **Low cost** — lightweight memory index for edge/embedded scenarios
5. **Gremlin-compatible** — standard graph query interface
6. **Time travel** — per-vertex MVCC, soft-delete, point-in-time queries
7. **Multi-graph** — multiple named graphs, isolated data directories

---

## License

MIT
