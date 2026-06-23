# Session Plan: Search Mode, Theme Refactor, Document Extraction, Vertex Fields

## Summary
Multi-session overhaul adding built-in vertex fields, backend document extraction, CSS theme system, greedy/exact search modes, Playwright e2e tests, and numerous frontend fixes.

## Changes

### Backend — Vertex & Document
- **Built-in `document` field**: Added `document: String` to `Vertex`, `VersionRecord`, `VertexResult` — stores source document ID
- **Frontend extraction → Backend API**: Replaced frontend LLM calls with backend `POST /documents/:id/extract` async task pipeline with step progress
- **Remove `source_file` from properties**: Replaced with built-in `document` field, set on extraction, used in document-delete matching
- **Fixed nid collision bug**: `document_extractor.rs` neurons all got same nid=19 due to `(nn.neuron_count()+1)` in separate lock scopes — fixed by pre-computing `start_nid` before loop

### Backend — Search
- **Search step returns full vertex data**: `TraversalStep::Search` now looks up vertices from graph via `g.get_vertex(vid)` instead of creating synthetic empty-name results
- **Greedy/Exact search modes**: `SearchMode` enum (`Greedy`/`Exact`). Greedy matches ANY keyword (current behavior), Exact requires ALL keywords to match. Threshold behavior unchanged (activation propagation through synapses still works in both modes)
- **`/search` endpoint**: Supports `"mode": "greedy" | "exact"` from request body
- **`#[serde(default)]` on `search_mode`**: Backward compat with old config serialization

### Backend — Infrastructure
- **Neuron struct: Added `Clone, Debug, Serialize, Deserialize` derives** — required for new SearchMode enum and existing clone() calls
- **`SearchMode` enum**: Added `Default` impl (= `Greedy`), exported via `pub use neuron::SearchMode` in `mod.rs`

### Frontend — Theme (CSS Variables)
- **Removed all hardcoded colors**: Replaced `bg-[#xxx]`/`text-[#xxx]` with `bg-[var(--xxx)]`/`text-[var(--xxx)]` across all components
- **`index.css`: Added `:root` (dark) and `.light` CSS variable blocks** — backgrounds, text, borders, accents, scrollbars
- **`App.jsx`**: Toggles `dark`/`light` class on `<html>`, light mode now renders instantly without conditional className chains
- **vis-network**: Replaced `DARK_OPTIONS` constant with per-theme `DARK_OPTS`/`LIGHT_OPTS` selection in useEffect (dependency: `[data, graph, theme]`)

### Frontend — vis-network Display
- **Search result node labels**: Changed `item.properties?.name` to `item.name` (name is now top-level)
- **Theme prop threading**: `theme` prop added to `MessageList` → `ChatMessage` → `GraphViewer`
- **Fix focus-loss on custom property key edit**: React `key` changed from property key `k` to array index `idx`
- **Remove stale `_name`/`_keywords` hack**: Replaced with separate `localName`/`localKeywords` state, `editProps` now only contains custom properties
- **Add `localName`/`localKeywords` to `saveEdit` deps**: Missing deps caused `useCallback` to capture stale `""` values

### Frontend — Chat & Messages
- **Language switcher**: Changed from toggle button (EN/中文) to dropdown with `LANG` trigger, option list 中文/English, current language highlighted
- **ChatInput forwardRef**: Added `forwardRef` + `useImperativeHandle` to expose `focus()` method; ChatArea calls `chatInputRef.current?.focus()` after response completes
- **Edge `ref` parameter fix**: `forwardRef` was missing the `ref` second param, causing `ref is not defined` error

### Frontend — i18n
- Added `chat.greedy`, `chat.exact`, `chat.kwModeHint` keys for search mode labels
- Chinese: "贪婪搜索", "精确搜索"; English: "Greedy", "Exact"

### Testing
- **Playwright e2e test**: `src/ui/test/e2e/basic-load.mjs` — loads frontend, checks for JS errors, verifies theme toggle

## Files Changed (29 files, +647/-344)

| File | Change |
|------|--------|
| `src/graph/vertex.rs` | Add `document` field |
| `src/gremlin/steps.rs` | Search returns full vertex data, debug logs |
| `src/gremlin/query.rs` | Add `mode: Option<String>` to `Search` step |
| `src/gremlin/server.rs` | `/search` endpoint accepts `mode`, pass to Gremlin |
| `src/neuron/neuron.rs` | `SearchMode` enum + `Default` + Neuron derives |
| `src/neuron/activation.rs` | `match_keywords` takes `SearchMode`, exact mode logic |
| `src/neuron/network.rs` | `set_search_mode()` method |
| `src/neuron/mod.rs` | Re-export `SearchMode` |
| `src/extract/document_extractor.rs` | Set `v.document`, remove `source_file`, fix nid bug |
| `src/extract/task_manager.rs` | Pass doc_id to extractor |
| `src/memory_system.rs` | Add `mode: None` to Search step |
| `src/ui/src/index.css` | CSS variable theme system |
| `src/ui/src/App.jsx` | Theme toggle, language toggle signature |
| `src/ui/src/components/*.jsx` (10 files) | Theme vars, forwardRef, editProps refactor, lang dropdown, ChatInput focus, MessageList chatMessage theme |
| `src/ui/src/locales/*.json` | Search mode i18n keys |
| `src/ui/test/e2e/basic-load.mjs` | Playwright e2e test |
| `src/ui/package.json` | `@playwright/test` dependency |
