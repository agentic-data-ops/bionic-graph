//! Tokenizer custom-word configuration endpoint.
//!
//! Allows users to add/remove custom dictionary words at runtime.
//! Persisted to `<data_dir>/tokenizer/words.json`.

use axum::{extract::State, Json};

use crate::gremlin::AppState;

/// GET /settings/tokenizer/words — list all custom words
pub async fn get_tokenizer_words(
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

    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let gateway = state.cluster_gateway();

    // Worker → Master forwarding via ClusterGateway (tokenizer ops handled by gateway)
    let req = crate::cluster::request::ClusterRequest::new("POST", "/settings/tokenizer/words")
        .with_body(&body_str);
    match gateway.forward::<serde_json::Value>(&req).await {
        Ok(Some(val)) => return Json(val),
        Ok(None) => {}
        Err(_) => return Json(serde_json::json!({"status": "error"})),
    }

    crate::graph::tokenizer::add_custom_words(&body.words);

    // Broadcast to workers in cluster mode (master only). Gateway auto-routes to /cluster/tokenizer-sync.
    gateway.broadcast(&req);

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

    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let gateway = state.cluster_gateway();

    // Worker → Master forwarding via ClusterGateway (tokenizer ops handled by gateway)
    let req = crate::cluster::request::ClusterRequest::new("DELETE", "/settings/tokenizer/words")
        .with_body(&body_str);
    match gateway.forward::<serde_json::Value>(&req).await {
        Ok(Some(val)) => return Json(val),
        Ok(None) => {}
        Err(_) => return Json(serde_json::json!({"status": "error"})),
    }

    crate::graph::tokenizer::remove_custom_words(&body.words);

    // Broadcast to workers in cluster mode (master only). Gateway auto-routes to /cluster/tokenizer-sync.
    gateway.broadcast(&req);

    Json(serde_json::json!({ "status": "ok", "removed": body.words.len() }))
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct TokenizerWordsBody {
    pub words: Vec<String>,
}
