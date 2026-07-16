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
pub async fn add_tokenizer_words(
    State(_state): State<AppState>,
    Json(body): Json<TokenizerWordsBody>,
) -> Json<serde_json::Value> {
    if body.words.is_empty() {
        return Json(serde_json::json!({ "status": "ok", "message": "no words provided" }));
    }
    crate::graph::tokenizer::add_custom_words(&body.words);
    Json(serde_json::json!({ "status": "ok", "added": body.words.len() }))
}

/// DELETE /settings/tokenizer/words — remove custom words
pub async fn remove_tokenizer_words(
    State(_state): State<AppState>,
    Json(body): Json<TokenizerWordsBody>,
) -> Json<serde_json::Value> {
    if body.words.is_empty() {
        return Json(serde_json::json!({ "status": "ok", "message": "no words provided" }));
    }
    crate::graph::tokenizer::remove_custom_words(&body.words);
    Json(serde_json::json!({ "status": "ok", "removed": body.words.len() }))
}

#[derive(serde::Deserialize)]
pub struct TokenizerWordsBody {
    pub words: Vec<String>,
}
