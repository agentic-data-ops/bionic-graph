//! Token extraction and query tokenization for the search engine.
//!
//! Uses jieba-rs for ALL text (English, Chinese, mixed). jieba handles
//! English by treating continuous letter sequences as tokens, and
//! segments Chinese via its built-in dictionary.
//!
//! Supports custom user dictionary words loaded from a JSON config file
//! (default `~/.config/bionic-graph/tokenizer.json`). Words can be added
//! and removed at runtime via API endpoints.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use crate::storage::types::Hit;

/// Path to the tokenizer config file (set via CLI before any API calls).
static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Set the tokenizer config file path (called once at startup).
pub fn set_config_path(path: PathBuf) {
    let _ = CONFIG_PATH.set(path.clone());
    // Load custom words from the config file into jieba.
    let words = load_words_from_config(CONFIG_PATH.get());
    if !words.is_empty() {
        let jieba = jieba();
        let mut j = jieba.write().unwrap();
        for word in &words {
            j.add_word(word, None, None);
        }
    }
}

/// Load custom words list from the JSON config file.
fn load_words_from_config(path: Option<&PathBuf>) -> Vec<String> {
    if let Some(p) = path {
        if p.exists() {
            if let Ok(content) = std::fs::read_to_string(p) {
                if let Ok(cfg) = serde_json::from_str::<HashMap<String, Vec<String>>>(&content) {
                    return cfg.get("custom_words").cloned().unwrap_or_default();
                }
            }
        }
    }
    Vec::new()
}

/// Persist custom words list to the JSON config file.
fn save_words_to_config(path: Option<&PathBuf>, words: &[String]) {
    if let Some(p) = path {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut map = HashMap::new();
        map.insert("custom_words".to_string(), words.to_vec());
        if let Ok(json) = serde_json::to_string_pretty(&map) {
            let _ = std::fs::write(p, &json);
        }
    }
}

/// Global jieba instance, lazily initialized on first use.
/// Uses RwLock to allow runtime dictionary modifications.
fn jieba() -> &'static RwLock<jieba_rs::Jieba> {
    static JIEBA: OnceLock<RwLock<jieba_rs::Jieba>> = OnceLock::new();
    JIEBA.get_or_init(|| RwLock::new(jieba_rs::Jieba::new()))
}

/// Add custom words to the jieba dictionary and save to config file.
pub fn add_custom_words(words: &[String]) {
    if words.is_empty() {
        return;
    }
    let jieba = jieba();
    let mut j = jieba.write().unwrap();
    for word in words {
        j.add_word(word, None, None);
    }
    drop(j);

    // Persist updated word list.
    let mut all = load_words_from_config(CONFIG_PATH.get());
    for word in words {
        if !all.contains(word) {
            all.push(word.clone());
        }
    }
    save_words_to_config(CONFIG_PATH.get(), &all);
}

/// Remove custom words from the jieba dictionary.
/// jieba-rs has no remove_word(), so we reload the default dict and re-add all
/// remaining custom words.
pub fn remove_custom_words(words: &[String]) {
    if words.is_empty() {
        return;
    }
    let jieba = jieba();
    let mut j = jieba.write().unwrap();

    // Get current custom words from config.
    let mut all = load_words_from_config(CONFIG_PATH.get());
    all.retain(|w| !words.contains(w));

    // Reload: clear + re-add default dict + all remaining custom words.
    j.clear();
    j.load_default_dict();

    // Re-add all custom words.
    for word in &all {
        j.add_word(word, None, None);
    }
    drop(j);

    save_words_to_config(CONFIG_PATH.get(), &all);
}

/// List all current custom words.
pub fn list_custom_words() -> Vec<String> {
    load_words_from_config(CONFIG_PATH.get())
}

/// English stop words to filter out.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for",
    "of", "with", "by", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "shall", "can", "it", "its", "this", "that",
    "these", "those", "i", "me", "my", "we", "our", "you", "your", "he",
    "she", "they", "them", "their", "not", "no", "nor",
];

/// Minimum token length in Unicode characters — single-char tokens are discarded.
const MIN_TOKEN_LEN: usize = 2;

/// Tokenizer for splitting text into search tokens.
pub struct Tokenizer;

impl Tokenizer {
    /// Extract tokens from a set of string attribute values.
    ///
    /// Each attribute is a `(key, value)` pair. The `key` is recorded in
    /// each hit's `hit_key`. Returns a list of `(token, hits)` pairs.
    pub fn extract_tokens(attrs: &[(&str, &str)]) -> Vec<(String, Vec<Hit>)> {
        let mut token_map: HashMap<String, Vec<Hit>> = HashMap::new();

        for &(key, value) in attrs {
            let tokens = Self::tokenize(value);
            for (offset, token) in tokens.iter().enumerate() {
                token_map
                    .entry(token.clone())
                    .or_default()
                    .push(Hit {
                        hit_key: key.to_string(),
                        hit_offset: offset as u16,
                    });
            }
        }

        token_map.into_iter().collect()
    }

    /// Tokenize a search query into individual keywords.
    /// Calls jieba for all text (English, Chinese, mixed).
    pub fn tokenize_query(query: &str) -> Vec<String> {
        Self::tokenize(query)
    }

    // ── Internal ─────────────────────────────────────────────────────────────

    fn tokenize(text: &str) -> Vec<String> {
        let lower = text.to_lowercase();
        let jieba = jieba();
        let j = jieba.read().unwrap();
        j.cut(&lower, true) // HMM mode for unknown words
            .into_iter()
            .filter(|t| {
                let word = t.word;
                // Filter single-character tokens (both English and CJK)
                word.chars().count() >= MIN_TOKEN_LEN
                    // Filter English stop words
                    && !STOP_WORDS.contains(&word)
            })
            .map(|t| t.word.to_string())
            .collect()
    }
}
