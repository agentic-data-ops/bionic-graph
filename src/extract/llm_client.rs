use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::config::ExtractionConfig;

// ─── OpenAI Chat Completion API types ────────────────────────────

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: usize,
    temperature: f32,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

/// Result of an LLM API call.
#[derive(Debug)]
pub struct LlmResult {
    pub content: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    /// Reason the generation finished: "stop", "length", etc.
    pub finish_reason: Option<String>,
}

/// Error from the LLM API.
#[derive(Debug)]
pub enum LlmError {
    Http(reqwest::Error),
    Api { status: u16, body: String },
    EmptyResponse,
    MaxRetriesExceeded(Vec<LlmError>),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {}", e),
            Self::Api { status, body } => {
                write!(f, "API error ({}): {}", status, body)
            }
            Self::EmptyResponse => write!(f, "LLM returned empty response"),
            Self::MaxRetriesExceeded(errors) => {
                write!(f, "Max retries exceeded. Errors: {:?}", errors)
            }
        }
    }
}

impl std::error::Error for LlmError {}

/// Call the OpenAI-compatible chat completion API.
///
/// Sends system + user messages, receives a JSON response, and returns
/// the content text along with token usage stats.
pub async fn chat_completion(
    config: &ExtractionConfig,
    system_prompt: &str,
    user_message: &str,
) -> Result<LlmResult, LlmError> {
    let url = format!("{}/chat/completions", config.api_base_url.trim_end_matches('/'));

    let request_body = ChatRequest {
        model: config.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_message.to_string(),
            },
        ],
        max_tokens: config.max_output_tokens,
        temperature: 0.1, // Low temperature for structured extraction
        stream: false,
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(LlmError::Http)?;

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(LlmError::Http)?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(LlmError::Api {
            status: status.as_u16(),
            body,
        });
    }

    let chat_response: ChatResponse = response
        .json()
        .await
        .map_err(LlmError::Http)?;

    let choice = chat_response
        .choices
        .into_iter()
        .next()
        .ok_or(LlmError::EmptyResponse)?;

    let content = choice.message.content.ok_or(LlmError::EmptyResponse)?;
    let prompt_tokens = chat_response
        .usage
        .as_ref()
        .and_then(|u| u.prompt_tokens)
        .unwrap_or(0);
    let completion_tokens = chat_response
        .usage
        .as_ref()
        .and_then(|u| u.completion_tokens)
        .unwrap_or(0);
    let finish_reason = choice.finish_reason;

    Ok(LlmResult {
        content,
        prompt_tokens,
        completion_tokens,
        finish_reason,
    })
}

/// Call with automatic retry on failure.
pub async fn chat_completion_with_retry(
    config: &ExtractionConfig,
    system_prompt: &str,
    user_message: &str,
) -> Result<LlmResult, LlmError> {
    let mut errors = Vec::new();

    for attempt in 0..=config.max_retries {
        if attempt > 0 {
            let delay = Duration::from_secs(2u64.pow(attempt)); // exponential backoff
            tokio::time::sleep(delay).await;
            log::info!("Retry attempt {}/{} for LLM call", attempt, config.max_retries);
        }

        match chat_completion(config, system_prompt, user_message).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                log::warn!("LLM call failed (attempt {}): {}", attempt, e);
                errors.push(e);
            }
        }
    }

    Err(LlmError::MaxRetriesExceeded(errors))
}
