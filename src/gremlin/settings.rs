//! Search configuration settings endpoint.
//!
//! Persisted in `~/.config/bionic-graph/settings.json` under the `"search"` key.
//! Editable via `GET/PUT /settings/search`.

use std::sync::Arc;

use axum::{extract::State, Json};
use serde::Deserialize;

use crate::config::settings::SearchSettings;
use crate::gremlin::AppState;

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
) -> Json<serde_json::Value> {
    let save_result = {
        let mut guard = state.settings.lock().unwrap();
        guard.search = new_settings;
        crate::config::loader::save_settings(&guard)
    };
    if let Err(e) = save_result {
        log::warn!("Failed to save search settings: {}", e);
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }
    Json(serde_json::json!({ "status": "ok" }))
}

// ── /settings/llm ───────────────────────────────────────────────────────────

use crate::config::settings::LlmConfig;

/// GET /settings/llm — return the full LLM config wrapped for frontend.
pub async fn get_llm_settings(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let settings = state.settings.lock().unwrap();
    Json(serde_json::json!({ "llm": &settings.llm }))
}

/// PUT /settings/llm — update LLM providers and default model, persist to disk.
#[derive(Deserialize)]
pub struct UpdateLlmBody {
    pub providers: Option<Vec<crate::config::settings::LlmProvider>>,
    pub default_model: Option<String>,
}

pub async fn update_llm_settings(
    State(state): State<AppState>,
    Json(body): Json<UpdateLlmBody>,
) -> Json<serde_json::Value> {
    let save_result = {
        let mut guard = state.settings.lock().unwrap();
        if let Some(providers) = body.providers {
            guard.llm.providers = providers;
        }
        if let Some(model) = body.default_model {
            guard.llm.default_model = model;
        }
        crate::config::loader::save_settings(&guard)
    };
    if let Err(e) = save_result {
        log::warn!("Failed to save LLM settings: {}", e);
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }
    Json(serde_json::json!({ "status": "ok" }))
}

// ── /settings/rank ──────────────────────────────────────────────────────────

use crate::config::settings::RankConfig;

/// GET /settings/rank — return the current rank config.
pub async fn get_rank_settings(
    State(state): State<AppState>,
) -> Json<RankConfig> {
    let settings = state.settings.lock().unwrap();
    Json(settings.rank.clone())
}

/// PUT /settings/rank — update rank config and persist to disk.
pub async fn update_rank_settings(
    State(state): State<AppState>,
    Json(new_config): Json<RankConfig>,
) -> Json<serde_json::Value> {
    let save_result = {
        let mut guard = state.settings.lock().unwrap();
        guard.rank = new_config;
        crate::config::loader::save_settings(&guard)
    };
    if let Err(e) = save_result {
        log::warn!("Failed to save rank settings: {}", e);
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }
    Json(serde_json::json!({ "status": "ok" }))
}
