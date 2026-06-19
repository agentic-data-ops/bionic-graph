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
- `src/neuron/` — Spreading activation network, Hebbian learning (no NN deps)
- `src/storage/` — Disk-backed storage: subgraph partitioning, LRU cache, WAL (redo_log), version log (vlog), compaction
- `src/gremlin/` — REST API routes + Gremlin JSON pipeline step engine
- `src/extract/` — Markdown document → LLM extraction → graph insert pipeline
- `src/config/` — Settings struct (serde) + loader with env override
- `src/persistence/` — graph_store/neuron_store serialization + auto-save thread
- `src/graph_manager.rs` — Multi-graph manager (HashMap<String, GraphHandle>)
- `src/memory_system.rs` — Legacy single-graph wrapper (backward compat)

## Commands
- **build**: `cargo build`
- **release**: `cargo build --release`
- **test**: `cargo test` (unit tests colocated in each module via `#[cfg(test)]`)
- **run**: `cargo run --release`
- **demo**: `cargo run --example demo`

## Conventions
- **Config**: settings.json sections map to Rust structs in `src/config/settings.rs`. Env vars `BGRAPH_*` and `BGRAPH_EXTRACT_*` override file values.
- **Multi-graph**: all data endpoints read `X-Graph-Name` header (default: `"default"`). Graph data lives under `data/{name}/`.
- **Time travel**: optional per graph (`create_with_opts(name, time_travel=true)`). Internal fields prefixed `_` (`_version`, `_updated_at`, `_is_deleted`, `_history`). Controlled by `Graph.time_travel_enabled`.
- **Version log**: compacted history written to `data/{name}/version_log/*.vlog` (v2 format with sparse index, v1 backward compatible).
- **Neural IDs**: all use `u64` (`VertexId`, `EdgeId`, `SubgraphId`, `NeuronId`).

## Watch out for
- **`edit_file` SEARCH must match byte-for-byte** — the Rust source has no trailing whitespace convention, and SEARCH is whitespace-sensitive.
- **No `cargo` in this environment** — compilation can only be verified locally. Code changes rely on static analysis.
- **`.reasonix/` is committed** — plans in `.reasonix/plans/` and outputs in `.reasonix/output/` are part of the repo.
- **`Vertex::update_properties(props, record_history)`** — second boolean param controls whether the old state is pushed to `_history`. Call sites must pass the graph's `time_travel_enabled` flag.
- **`Graph::remove_vertex(id, force)`** — when `force=false` and `time_travel_enabled=true`, performs soft-delete. Otherwise hard-delete.
