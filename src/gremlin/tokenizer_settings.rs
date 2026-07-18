//! Tokenizer custom-word configuration endpoint.
//!
//! Allows users to add/remove custom dictionary words at runtime.
//! Persisted to the tokenizer config file (default ~/.config/bionic-graph/tokenizer.json).

use axum::{extract::State, Json};

use crate::gremlin::AppState;

/// GET /settings/tokenizer — list all custom words
pub async fn get_tokenizer_settings(
    State(_state): State<AppState>,
) -> Json<serde_json::Value> {
    let words = crate::graph::tokenizer::list_custom_words();
    Json(serde_json::json!({ "custom_words": words }))
}

/// POST /settings/tokenizer/words — add custom words
/// On master: broadcasts to workers. On worker: forwards to master.
pub async fn add_tokenizer_words(
    State(state): State<AppState>,
    Json(body): Json<TokenizerWordsBody>,
) -> Json<serde_json::Value> {
    if body.words.is_empty() {
        return Json(serde_json::json!({ "status": "ok", "message": "no words provided" }));
    }

    // If this is a worker node, forward to master.
    if let Some(ref master_api) = state.master_api_addr {
        let url = format!("http://{}/settings/tokenizer/words", master_api);
        match reqwest::Client::new()
            .post(&url)
            .json(&serde_json::json!({"words": body.words}))
            .send().await
        {
            Ok(resp) => {
                if let Ok(body) = resp.text().await {
                    return Json(serde_json::from_str(&body).unwrap_or(serde_json::json!({"status": "forwarded"})));
                }
            }
            Err(e) => {
                return Json(serde_json::json!({"status": "error", "message": format!("forward failed: {}", e)}));
            }
        }
    }

    crate::graph::tokenizer::add_custom_words(&body.words);

    // Broadcast to workers in cluster mode (master only).
    if let Some(ref registry) = state.cluster_registry {
        let workers = registry.alive_workers();
        if !workers.is_empty() {
            let words = body.words.clone();
            tokio::spawn(async move {
                for worker in &workers {
                    let url = format!("http://{}/cluster/tokenizer-sync", worker.cluster_addr);
                    let sync_body = serde_json::json!({
                        "operation": "add",
                        "words": words,
                    });
                    if let Err(e) = reqwest::Client::new()
                        .post(&url).json(&sync_body).send().await
                    {
                        log::warn!("Tokenizer sync to worker {} failed: {}", worker.node_id, e);
                    }
                }
            });
        }
    }

    Json(serde_json::json!({ "status": "ok", "added": body.words.len() }))
}

/// DELETE /settings/tokenizer/words — remove custom words
/// On master: broadcasts to workers. On worker: forwards to master.
pub async fn remove_tokenizer_words(
    State(state): State<AppState>,
    Json(body): Json<TokenizerWordsBody>,
) -> Json<serde_json::Value> {
    if body.words.is_empty() {
        return Json(serde_json::json!({ "status": "ok", "message": "no words provided" }));
    }

    // If this is a worker node, forward to master.
    if let Some(ref master_api) = state.master_api_addr {
        let url = format!("http://{}/settings/tokenizer/words", master_api);
        match reqwest::Client::new()
            .delete(&url)
            .json(&serde_json::json!({"words": body.words}))
            .send().await
        {
            Ok(resp) => {
                if let Ok(body) = resp.text().await {
                    return Json(serde_json::from_str(&body).unwrap_or(serde_json::json!({"status": "forwarded"})));
                }
            }
            Err(e) => {
                return Json(serde_json::json!({"status": "error", "message": format!("forward failed: {}", e)}));
            }
        }
    }

    crate::graph::tokenizer::remove_custom_words(&body.words);

    // Broadcast to workers in cluster mode (master only).
    if let Some(ref registry) = state.cluster_registry {
        let workers = registry.alive_workers();
        if !workers.is_empty() {
            let words = body.words.clone();
            tokio::spawn(async move {
                for worker in &workers {
                    let url = format!("http://{}/cluster/tokenizer-sync", worker.cluster_addr);
                    let sync_body = serde_json::json!({
                        "operation": "remove",
                        "words": words,
                    });
                    if let Err(e) = reqwest::Client::new()
                        .post(&url).json(&sync_body).send().await
                    {
                        log::warn!("Tokenizer sync to worker {} failed: {}", worker.node_id, e);
                    }
                }
            });
        }
    }

    Json(serde_json::json!({ "status": "ok", "removed": body.words.len() }))
}

#[derive(serde::Deserialize)]
pub struct TokenizerWordsBody {
    pub words: Vec<String>,
}
