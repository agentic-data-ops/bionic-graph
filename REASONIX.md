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
| 步骤 | 说明 |
|------|------|
| `keywordSearch` | 神经网络关键词搜索，返回顶点+边 |
| `semanticSearch` | LLM 提取关键词 → keywordSearch → LLM 语义裁剪 |
| `V` / `E` | 取全部或指定顶点/边 |
| `has` / `hasNot` / `hasKey` / `hasValue` / `hasLabel` / `hasText` | 属性过滤 |
| `out` / `in` / `both` | 顶点遍历（支持 depth） |
| `outE` / `inE` / `bothE` | 边遍历（返回 EdgeResult） |
| `values` / `limit` / `count` / `dedup` | 结果处理 |
| `repeat` | 循环执行子步骤 |
| `timeTravel` | 时间点查询 |
| `compact` | 历史版本归档 |

## Watch out for
- **`edit_file` SEARCH must match byte-for-byte** — the Rust source has no trailing whitespace convention, and SEARCH is whitespace-sensitive.
- **`.reasonix/` is committed** — plans in `.reasonix/plans/` and outputs in `.reasonix/output/` are part of the repo.
- **`Vertex::update_properties(props, record_history)`** — second boolean param controls whether the old state is pushed to `_history`. Call sites must pass the graph's `time_travel_enabled` flag.
- **`Graph::remove_vertex(id, force)`** — when `force=false` and `time_travel_enabled=true`, performs soft-delete. Otherwise hard-delete.
- **`POST /vertices` and `POST /edges` auto-create neurons** — HTTP handlers call `Neuron::for_vertex` / `Neuron::for_edge` + `auto_synapse`.

## Implemented Plans
- `001-arch-verify.md` — 全功能验证（151 tests, 0 failed）
- `002-section-paragraph-graph.md` — 章节/段落结构入图（section/paragraph 顶点 + 分层边）
- `003-keyword-semantic-search.md` — keywordSearch + semanticSearch + 全局 LLM 配置
