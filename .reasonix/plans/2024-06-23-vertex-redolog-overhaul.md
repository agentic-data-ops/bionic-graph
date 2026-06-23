# Plan: Full-stack Feature Updates (Session 2026-06-23)

## Changes

### 1. Directory Structure
- Graphs moved to `data/graphs/<name>/` (was `data/<name>/`)
- Documents stored under `data/documents/YYMMDD/<id>.md` (was flat)
- `UnifiedWal` renamed to `RedologWal`, file `redolog.wal` (was `unified.wal`)

### 2. Vertex Built-in Fields
- Added `name: String` (required) and `keywords: Vec<String>` as built-in fields on `Vertex`
- `name` removed from custom `properties` — it's now a top-level struct field
- `VertexResult` API response includes `name` and `keywords` at top level
- `POST /vertices` requires `name`, accepts optional `keywords`
- `PUT /vertices/:id` accepts `name`/`keywords` as optional fields
- Frontend GraphViewer displays Name / Keywords as built-in (non-deletable) fields
- Custom properties shown separately with add/delete support

### 3. Neuron Keywords Sync
- When creating/updating a vertex, neuron keywords = labels + name + keywords
- LLM extraction prompt tells model to provide 0-5 search keywords per entity
- Keywords from LLM are stored in `vertex.keywords` and synced to neuron

### 4. WAL (Crash Recovery)
- Created `src/storage/redolog_wal.rs` — single file atomic WAL for both graph + neuron ops
- Replaced separate `graph_wal` + `neuron_wal` with unified `RedologWal`
- All mutations write both graph + neuron entry in one `write_all + sync_all` call
- On recovery, replay both graph and neuron entries atomically

### 5. Signal Handling
- `main.rs`: graceful shutdown via `axum::serve.with_graceful_shutdown()`
- Handles SIGINT (Ctrl+C) and SIGTERM
- On signal: finish in-flight requests → `save_all()` → exit cleanly

### 6. Frontend Improvements
- Sidebar collapsible (w-64 ↔ w-12)
- Model selector moved to leftmost position
- Chat model selection persisted to localStorage (`settings.chatModel`)
- Default model marked with `(default)` in dropdown
- GraphViewer InfoPanel supports editing name/keywords/custom properties
- Add/delete custom properties in edit mode
- Delete vertex button with confirmation
- i18n support for all new UI text

### 7. Rust Warning Cleanup
- Removed ~38 compiler warnings (unused imports, variables, mut, results, dead code)

## Files Changed
- New: `src/storage/redolog_wal.rs`
- Removed: `src/storage/graph_wal.rs`, `src/storage/neuron_wal.rs`
- Modified: ~30 source files across backend + frontend
