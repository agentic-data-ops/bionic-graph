# Bionic-Graph

> **Ultral fast graph indexed with bionic neural net**
>
> Pure Rust | CPU inference | Zero external NN deps | Gremlin-compatible API

---

## What it is

Bionic-Graph is a **low-cost AI memory system** that combines a knowledge graph with a bio-inspired neural index layer. It is designed for scenarios where you need a fast, explainable, always-up-to-date graph index — without GPU costs, without pre-training, and without black-box inference.

### Architecture

```
┌──────────────────────────────────────────────────────┐
│                   Gremlin API (REST)                  │
│  V() / E() / has() / hasText() / out(depth) / in()   │
│  both() / repeat() / limit() / neuralSearch()        │
│  extract(Markdown)                                   │
├──────────────────────────────────────────────────────┤
│              Neural Index (spreading activation)      │
│  keyword → neuron activation → spread → vertex find  │
│  Hebbian learning  |  auto-persist to disk           │
├──────────────────────────────────────────────────────┤
│              Knowledge Graph (adjacency list)         │
│  Vertex / Edge / Property  |  BFS / DFS traversal    │
├──────────────────────────────────────────────────────┤
│              Storage Engine (disk-backed)             │
│  Subgraph partitioning  |  LRU cache  |  WAL + redo  │
└──────────────────────────────────────────────────────┘
```

### Three layers in detail

| Layer | Module | What it does |
|-------|--------|-------------|
| **Graph** | `src/graph/` | Directed property graph with dual adjacency lists (forward + backward). Vertices carry labels and key-value properties. Edges are directional with labels. BFS/DFS iterators support depth limits and label filtering. |
| **Neural Index** | `src/neuron/` | Spreading activation network — each neuron represents a concept, fires when activation exceeds a threshold, spreads to connected neurons via synapses. Hebbian learning strengthens co-firing connections. No weight matrices, no matrix multiply — just f32 add/compare. |
| **Gremlin API** | `src/gremlin/` | Minimal Gremlin subset exposed as a JSON pipeline over HTTP. Steps: V, E, has, hasLabel, hasText, out(depth), in, both, values, limit, count, dedup, repeat, plus custom `neuralSearch`. |
| **Storage** | `src/storage/` | Disk-backed storage via subgraph partitioning. LRU cache loads blocks on demand. Write-Ahead Log (WAL) with CRC32 for crash recovery. Checkpoints flush dirty subgraphs and truncate the log. |
| **Extraction** | `src/extract/` | Markdown document → section splitting → LLM (OpenAI-compatible API) → extracted entities/relations → graph insert. Configurable context window, output token limit, retry. |
| **Config** | `src/config/` | File-based configuration from `~/.config/bionic-graph/settings.json`. Environment variable overrides. Auto-generates defaults on first run. |

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
       │  tick 2: Neuron("ML") fires → spreads further
       │  tick 3: no more firing → stabilize
       ▼
  Collect vertex_refs from fired neurons
       │  Neuron("AI") → vertex #42 (Alice), #88 (Acme Corp)
       │  Neuron("ML") → vertex #103 (Project X)
       ▼
  Gremlin traversal from starting vertices
       │  out("works_at") → find colleagues
       │  has("industry", "AI") → filter
       ▼
  Return ranked results
```

### Neural model vs. Transformer

| Attribute | **Bionic-Graph (spreading activation)** | **Transformer (self-attention)** |
|---|---|---|
| **Compute model** | Conditional branches + f32 add/compare — activation flows through a graph | Dense matrix multiplies (Q·Kᵀ·V) — all-pairs attention |
| **Learning** | Hebbian: co-firing strengthens connections — online, unsupervised | Backprop + SGD — offline, requires large pre-training datasets |
| **Training cost** | Zero — learns during normal usage | Millions of $ (GPT-4 class) |
| **Inference hardware** | Any CPU, even a single core | GPU required (or dedicated accelerators) |
| **Parameter count** | Bytes per neuron — a 10k-neuron net ≈ a few MB | Billions of params — hundreds of MB to GB |
| **Explainability** | High — every neuron maps to a named concept; activation paths are traceable | Low — attention weights are suggestive, not causal |
| **Knowledge update** | Instant — add/remove a vertex → create/update a neuron live | Retrain or fine-tune; weeks of engineering |
| **Semantic understanding** | None — "apple" (fruit) and "Apple" (company) are just string matches | Deep — contextual embeddings disambiguate by surrounding tokens |
| **Generative capability** | None — retrieves known graph paths, invents nothing | Strong — can synthesize novel text, code, images |
| **Determinism** | Deterministic — same input → same output | Non-deterministic (sampling, temperature) |
| **Cold start** | Zero — works from the first vertex added | Requires months of pre-training |

**Key takeaway:** Bionic-Graph is **not a Transformer replacement**. It is a graph index cache — cheap, fast, deterministic, instantly updatable. Use Transformers for deep semantics and generation; use Bionic-Graph as a fast, lightweight index layer over a knowledge graph.

---

## How to

### Clone & build

```bash
git clone <repo-url>
cd bionic-graph
cargo build --release          # optimised binary
cargo build                    # debug build (faster compile)
```

### Run

```bash
# Start the HTTP server with defaults
cargo run --release

# With auto-index and custom data directory
cargo run --release -- --auto-index --data-dir ./my-data

# With explicit config file path
cargo run --release -- --config ~/.config/bionic-graph/settings.json
```

#### CLI flags

| Flag | Default | Description |
|------|---------|-------------|
| `-d, --data-dir <DIR>` | from settings.json | Data directory for persistence (overrides config) |
| `-H, --host <HOST>` | from settings.json | HTTP bind address (overrides config) |
| `-P, --port <PORT>` | from settings.json | HTTP port (overrides config) |
| `-i, --auto-index` | true | Auto-create neurons from vertex labels on startup |
| `--no-auto-save` | off | Disable the background auto-save thread |
| `--config <PATH>` | `~/.config/bionic-graph/settings.json` | Path to config file |
| `-h, --help` | — | Show help |
| `-V, --version` | — | Show version |

### Settings reference

On first run, Bionic-Graph creates `~/.config/bionic-graph/settings.json` with these defaults:

#### `server` — HTTP server

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `host` | string | `"127.0.0.1"` | Bind address. Use `"0.0.0.0"` for all interfaces. |
| `port` | integer | `8080` | TCP port. |

#### `extraction` — Document knowledge extraction (LLM)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `api_base_url` | string | `"https://api.deepseek.com/v1"` | OpenAI-compatible chat completions endpoint. |
| `model` | string | `"deepseek-v4-flash"` | Model identifier sent in the API request. |
| `context_window` | integer | `65536` | Max context window in tokens. Adjust per model (GPT-4o = 128000). |
| `max_output_tokens` | integer | `8192` | Max tokens in the LLM response. |
| `max_retries` | integer | `3` | Number of retry attempts on API failure. |
| `concurrent_sections` | integer | `1` | Sections to process in parallel (1 = sequential, safe for rate limits). |
| `pass_section_context` | bool | `true` | Include previous section summary as context for the next LLM call. |

**API key** is read from environment variable `BGRAPH_EXTRACT_API_KEY` — never stored in the config file.

#### `storage` — Data persistence

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `data_dir` | string | `"data"` | Directory for subgraph files, index bundle, WAL, and neural network state. |
| `cache_capacity` | integer | `1000` | Max number of subgraphs kept in the LRU memory cache. |
| `checkpoint_interval_entries` | integer | `1000` | Trigger a checkpoint after this many WAL entries. |
| `auto_save_interval_secs` | integer | `5` | How often the background checkpoint thread checks for work. |

#### `graph` — Vertex/edge defaults

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `default_vertex_labels` | array[string] | `["entity"]` | Fallback label when creating a vertex without labels. |
| `max_edges_per_vertex` | integer | `10000` | Safety limit on incident edges per vertex. |

#### `neural` — Spreading activation defaults

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `default_threshold` | float | `0.7` | Default neuron firing threshold (0.0–1.0). |
| `default_decay_rate` | float | `0.1` | Per-tick activation decay (0.0–1.0). |
| `default_refractory_ticks` | integer | `3` | How many ticks a neuron rests after firing. |
| `learning_enabled` | bool | `true` | Enable Hebbian learning. |
| `co_fire_window` | integer | `5` | Tick window for detecting co-firing events. |

#### Environment variable overrides

| Variable | Overrides |
|----------|-----------|
| `BGRAPH_HOST` | `server.host` |
| `BGRAPH_PORT` | `server.port` |
| `BGRAPH_DATA_DIR` | `storage.data_dir` |
| `BGRAPH_EXTRACT_API_KEY` | API key (not in settings.json) |
| `BGRAPH_EXTRACT_BASE_URL` | `extraction.api_base_url` |
| `BGRAPH_EXTRACT_MODEL` | `extraction.model` |

### Use the API

#### Health check

```bash
curl localhost:8080/health
```

```json
{"status":"ok","vertices":42,"edges":128,"neurons":10,"total_ticks":57}
```

#### Add data

```bash
# Add a vertex
curl -X POST localhost:8080/vertices \
  -H 'Content-Type: application/json' \
  -d '{"labels":["person","engineer"],"properties":{"name":"Alice","age":30}}'

# Add an edge
curl -X POST localhost:8080/edges \
  -H 'Content-Type: application/json' \
  -d '{"label":"works_at","source":1,"target":3,"properties":{"since":2020}}'

# Create a neuron index
curl -X POST localhost:8080/neurons \
  -H 'Content-Type: application/json' \
  -d '{"label":"Artificial Intelligence","keywords":["AI","machine learning"],"vertex_refs":[1,2,3]}'
```

#### Neural search + graph traversal

```bash
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"neuralSearch","keywords":["AI","engineer"]},
    {"step":"out","label":"works_at"},
    {"step":"values","key":"name"},
    {"step":"dedup"},
    {"step":"limit","count":10}
  ]}'
```

#### Quick keyword search

```bash
curl -X POST localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d '{"query":"AI engineer"}'
```

#### Advanced Gremlin traversals

Depth-limited traversal (BFS up to N levels):

```bash
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"V"},
    {"step":"out","label":"knows","depth":3}
  ]}'
```

Text fuzzy matching (case-insensitive substring):

```bash
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"V"},
    {"step":"hasText","key":"name","pattern":"ali"}
  ]}'
```

Repeat traversal (execute sub-pipeline N times):

```bash
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[
    {"step":"V","ids":[1]},
    {"step":"repeat","times":3,"steps":[
      {"step":"out","label":"knows"}
    ]}
  ]}'
```

**Supported steps:**

| Step | Parameters | Description |
|------|-----------|-------------|
| `neuralSearch` | `keywords: [string]` | 🔥 Find vertices via neural index |
| `V` | `ids?: [number]` | All or specific vertices |
| `E` | `ids?: [number]` | All or specific edges |
| `has` | `key, value` | Exact property filter |
| `hasLabel` | `labels: [string]` | Label filter |
| `hasText` | `key, pattern` | Case-insensitive substring match on property |
| `out` | `label?: string, depth?: int` | Outgoing edges (depth=1 single level, depth=N BFS) |
| `in` | `label?: string, depth?: int` | Incoming edges |
| `both` | `label?: string, depth?: int` | Both directions |
| `values` | `key: string` | Extract property values |
| `limit` | `count: number` | Cap results |
| `count` | — | Count results |
| `dedup` | — | Deduplicate by ID |
| `repeat` | `times: int, steps: [step]` | Repeat sub-pipeline N times |

#### Document extraction (requires BGRAPH_EXTRACT_API_KEY)

```bash
curl -X POST localhost:8080/extract \
  -H 'Content-Type: text/markdown' \
  --data-binary @README.md
```

```json
{
  "success": true,
  "stats": {
    "total_sections": 18,
    "processed_sections": 18,
    "new_vertices": 34,
    "new_edges": 12,
    ...
  }
}
```

### Run the demo

```bash
cargo run --example demo
```

### Run tests

```bash
cargo test
```

---

## Project structure

```
src/
├── main.rs                    # CLI entry + HTTP server
├── lib.rs                     # Library exports
├── config/
│   ├── settings.rs            # Settings struct (5 sub-configs)
│   └── loader.rs              # File load + env override + default generation
├── graph/
│   ├── vertex.rs              # Vertex type
│   ├── edge.rs                # Edge type
│   ├── graph.rs               # Adjacency list implementation
│   └── traversal.rs           # BFS/DFS iterators
├── neuron/
│   ├── neuron.rs              # Neuron/Synapse structs
│   ├── network.rs             # Network orchestration
│   ├── activation.rs          # Spreading activation algorithm
│   └── learning.rs            # Hebbian learning
├── storage/
│   ├── disk_graph.rs          # Disk-backed Graph (SubgraphCache + WAL)
│   ├── index.rs               # VertexIndex / SubgraphIndex / LabelIndex
│   ├── subgraph.rs            # Subgraph data + serialization
│   ├── subgraph_cache.rs      # LRU cache + on-demand load
│   ├── redo_log.rs            # WAL + Checkpoint + crash recovery
│   └── partition.rs           # BFS clustering partitioner
├── persistence/
│   ├── graph_store.rs         # Graph persistence (legacy + disk)
│   ├── neuron_store.rs        # Neural network persistence
│   └── auto_save.rs           # Auto-save / checkpoint thread
├── gremlin/
│   ├── query.rs               # Gremlin query types
│   ├── steps.rs               # Step execution engine
│   └── server.rs              # REST API (axum)
├── extract/
│   ├── config.rs              # Extraction config (LLM endpoint / model / tokens)
│   ├── document.rs            # Markdown reader + section splitter
│   ├── llm_client.rs          # OpenAI-compatible API client
│   ├── extraction.rs          # Prompt templates + response parser
│   └── pipeline.rs            # Orchestrator: doc → LLM → entities → graph
└── memory_system.rs           # Top-level unified API
```

---

## Design principles

1. **Pure Rust** — zero external neural network libraries
2. **CPU inference** — all computation in memory, no GPU required
3. **Bio-inspired** — spreading activation mimics biological neurons (threshold, refractory, synapse, Hebbian learning)
4. **Low cost** — lightweight memory index, suitable for embedded and edge scenarios
5. **Gremlin-compatible** — standard graph query interface

---

## License

MIT
