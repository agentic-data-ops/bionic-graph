use std::time::Duration;

use futures_util::StreamExt;
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

/// One SSE chunk in a streaming chat completion response.
#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Option<Vec<StreamChoice>>,
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<StreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    /// Reasoning model thinking text (DeepSeek R1 / V4 etc.). Not part of
    /// the final answer, but consumes the `max_tokens` budget.
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    #[allow(dead_code)]
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
    /// Stream contained only `reasoning_content` (thinking) — the model burned
    /// its whole `max_tokens` budget on reasoning, so no final answer arrived.
    EmptyResponseWithReasoning,
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
            Self::EmptyResponseWithReasoning => write!(
                f,
                "LLM stream contained only reasoning_content — max_tokens budget likely exhausted by the model's thinking; increase max_output_tokens"
            ),
            Self::MaxRetriesExceeded(errors) => {
                write!(f, "Max retries exceeded. Errors: {:?}", errors)
            }
        }
    }
}

impl std::error::Error for LlmError {}

/// Call the OpenAI-compatible chat completion API in streaming mode.
///
/// Sends system + user messages with `stream: true`, consumes the SSE stream
/// chunk by chunk, and accumulates `delta.content` until the stream finishes
/// (`[DONE]` or EOF). The complete output is returned only after the stream
/// ends, so callers always receive the full text before parsing.
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
        stream: true,
    };

    let mut client_builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(300)); // longer budget for streaming

    // Apply proxy if configured
    if let Some(proxy_url) = &config.proxy {
        if !proxy_url.is_empty() {
            match reqwest::Proxy::all(proxy_url) {
                Ok(proxy) => {
                    client_builder = client_builder.proxy(proxy);
                }
                Err(e) => {
                    log::warn!("Invalid proxy URL '{}': {}", proxy_url, e);
                }
            }
        }
    }

    // Apply SSL verification
    if !config.ssl_verify {
        client_builder = client_builder.danger_accept_invalid_certs(true);
    }

    let client = client_builder
        .build()
        .map_err(LlmError::Http)?;

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
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

    // ── Consume the SSE stream and accumulate the full output ──
    let mut full_content = String::new();
    let mut prompt_tokens: u32 = 0;
    let mut completion_tokens: u32 = 0;
    let mut finish_reason: Option<String> = None;
    let mut reasoning_seen = false; // true if the stream carried reasoning_content chunks
    let mut buffer = String::new();

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(LlmError::Http)?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete SSE events (separated by blank lines).
        while let Some(sep) = buffer.find("\n\n") {
            let event = buffer[..sep].to_string();
            buffer.drain(..sep + 2);
            consume_sse_event(
                &event,
                &mut full_content,
                &mut prompt_tokens,
                &mut completion_tokens,
                &mut finish_reason,
                &mut reasoning_seen,
            );
        }
    }

    // Flush any trailing event without a trailing blank line.
    consume_sse_event(
        &buffer,
        &mut full_content,
        &mut prompt_tokens,
        &mut completion_tokens,
        &mut finish_reason,
        &mut reasoning_seen,
    );

    // Diagnostics: surface token-budget truncation so callers can react.
    if finish_reason.as_deref() == Some("length") {
        log::warn!(
            "LLM response truncated by max_tokens={} (finish_reason=length, completion_tokens={})",
            config.max_output_tokens,
            completion_tokens
        );
    }

    if full_content.is_empty() {
        if reasoning_seen {
            return Err(LlmError::EmptyResponseWithReasoning);
        }
        return Err(LlmError::EmptyResponse);
    }

    Ok(LlmResult {
        content: full_content,
        prompt_tokens,
        completion_tokens,
        finish_reason,
    })
}

/// Parse one SSE event block (all `data:` lines up to a blank line) and fold
/// its content deltas / finish reason / usage into the accumulators.
#[allow(clippy::too_many_arguments)]
fn consume_sse_event(
    event: &str,
    full_content: &mut String,
    prompt_tokens: &mut u32,
    completion_tokens: &mut u32,
    finish_reason: &mut Option<String>,
    reasoning_seen: &mut bool,
) {
    for line in event.lines() {
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }

        let parsed: StreamChunk = match serde_json::from_str(data) {
            Ok(p) => p,
            Err(_) => continue, // ignore malformed keep-alive chunks
        };

        if let Some(choices) = parsed.choices {
            for choice in choices {
                if let Some(delta) = choice.delta {
                    if delta.reasoning_content.is_some() {
                        *reasoning_seen = true;
                    }
                    if let Some(text) = delta.content {
                        full_content.push_str(&text);
                    }
                }
                if choice.finish_reason.is_some() {
                    *finish_reason = choice.finish_reason.clone();
                }
            }
        }

        if let Some(usage) = parsed.usage {
            *prompt_tokens = usage.prompt_tokens.unwrap_or(0);
            *completion_tokens = usage.completion_tokens.unwrap_or(0);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn consume(event: &str) -> (String, u32, u32, Option<String>) {
        let mut content = String::new();
        let mut pt = 0;
        let mut ct = 0;
        let mut fr = None;
        let mut reasoning = false;
        consume_sse_event(event, &mut content, &mut pt, &mut ct, &mut fr, &mut reasoning);
        (content, pt, ct, fr)
    }

    #[test]
    fn test_sse_accumulates_full_content() {
        let event = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"},\"finish_reason\":null}]}\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"world\"},\"finish_reason\":null}]}\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n",
        );
        let (content, _, _, fr) = consume(event);
        assert_eq!(content, "Hello world");
        assert_eq!(fr.as_deref(), Some("stop"));
    }

    #[test]
    fn test_sse_done_and_usage() {
        let event = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n",
            "data: [DONE]\n",
        );
        let (content, _, _, fr) = consume(event);
        assert_eq!(content, "partial");
        assert_eq!(fr, None);

        let usage_event = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n";
        let (content, pt, ct, _) = consume(usage_event);
        assert_eq!(content, "");
        assert_eq!(pt, 10);
        assert_eq!(ct, 5);
    }

    #[test]
    fn test_sse_ignores_non_data_and_malformed() {
        let event = concat!(
            ": keep-alive comment\n",
            "event: message\n",
            "data: not-json\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n",
        );
        let (content, _, _, _) = consume(event);
        assert_eq!(content, "ok");
    }

    #[test]
    fn test_sse_multi_line_event_block() {
        // SSE events are separated by blank lines; each block may hold
        // multiple `data:` lines (one per chunk in some servers).
        let event = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"b\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"c\"},\"finish_reason\":\"stop\"}]}\n\n",
        );
        let (content, _, _, fr) = consume(event);
        assert_eq!(content, "abc");
        assert_eq!(fr.as_deref(), Some("stop"));
    }
}
