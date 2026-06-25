# Session Plan: Soft-Delete, Time Travel, Neuron Auto-Management, Frontend Features

> Date: 2025-07-11
> Branch: dev

## Summary

Comprehensive refactoring of the document extraction pipeline, unified vertex/edge creation with automatic neuron management, soft-delete with time-travel awareness, frontend graph viewer enhancements, and default graph rename.

---

## Changes

### 1. Soft-Delete & Time Travel System

**Problem**: Soft-deleted vertices' neurons were hard-removed, preventing time-travel recovery. Edges were always hard-deleted even when vertices were soft-deleted.

**Changes**:
- `src/neuron/neuron.rs`: Added `_deleted_at` field, `mark_deleted()` (idempotent), `is_deleted_at(search_at)` method
- `src/graph/graph.rs`: Added `soft_delete_edge()` method; `get_edge()` now filters soft-deleted edges when `time_travel_enabled`
- `src/gremlin/server.rs`: `delete_vertex_handler` uses `soft_delete_edge` when `!force`; neuron marked instead of removed; `DELETE /edges/{id}` endpoint added
- `src/neuron/activation.rs`: `tick()` and `search()` skip deleted neurons at all phases (matching, firing, propagation)
- `src/neuron/network.rs`: `search()` accepts `search_at` parameter for time-aware queries
- `src/gremlin/query.rs`: `Search` step gains optional `at` field
- `src/gremlin/steps.rs`: Search auto-injects timestamp from following `timeTravel` step; uses `get_edge_including_deleted` / `get_vertex_including_deleted` when `search_at` is set
- `src/ui/`: Timezone conversion (`localDatetimeToUTC`), timeTravel Gremlin step wired into frontend pipeline
- Fixed `from_history()` to use `record.name` and `record.keywords` (was hardcoded to empty)

### 2. Unified Vertex/Edge Creation with Auto Neuron Management

**Problem**: Document extraction, HTTP handlers, and internal code each had separate logic for creating neurons alongside vertices/edges, causing inconsistencies.

**Changes**:
- `src/graph_manager.rs`: Added `add_vertex_to_graph()` and `add_edge_to_graph()` methods — atomically create graph entity + neuron + WAL in one call
- `src/gremlin/server.rs`: `add_vertex_handler` and `add_edge_handler` now delegate to `GraphManager` methods
- `src/extract/document_extractor.rs`: Completely rewritten — uses `GraphManager` API instead of direct graph+neural access

### 3. Document Extraction Refactoring

**Problem**: Large docs failed with `DOCUMENT_TOO_LARGE`; extracted entities had missing keywords (vertex name not in neuron keywords).

**Changes**:
- Auto-split by chapters when content exceeds token limits (for both tags and entity extraction)
- Merge tags and entities from multiple LLM calls
- Dedup entities by name (merge keywords, merge property keys, ignore value differences)
- Tags extracted in same LLM call as entities (removed separate tag extraction step)
- Prompt updated to include `tags` field in output structure
- Extraction uses `GraphManager::add_vertex_to_graph()` which always includes vertex name in neuron keywords

### 4. Graph Viewer Frontend Enhancements

- **Search box**: Fuzzy label search over nodes/edges with dropdown, select to focus/select
- **Edge label display**: Source → target shown in search dropdown
- **Read-only mode**: In time-travel view, edit/delete buttons hidden
- **Edge edit/delete**: InfoPanel shows edit+delete for edges; custom modal with hard-delete checkbox
- **Add Vertex/Edge toolbar**: `+Vertex` and `+Edge` buttons (non-time-travel only); edge source/target selectable from existing nodes; property key-value pairs supported
- **Expand step**: New Gremlin `expand` step returns neighbor vertices + connected edges in one query
- **timeTravelAt propagation**: Stored in conversation messages, passed to GraphViewer, used in `traverse()` for double-click expansion

### 5. Default Graph Rename

- Default graph renamed from `"default"` to `"graph0"`
- Default time-travel enabled
- Delete guard updated

### 6. Bug Fixes

- `from_history()` in `vertex.rs`: hardcoded `name: ""` and `keywords: []` — fixed to use record values
- `neuron.rs` `match_keywords()`: removed inverted `token.contains(k.as_str())` substring matching
- Edge delete button: used undefined `setConfirmDeleteEdge` in `InfoPanel` — fixed with `onDeleteEdge` prop
- Edge label editing: used `item.labels` (array, vertex-only) instead of `item.label` (string) for initial edit value
- `mark_deleted()`: made idempotent to preserve original `_deleted_at`
- `GraphViewer` search: `net.focus()` on edge ID crashes vis-network — fixed to focus on source node
- Various borrow checker fixes

### 7. Tests

- `src/ui/test/e2e/stop-button.mjs`: Playwright test for chat stop button
- `src/ui/test/e2e/add-vertex-edge.mjs`: Playwright test for add vertex/edge modals

---

## Files Changed

```
src/neuron/neuron.rs          — _deleted_at, mark_deleted, is_deleted_at, match_keywords fix
src/neuron/activation.rs      — search_at param, deleted neuron filtering in tick+search
src/neuron/network.rs         — search_at param, tick search_at
src/graph/graph.rs            — soft_delete_edge, get_edge filters deleted, get_edge_including_deleted
src/graph/vertex.rs           — from_history fix
src/graph/edge.rs             — soft_delete
src/graph_manager.rs          — add_vertex_to_graph, add_edge_to_graph
src/gremlin/server.rs         — DELETE /edges/{id}, unified add handlers, force query param
src/gremlin/query.rs          — Search.at field, Expand step
src/gremlin/steps.rs          — Expand step, search_at injection, E step filtered
src/extract/document_extractor.rs — Rewritten: split, dedup, GraphManager API
src/extract/task_manager.rs   — Updated to use GraphManager
src/memory_system.rs          — graph0 rename
src/ui/src/api.js             — deleteEdge, traverse at
src/ui/src/components/ChatArea.jsx    — timeTravelAt, timezone conversion
src/ui/src/components/GraphViewer.jsx — Search, add V/E, edge edit/delete, readOnly, expand
src/ui/src/components/MessageList.jsx — timeTravelEnabled/At props
src/ui/src/locales/en.json, zh.json  — New translation keys
src/ui/test/e2e/*.mjs         — Playwright tests
```
