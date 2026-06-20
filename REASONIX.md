# Bionic-Graph — REASONIX.md

## Stack
- **Language**: Rust 2021 edition
- **Web framework**: axum 0.7 (REST API, JSON, CORS)
- **Serialization**: serde + serde_json + bincode (binary persistence)
- **CLI**: clap 4 (derive)
- **Async runtime**: tokio (full)
- **Config**: `~/.config/bionic-graph/settings.json`, auto-generated on first run

## Layout
- `src/graph/` — Vertex/Edge/Graph types, MVCC versioning, BFS/DFS traversal
- `src/neuron/` — Spreading activation network, Hebbian learning, `EntityType` (Vertex/Edge per neuron)
- `src/storage/` — Disk-backed storage: subgraph partitioning, LRU cache, WAL (redo_log), version log (vlog), compaction
- `src/gremlin/` — REST API routes + Gremlin JSON pipeline step engine (16 steps)
- `src/extract/` — Markdown document → LLM extraction → graph insert + section/paragraph structure
- `src/config/` — Settings struct (serde) + loader with env override
- `src/persistence/` — graph_store/neuron_store serialization + auto-save thread
- `src/graph_manager.rs` — Multi-graph manager (HashMap<String, GraphHandle>)
- `src/memory_system.rs` — Legacy single-graph wrapper (backward compat)

## Commands
- **build**: `cargo build`
- **release**: `cargo build --release`
- **test**: `cargo test` (151 unit tests)
- **run**: `cargo run --release`
- **demo**: `cargo run --example demo`

## Conventions
- **Config**: settings.json sections map to Rust structs in `src/config/settings.rs`. Env vars `BGRAPH_*` and `BGRAPH_LLM_*` override file values (`BGRAPH_EXTRACT_*` also accepted for backward compat).
- **Multi-graph**: all data endpoints read `X-Graph-Name` header (default: `"default"`). Graph data lives under `data/{name}/`.
- **Time travel**: optional per graph (`create_with_opts(name, time_travel=true)`). Internal fields prefixed `_` (`_version`, `_updated_at`, `_is_deleted`, `_history`). Controlled by `Graph.time_travel_enabled`.
- **Version log**: compacted history written to `data/{name}/version_log/*.vlog` (v2 format with sparse index, v1 backward compatible).
- **Neural IDs**: all use `u64` (`VertexId`, `EdgeId`, `SubgraphId`, `NeuronId`).
- **EntityType**: `Neuron` stores `entity_type: Option<EntityType>` identifying the graph entity (Vertex or Edge) it represents.

## Gremlin Steps (16 total)
| Step | Description |
|------|-------------|
| `keywordSearch` | Neural index search, returns vertices + edges |
| `semanticSearch` | LLM keywords → keywordSearch → LLM result filter |
| `V` / `E` | All or specific vertices / edges |
| `has` / `hasNot` / `hasKey` / `hasValue` / `hasLabel` / `hasText` | Property filters |
| `out` / `in` / `both` | Vertex traversal (supports depth) |
| `outE` / `inE` / `bothE` | Edge traversal (returns EdgeResult) |
| `values` / `limit` / `count` / `dedup` | Result processing |
| `repeat` | Loop sub-steps N times |
| `timeTravel` | Point-in-time query |
| `compact` | Archive old history to vlog |

## Watch out for
- **`edit_file` SEARCH must match byte-for-byte** — the Rust source has no trailing whitespace convention, and SEARCH is whitespace-sensitive.
- **`.reasonix/` is committed** — plans in `.reasonix/plans/` and outputs in `.reasonix/output/` are part of the repo.
- **`Vertex::update_properties(props, record_history)`** — second boolean param controls whether the old state is pushed to `_history`. Call sites must pass the graph's `time_travel_enabled` flag.
- **`Graph::remove_vertex(id, force)`** — when `force=false` and `time_travel_enabled=true`, performs soft-delete. Otherwise hard-delete.
- **`POST /vertices` and `POST /edges` auto-create neurons** — HTTP handlers call `Neuron::for_vertex` / `Neuron::for_edge` + `auto_synapse`.

## Implemented Plans
- `001-arch-verify.md` — Full feature verification (151 tests, 0 failed)
- `002-section-paragraph-graph.md` — Section/paragraph graph structure
- `003-keyword-semantic-search.md` — keywordSearch + semanticSearch + global LLM config
