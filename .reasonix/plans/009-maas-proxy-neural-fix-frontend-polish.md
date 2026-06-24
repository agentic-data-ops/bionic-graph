# Plan 009 — MaaS Proxy, Neural Search Fixes, Frontend Polish

## Summary
Backend OpenAI-compatible proxy, neural search keyword bugfix, frontend model selector rework, semantic search prompt optimization, UI Light mode polish.

## Changes

### Backend — MaaS Proxy (`src/maas/`)
- New `src/maas/` module with OpenAI-compatible proxy endpoints
  - `GET /maas/openai/v1/models` — returns model list (format `provider/model`) with `x-default-model` header
  - `POST /maas/openai/v1/chat/completions` — proxies to configured provider using stored api_key, supports SSE streaming
- Settings API security: `GET /settings` strips `api_key`; `PUT /settings` preserves existing key when empty
- Document extraction: supports `?model=Provider/Model` query param to override LLM model
- Routes registered in `src/gremlin/server.rs`; module exported from `src/lib.rs`

### Backend — Neural Search Bugfix
- **`with_keywords()` bug**: `Neuron::with_keywords()` was losing CJK keywords. Fixed by using direct field assignment `neuron.keywords = keywords` instead of `.with_keywords(keywords)` in `add_vertex_handler` (`src/gremlin/server.rs`)
- **Edge neuron cleanup on doc delete**: `DELETE /documents/:id?clean=true` now also removes edge neurons (`EntityType::Edge`), not just vertex neurons
- **Log messages**: `"Unified WAL"` → `"Redolog WAL"` across `redolog_wal.rs` and `graph_manager.rs`
- **Startup banner**: Updated with all API endpoints grouped by category (Knowledge Graph / Document Management / MaaS Proxy)

### Frontend — Model Selector Rework
- `fetchModels()` now reads `x-default-model` header from backend, returns `{ models, defaultModel }`
- `ChatInput.jsx`: model dropdown fetches from `/maas/openai/v1/models`, saves selection to `localStorage('bgraph-last-model')`, prefers saved model on init, falls back to backend default; re-fetches when `providers`/`defaultModelKey` props change
- `KnowledgeBase.jsx`: import dialog model selector uses `fetchModels()` instead of `providers` prop; passes selected model to `startDocumentExtraction(docId, graphName, model)`
- `App.jsx`: settings sync now sends `defaultModelKey` to `PUT /settings`; effect dependency includes `defaultModelKey`

### Frontend — Semantic Search Prompt
- `ChatArea.jsx`: optimized LLM filter prompt with structured field descriptions and rule 3 ("if you select an edge, ALSO select its source and target vertices")
- Tested with query "韩立的好友" → correctly returns 韩立, 海大少, 向之礼 vertices + their edges

### Frontend — SOURCE/TARGET Vertex Name Lookup
- `GraphViewer.jsx`: `InfoPanel` now also searches `nodesRef.current` (vis-network DataSet) for source/target vertex names, fixing `#id` fallback for expanded nodes
- Passed `nodesRef` as prop to `InfoPanel` component

### Frontend — Light Mode UI Polish
- `Sidebar.jsx`: selected item `text-white` → `text-[var(--text-primary)]`
- `ChatInput.jsx`: send button hardcoded colors → CSS variables `var(--accent)` / `var(--bg-hover)` / `var(--text-tertiary)`; checkbox border `#3a3a3e` → `var(--border)`
- `GraphViewer.jsx`: Light mode vertex background `#d1d1d6` → `#e8e8ed`, edge text `#8e8e93` → `#636366`, edge line `#c7c7cc` → `#aeaeb2`

### Dependencies Added
- `Cargo.toml`: `tokio-stream`, `futures-util`, `http-body`, `http-body-util`

### Files Changed
```
M  Cargo.lock
M  Cargo.toml
M  src/config/loader.rs
M  src/extract/document_extractor.rs
M  src/extract/pipeline.rs
M  src/extract/task_manager.rs
M  src/graph/edge.rs
M  src/graph_manager.rs
M  src/gremlin/query.rs
M  src/gremlin/server.rs
M  src/gremlin/steps.rs
M  src/lib.rs
M  src/main.rs
M  src/memory_system.rs
M  src/neuron/network.rs
M  src/storage/redolog_wal.rs
A  src/maas/mod.rs
A  src/maas/openai.rs
M  src/ui/src/App.jsx
M  src/ui/src/api.js
M  src/ui/src/components/ChatArea.jsx
M  src/ui/src/components/ChatInput.jsx
M  src/ui/src/components/GraphViewer.jsx
M  src/ui/src/components/KnowledgeBase.jsx
M  src/ui/src/components/SettingsDialog.jsx
M  src/ui/src/components/Sidebar.jsx
M  src/ui/src/locales/en.json
M  src/ui/src/locales/zh.json
```
