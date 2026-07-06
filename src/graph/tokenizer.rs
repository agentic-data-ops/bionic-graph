//! Token extraction and query tokenization for the search engine.
//!
//! Uses jieba-rs for ALL text (English, Chinese, mixed). jieba handles
//! English by treating continuous letter sequences as tokens, and
//! segments Chinese via its built-in dictionary.
//!
//! Tokens are extracted from vertex/edge attributes at create/update time,
//! and the same tokenization is applied to query keywords at search time.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::storage::types::Hit;

/// Global jieba instance, lazily initialized on first use.
fn jieba() -> &'static jieba_rs::Jieba {
    static JIEBA: OnceLock<jieba_rs::Jieba> = OnceLock::new();
    JIEBA.get_or_init(jieba_rs::Jieba::new)
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
        jieba()
            .cut(&lower, true) // HMM mode for unknown words
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_en_simple() {
        let tokens = Tokenizer::tokenize_query("Hello World");
        assert_eq!(tokens, vec!["hello", "world"]);
    }

    #[test]
    fn test_en_removes_stop_words() {
        let tokens = Tokenizer::tokenize_query("the cat and the dog");
        assert_eq!(tokens, vec!["cat", "dog"]);
    }

    #[test]
    fn test_en_case_insensitive() {
        let tokens = Tokenizer::tokenize_query("ALICE BOB");
        assert_eq!(tokens, vec!["alice", "bob"]);
    }

    #[test]
    fn test_extract_tokens() {
        let tokens = Tokenizer::extract_tokens(&[
            ("name", "Alice"),
            ("title", "Engineer"),
        ]);
        let map: HashMap<String, Vec<Hit>> = tokens.into_iter().collect();
        assert!(map.contains_key("alice"));
        assert!(map.contains_key("engineer"));
    }

    #[test]
    fn test_cjk_simple() {
        let tokens = Tokenizer::tokenize_query("我爱北京天安门");
        assert!(tokens.contains(&"北京".to_string()));
        assert!(tokens.contains(&"天安门".to_string()));
    }

    #[test]
    fn test_cjk_mixed_edge_cases() {
        let cases = vec![
            ("OpenAI发布GPT-4模型", vec!["openai", "发布", "gpt-4", "模型"]),
            ("Bionic-Graph是高性能图数据库", vec!["bionic", "graph", "高性能", "数据库"]),
            ("Hello世界", vec!["hello", "世界"]),
        ];
        for (input, expected) in &cases {
            let tokens = Tokenizer::tokenize_query(input);
            for word in expected {
                assert!(tokens.iter().any(|t| t == word), "Expected '{}' in tokens for '{}': got {:?}", word, input, tokens);
            }
        }
    }

    #[test]
    fn test_cjk_noise_filter() {
        // Single CJK characters should be filtered
        let tokens = Tokenizer::tokenize_query("的人");
        assert!(!tokens.contains(&"的".to_string()));
        assert!(!tokens.contains(&"人".to_string()));
    }

    #[test]
    fn test_mixed_all_languages() {
        // Pure English
        assert_eq!(Tokenizer::tokenize_query("hello world"), vec!["hello", "world"]);
        // Pure Chinese
        let cn = Tokenizer::tokenize_query("北京天安门");
        assert!(cn.contains(&"北京".to_string()));
        // Mixed
        let mx = Tokenizer::tokenize_query("hello世界");
        assert!(mx.contains(&"hello".to_string()));
        assert!(mx.contains(&"世界".to_string()));
    }
}
