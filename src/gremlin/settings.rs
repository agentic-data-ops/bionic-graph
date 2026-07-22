//! Search configuration settings endpoint.
//!
//! Persisted in `~/.config/bionic-graph/settings.json` under the `"graph.search"` key.
//! Editable via `GET/PUT /settings/graph/search`.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::config::settings::SearchSettings;
use crate::gremlin::AppState;

// ── Handlers ────────────────────────────────────────────────────────────────

/// GET /settings/search
pub async fn get_search_settings(
    State(state): State<AppState>,
) -> Json<SearchSettings> {
    let settings = state.settings.lock().unwrap();
    Json(settings.graph.search.clone())
}

/// PUT /settings/search — update search settings and persist to disk.
pub async fn update_search_settings(
    State(state): State<AppState>,
    Json(new_settings): Json<SearchSettings>,
) -> Json<serde_json::Value> {
    let save_result = {
        let mut guard = state.settings.lock().unwrap();
        guard.graph.search = new_settings;
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

use crate::config::settings::{RankConfig, WebSearchConfig};

/// GET /settings/web-search — return the web search config.
pub async fn get_web_search_settings(
    State(state): State<AppState>,
) -> Json<WebSearchConfig> {
    let settings = state.settings.lock().unwrap();
    Json(settings.web_search.clone())
}

/// PUT /settings/web-search — update web search config and persist.
pub async fn update_web_search_settings(
    State(state): State<AppState>,
    Json(new_config): Json<WebSearchConfig>,
) -> Json<serde_json::Value> {
    let save_result = {
        let mut guard = state.settings.lock().unwrap();
        guard.web_search = new_config;
        crate::config::loader::save_settings(&guard)
    };
    if let Err(e) = save_result {
        log::warn!("Failed to save web search settings: {}", e);
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }
    Json(serde_json::json!({ "status": "ok" }))
}

/// POST /web-search/proxy — proxy search request through backend to avoid CORS.
#[derive(serde::Deserialize)]
pub struct WebSearchProxyBody {
    pub query: String,
    pub provider_name: Option<String>,
}

pub async fn web_search_proxy(
    State(state): State<AppState>,
    Json(body): Json<WebSearchProxyBody>,
) -> Response {
    let provider_name = {
        let settings = state.settings.lock().unwrap();
        body.provider_name.clone().unwrap_or_else(|| settings.web_search.default_provider.clone())
    };

    let provider = {
        let settings = state.settings.lock().unwrap();
        settings.web_search.providers.iter().find(|p| p.name == provider_name).cloned()
    };

    let provider = match provider {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"success": false, "error": "provider not found"}))).into_response(),
    };

    let is_post = provider.method.to_uppercase() == "POST";

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success": false, "error": "http client error"}))).into_response(),
    };

    let mut req = if is_post {
        let body_str = provider.body_template
            .unwrap_or_default()
            .replace("{text}", &body.query);
        client.post(&provider.search_url)
            .header("Content-Type", "application/json")
            .body(body_str)
    } else {
        // Percent-encode query for GET requests
        let encoded: String = body.query.bytes().flat_map(|b| {
            let v: Vec<char> = match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => vec![b as char],
                b' ' => vec!['+'],
                _ => format!("%{:02X}", b).chars().collect(),
            };
            v
        }).collect();
        let url = provider.search_url.replace("{text}", &encoded);
        client.get(&url)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("DNT", "1")
            .header("Upgrade-Insecure-Requests", "1")
    };
    // Provider-specific headers override defaults
    for (k, v) in &provider.headers {
        req = req.header(k.as_str(), v.as_str());
    }

    match req.send().await {
        Ok(resp) => match resp.text().await {
            Ok(text) => Json(serde_json::json!({"success": true, "data": text})).into_response(),
            Err(_) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"success": false, "error": "read error"}))).into_response(),
        },
        Err(_) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"success": false, "error": "request failed"}))).into_response(),
    }
}

/// GET /web-search/fetch-page — proxy page content fetch to avoid CORS.
/// GET /settings/rank — return the current rank config.
pub async fn get_rank_settings(
    State(state): State<AppState>,
) -> Json<RankConfig> {
    let settings = state.settings.lock().unwrap();
    Json(settings.graph.rank.clone())
}

/// PUT /settings/rank — update rank config and persist to disk.
pub async fn update_rank_settings(
    State(state): State<AppState>,
    Json(new_config): Json<RankConfig>,
) -> Json<serde_json::Value> {
    let save_result = {
        let mut guard = state.settings.lock().unwrap();
        guard.graph.rank = new_config;
        crate::config::loader::save_settings(&guard)
    };
    if let Err(e) = save_result {
        log::warn!("Failed to save rank settings: {}", e);
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }
    Json(serde_json::json!({ "status": "ok" }))
}
