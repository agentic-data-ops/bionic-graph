use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use http_body_util::{BodyExt, StreamBody};
use http_body::Frame;
use serde_json::{json, Value};

use crate::gremlin::AppState;

// ─── Response types ──────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct ModelListResponse {
    object: String,
    data: Vec<ModelEntry>,
}

#[derive(serde::Serialize)]
pub struct ModelEntry {
    id: String,
    object: String,
    created: u64,
    owned_by: String,
}

// ─── Handlers ────────────────────────────────────────────────────

/// `GET /proxy/openai/v1/models`
///
/// Returns all models from configured providers in OpenAI-compatible format.
/// Each model id is `"<provider>/<model>"`.
/// Response header `x-default-model` indicates the default model key.
pub async fn list_models_handler(
    State(state): State<AppState>,
) -> (HeaderMap, Json<ModelListResponse>) {
    let s = state.settings.lock().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut data = Vec::new();
    for provider in &s.llm.providers {
        for model in &provider.models {
            data.push(ModelEntry {
                id: format!("{}/{}", provider.name, model),
                object: "model".to_string(),
                created: now,
                owned_by: provider.name.clone(),
            });
        }
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        "x-default-model",
        HeaderValue::from_str(&s.llm.default_model).unwrap_or(HeaderValue::from_static("")),
    );

    (headers, Json(ModelListResponse {
        object: "list".to_string(),
        data,
    }))
}

/// `POST /proxy/openai/v1/chat/completions`
///
/// Forward an OpenAI-compatible chat completion request to the configured provider.
/// Accepts all standard OpenAI chat completion fields (via raw JSON passthrough).
/// Model format: `"<provider>/<model>"` — the provider is looked up from settings
/// and the request is proxied with the stored API key.
///
/// Streaming: forwards raw SSE bytes from the upstream — no re-encoding, so
/// the `data:` prefix and `[DONE]` signal arrive at the client unchanged.
pub async fn chat_completions_handler(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    // Extract model and stream flag
    let model = match body.get("model").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": "Missing required field: 'model'",
                        "type": "invalid_request_error",
                    }
                })),
            )
                .into_response();
        }
    };

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Parse model = "Provider/ModelName"
    let (provider_name, model_name) = match model.split_once('/') {
        Some((p, m)) => (p.to_string(), m.to_string()),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": "Invalid model format. Expected 'provider/model'",
                        "type": "invalid_request_error",
                    }
                })),
            )
                .into_response();
        }
    };

    // Look up provider settings
    let (api_key, api_base_url) = {
        let s = state.settings.lock().unwrap();
        match s.llm.providers.iter().find(|p| p.name == provider_name) {
            Some(provider) => (provider.api_key.clone(), provider.api_base_url.clone()),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": {
                            "message": format!("Provider '{}' not found", provider_name),
                            "type": "invalid_request_error",
                        }
                    })),
                )
                    .into_response();
            }
        }
    };

    // Clone the incoming body and replace model + stream so all other fields
    // (tools, response_format, temperature, etc.) pass through transparently.
    let mut forward_body = body;
    forward_body["model"] = json!(model_name);
    forward_body["stream"] = json!(is_stream);

    // Build reqwest client with proxy and SSL settings
    let mut client_builder = reqwest::Client::builder();

    {
        let s = state.settings.lock().unwrap();
        if let Some(proxy_url) = &s.internet.proxy {
            if !proxy_url.is_empty() {
                if let Ok(proxy) = reqwest::Proxy::all(proxy_url) {
                    client_builder = client_builder.proxy(proxy);
                }
            }
        }
        if !s.internet.ssl_verify {
            client_builder = client_builder.danger_accept_invalid_certs(true);
        }
    }

    let client = client_builder.build().unwrap_or_else(|_| reqwest::Client::new());
    let forward_url = format!("{}/chat/completions", api_base_url.trim_end_matches('/'));

    let mut req_builder = client
        .post(&forward_url)
        .header("Content-Type", "application/json")
        .json(&forward_body);

    if !api_key.is_empty() {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }

    match req_builder.send().await {
        Ok(resp) => {
            let status = resp.status();

            if is_stream && status.is_success() {
                // ── Streaming: forward raw SSE bytes as-is ──
                // No re-encoding through axum::Sse — the upstream already emits
                // correct "data: {...}\n\n" format including the final "data: [DONE]".
                let body_stream = resp.bytes_stream().map(|chunk| {
                    chunk
                        .map(Frame::data)
                        .map_err(|e| axum::Error::new(e))
                });
                let body = axum::body::Body::new(
                    StreamBody::new(body_stream).boxed_unsync(),
                );

                Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "text/event-stream")
                    .header("Cache-Control", "no-cache")
                    .header("Connection", "keep-alive")
                    .body(body)
                    .unwrap_or_else(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({"error": {"message": "Stream body error", "type": "server_error"}})),
                        )
                            .into_response()
                    })
            } else {
                // ── Non-streaming: return full JSON ──
                match resp.json::<Value>().await {
                    Ok(json_body) => (status, Json(json_body)).into_response(),
                    Err(e) => (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({
                            "error": {
                                "message": format!("Upstream error: {}", e),
                                "type": "upstream_error",
                            }
                        })),
                    )
                        .into_response(),
                }
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": {
                    "message": format!("Failed to connect to provider: {}", e),
                    "type": "upstream_error",
                }
            })),
        )
            .into_response(),
    }
}
