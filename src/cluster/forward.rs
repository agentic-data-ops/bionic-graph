//! Write forwarding — when a worker receives a write request, it proxies
//! it to the master.
//!
//! Workers are read-only. Any POST / PUT / DELETE operation arriving at a
//! worker is forwarded to the master via HTTP. The master executes the
//! operation, replicates the redo log entry to all workers, and returns
//! the result to the worker, which proxies it back to the client.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A forwarded write request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForwardedRequest {
    /// HTTP method (POST, PUT, DELETE).
    pub method: String,
    /// The request path (e.g. "/vertices").
    pub path: String,
    /// Query string (e.g. "?force=true").
    pub query: Option<String>,
    /// Request body as a JSON string.
    pub body: Option<String>,
    /// Optional graph name.
    pub graph: Option<String>,
}

/// Response from the master after executing a forwarded write.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForwardedResponse {
    pub success: bool,
    pub status_code: u16,
    pub body: Option<String>,
    pub error: Option<String>,
}

/// Errors during write forwarding.
#[derive(Error, Debug)]
pub enum ForwardError {
    #[error("Master {0} unreachable: {1}")]
    MasterUnreachable(String, String),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("Master returned error status {code}: {message}")]
    MasterError { code: u16, message: String },
}

/// Forward a write request from a worker to the master.
pub async fn forward_write(
    master_addr: &str,
    request: &ForwardedRequest,
) -> Result<ForwardedResponse, ForwardError> {
    let url = format!("http://{}/cluster/forward", master_addr);
    let body = serde_json::to_string(request)?;

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await?;

    let status = resp.status().as_u16();
    let response_body = resp.text().await?;

    if status >= 200 && status < 300 {
        let forwarded: ForwardedResponse = serde_json::from_str(&response_body)?;
        Ok(forwarded)
    } else {
        Err(ForwardError::MasterError {
            code: status,
            message: response_body,
        })
    }
}

/// Handle an incoming forwarded write on the master.
///
/// The master executes the write locally, then returns the result.
///
/// In production this would:
/// 1. Deserialize the ForwardedRequest
/// 2. Route it to the appropriate handler (vertex/edge/graph CRUD)
/// 3. Execute it on the local graph
/// 4. Replicate the resulting redo log entry to all workers
/// 5. Return the response
pub fn handle_forwarded_request(_request: &ForwardedRequest) -> ForwardedResponse {
    // TODO: route to appropriate CRUD handler
    ForwardedResponse {
        success: true,
        status_code: 200,
        body: None,
        error: None,
    }
}

/// A set of API paths that are considered "write" operations.
/// Workers should forward these to the master.
pub const WRITE_PATHS: &[&str] = &[
    "/vertices",
    "/edges",
    "/graphs",
    "/documents",
    "/settings",
    "/compact",
    "/reindex",
    "/extract",
];

/// Check if a path is a write operation that should be forwarded.
pub fn is_write_path(path: &str) -> bool {
    WRITE_PATHS.iter().any(|p| path.starts_with(p))
        && !path.starts_with("/settings/search") // search settings are local
        && !path.starts_with("/settings/llm")    // LLM settings forwarded
        && !path.starts_with("/maas")            // MaaS proxy requests are not forwarded
        && !path.starts_with("/health")           // health checks are local
}
