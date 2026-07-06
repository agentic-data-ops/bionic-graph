//! Search configuration settings endpoint.
//!
//! Persisted in `~/.config/bionic-graph/settings.json` under the `"search"` key.
//! Editable via `GET/PUT /settings/search`.
//! A backward-compat wrapper at `/settings/neural` is provided for the
//! transition period.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;

use crate::config::settings::SearchSettings;
use crate::gremlin::AppState;

/// Old neural settings shape (for backward compatibility).
#[derive(Deserialize)]
pub struct OldNeuralSettings {
    search_mode: Option<String>,
    greedy_threshold: Option<f32>,
    exact_threshold: Option<f32>,
    max_results: Option<u32>,
    hebbian_learning: Option<bool>,
    co_fire_window: Option<u32>,
    synaptic_decay: Option<f32>,
    plasticity: Option<f32>,
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// GET /settings/search
pub async fn get_search_settings(
    State(state): State<AppState>,
) -> Json<SearchSettings> {
    let settings = state.settings.lock().unwrap();
    Json(settings.search.clone())
}

/// PUT /settings/search — update search settings and persist to disk.
pub async fn update_search_settings(
    State(state): State<AppState>,
    Json(new_settings): Json<SearchSettings>,
) -> StatusCode {
    let save_result = {
        let mut guard = state.settings.lock().unwrap();
        guard.search = new_settings;
        crate::config::loader::save_settings(&guard)
    };
    if let Err(e) = save_result {
        log::warn!("Failed to save search settings: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::OK
}

/// GET /settings/neural — backward-compat wrapper.
pub async fn get_neural_settings(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let settings = state.settings.lock().unwrap();
    let g = &settings.search.greedy;
    let e = &settings.search.exact;
    let value = serde_json::json!({
        "search_mode": "greedy",
        "greedy_threshold": g.activate,
        "exact_threshold": e.activate,
        "max_results": 100,
        "hebbian_learning": null,
        "co_fire_window": null,
        "synaptic_decay": null,
        "plasticity": null,
    });
    Json(value)
}

/// PUT /settings/neural — backward-compat wrapper.
pub async fn update_neural_settings(
    State(state): State<AppState>,
    Json(old): Json<OldNeuralSettings>,
) -> StatusCode {
    let save_result = {
        let mut guard = state.settings.lock().unwrap();
        if let Some(t) = old.greedy_threshold {
            guard.search.greedy.activate = t;
        }
        if let Some(t) = old.exact_threshold {
            guard.search.exact.activate = t;
        }
        crate::config::loader::save_settings(&guard)
    };
    if let Err(e) = save_result {
        log::warn!("Failed to save search settings: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::OK
}
