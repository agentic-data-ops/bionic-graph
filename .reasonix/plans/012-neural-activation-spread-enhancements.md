# Plan 012 — Neural Activation Spread Enhancements

## Summary

Complete the spreading activation feedback loop: enable spread-activated neurons in search results, improve synapse strength for deeper propagation, add configurable mode-specific thresholds, and enrich edge search results with source/target vertices. Frontend: rename tab, 3-decimal float display, use `/settings/llm` endpoint.

## Changes

### Backend

| File | Change |
|------|--------|
| `src/neuron/activation.rs` | `is_spread_active = false` → `spread_recipients.contains(&neuron.id)` so spread-activated neurons appear in search results |
| `src/neuron/activation.rs` | Both Exact/Greedy modes use `in_direct \|\| is_spread_active` (previously Exact was `in_direct` only) |
| `src/neuron/activation.rs` | Add `greedy_threshold`/`exact_threshold` fields to `ActivationConfig`, replace hardcoded 0.6/0.8 with config values, save/restore original thresholds to avoid side effects |
| `src/neuron/network.rs` | `auto_synapse` default strength `0.5` → `0.8` (enables single-hop propagation past threshold 0.7) |
| `src/gremlin/steps.rs` | Remove Exact-mode vertex-level post-filter (`query_tokens_lower` always empty) |
| `src/gremlin/steps.rs` | When edge neurons are matched, also add source + target vertices to results (dedup against existing vertex results) |
| `src/config/settings.rs` | Add `greedy_threshold: f32` (default 0.6) and `exact_threshold: f32` (default 0.8) to `SearchConfig` |
| `src/graph_manager.rs` | Map `nc.search.greedy_threshold`/`exact_threshold` to `ActivationConfig` in `neural_to_configs()` |
| `src/gremlin/server.rs` | Expose `greedy_threshold`/`exact_threshold` in GET /settings/neural; read them in PUT /settings/neural; add `/settings/llm` routes (GET + PUT) aliasing `/settings` |

### Frontend

| File | Change |
|------|--------|
| `src/ui/src/api.js` | Rename `fetchSettings` → `fetchLlmSettings`, `updateSettings` → `updateLlmSettings`; endpoints changed from `/settings` to `/settings/llm` |
| `src/ui/src/App.jsx` | Update import and call sites for renamed functions |
| `src/ui/src/components/SettingsDialog.jsx` | Tab label `搜索` → `神经元`; add `greedy_threshold`/`exact_threshold` input fields under "搜索模式激活阈值" section; all float inputs use `f3()` helper (`.toFixed(3)`) with `step="0.001"` |

### Side Effect

`ActivationConfig` struct change broke backward compatibility with old `neural.bin` — data directory was cleared and re-imported. Added `#[serde(default)]` on new fields to prevent future breaks.

## Files Changed (9 total)

```
M src/config/settings.rs
M src/graph_manager.rs
M src/gremlin/server.rs
M src/gremlin/steps.rs
M src/neuron/activation.rs
M src/neuron/network.rs
M src/ui/src/App.jsx
M src/ui/src/api.js
M src/ui/src/components/SettingsDialog.jsx
```
