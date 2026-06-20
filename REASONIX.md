# Bionic-Graph — REASONIX.md

## Stack
- **Language**: Rust 2021 edition
- **Web framework**: axum 0.7 (REST API, JSON, CORS) — uses `:param` path syntax
- **Serialization**: serde + serde_json + bincode (binary persistence)
- **CLI**: clap 4 (derive)
- **Async runtime**: tokio (full)
- **Config**: `~/.config/bionic-graph/settings.json`, auto-generated on first run

## Layout
- `src/graph/` — Vertex/Edge/Graph types, MVCC versioning, BFS/DFS traversal
- `src/neuron/` — Spreading activation network, Hebbian learning, `EntityType` (Vertex/Edge per neuron)
- `src/storage/` — Disk-backed storage: subgraph partitioning, LRU cache, WAL (redo_log), version log (vlog), compaction
- `src/gremlin/` — REST API routes + Gremlin JSON pipeline step engine (16 steps)
- `src/extract/` — Markdown document → LLM extraction (batch/concurrent) → graph insert + section/paragraph structure
  - `task_manager.rs` — Async task lifecycle (pending → running → completed/failed), UUID-based task tracking with progress
- `src/config/` — Settings struct (serde) + loader with env override
- `src/persistence/` — graph_store/neuron_store serialization + auto-save thread
- `src/graph_manager.rs` — Multi-graph manager (HashMap<String, GraphHandle>)
- `src/memory_system.rs` — Legacy single-graph wrapper (backward compat)

## Commands
- **build**: `cargo build`
- **release**: `cargo build --release`
- **test**: `cargo test` (159 unit tests)
- **run**: `cargo run`
- **demo**: `cargo run --example demo`

## Extraction Config (`settings.json` → `extraction`)
| Field | Default | Description |
|-------|---------|-------------|
| `api_base_url` | `https://api.deepseek.com/v1` | OpenAI-compatible endpoint |
| `model` | `deepseek-v4-flash` | Model identifier |
| `context_window` | 65536 | Max tokens per call |
| `max_output_tokens` | 16384 | Max tokens in LLM response |
| `max_retries` | 3 | Retries on API failure |
| `concurrent_sections` | 3 | Parallel LLM call limit (via tokio semaphore) |
| `pass_section_context` | true | Pass previous section summary as context |
| `batch_size` | 5 | Sections per LLM call (reduces API calls ~5x) |

Env overrides: `BGRAPH_LLM_API_KEY` (required), `BGRAPH_EXTRACT_*` for backward compat.

## Extraction Pipeline
1. **Input**: Markdown text → `split_sections()` → heading-based section split
2. **Batching**: Sections grouped into batches of `batch_size` (default 5)
3. **Concurrent**: Batches run in parallel up to `concurrent_sections` via tokio semaphore
4. **LLM call**: Each batch sends one prompt listing all sections, expects JSON array response
5. **Parse**: `parse_batch_response()` extracts per-section `SectionExtraction` from JSON array
6. **Fallback**: On batch parse failure, falls back to per-section individual LLM calls
7. **Graph insert**: Each extraction's entities → vertices (with entity-specific neurons), relations → edges
8. **Progress**: `ProgressCallback` fires after each section, updates task via `task_manager.rs`

## Gremlin Steps (16 total)
| Step | Description |
|------|-------------|
| `keywordSearch` | Neural index search — **only returns vertices from matched/activated neurons** (inactive neurons filtered out). Capped at 100 results. |
| `semanticSearch` | LLM keywords → keywordSearch → LLM result filter |
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
| POST | `/search` | Quick neural keyword search |
| POST | `/vertices`, `/edges` | Add vertex/edge (auto-creates neurons) |
| POST | `/neurons`, `/neurons/:id/link`, `/neurons/:id/synapse` | Neural network management |
| **POST** | **`/extract`** | **Submit async extraction → returns `{task_id, status}`** |
| **GET** | **`/extract/task/:task_id`** | **Poll task status + progress + results** |
| **GET** | **`/extract/tasks`** | **List all extraction tasks (newest first)** |
| POST | `/compact` | Trigger history compaction |

## Watch out for
- **`edit_file` SEARCH must match byte-for-byte** — the Rust source has no trailing whitespace convention, and SEARCH is whitespace-sensitive.
- **`.` — `.reasonix/` is committed** — plans in `.reasonix/plans/` and outputs in `.reasonix/output/` are part of the repo.
- **`Vertex::update_properties(props, record_history)`** — second boolean param controls whether the old state is pushed to `_history`. Call sites must pass the graph's `time_travel_enabled` flag.
- **`Graph::remove_vertex(id, force)`** — when `force=false` and `time_travel_enabled=true`, performs soft-delete. Otherwise hard-delete.
- **`POST /vertices` and `POST /edges` auto-create neurons** — HTTP handlers call `Neuron::for_vertex` / `Neuron::for_edge` + `auto_synapse`.
- **Route params use `:param` syntax** — axum 0.7.9 requires `:param` (not `{param}`) for path parameters in `.route()`.
- **`keywordSearch` filters inactive neurons** — `activation.rs` only collects vertex refs from neurons with `activation > 0`. Entity-specific neurons are created during extraction with the entity name + labels as keywords.
- **Batch extraction can truncate** — if `max_output_tokens` is too low for a batch's combined output, JSON truncation occurs. Setting of 16384 handles 5-section batches comfortably. Falls back to per-section on parse error.

## Implemented Plans
- `001-arch-verify.md` — Full feature verification (151 tests, 0 failed)
- `002-section-paragraph-graph.md` — Section/paragraph graph structure
- `003-keyword-semantic-search.md` — keywordSearch + semanticSearch + global LLM config
