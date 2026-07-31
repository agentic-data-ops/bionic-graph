//! ClusterGateway — unified entry point for cluster forwarding and broadcasting.
//!
//! Replaces the scattered `try_forward_json`, `try_forward_status`,
//! `try_forward_read_json`, and `broadcast_request_to_workers` functions
//! that existed in each handler file.
//!
//! # Usage
//!
//! ```rust,ignore
//! let gateway = ClusterGateway::from_state(&state);
//!
//! // Forward a write request (worker → master). Checks REPLAYING flag.
//! if let Some(resp) = gateway.forward::<CreateVertexResponse>(&req).await? {
//!     return Ok(Json(resp));
//! }
//!
//! // Forward a read request (skips REPLAYING check).
//! if let Some(resp) = gateway.forward_read::<TaskResponse>(&req).await? {
//!     return Ok(Json(resp));
//! }
//!
//! // Broadcast a write request (master → all workers).
//! gateway.broadcast(&req);
//! ```

use std::sync::Arc;

use axum::http::StatusCode;
use serde::de::DeserializeOwned;

use crate::cluster::forward::{forward_write, ForwardedResponse};
use crate::cluster::node::NodeRegistry;
use crate::cluster::request::ClusterRequest;
use crate::graph::graph::is_broadcast_replay;

/// Unified cluster gateway for forwarding and broadcasting.
#[derive(Clone)]
pub struct ClusterGateway {
    /// Whether this node is a worker (writes are forwarded to master).
    is_worker: bool,
    /// Master's cluster address for forwarding (None on master/standalone).
    master_addr: Option<String>,
    /// Node registry for broadcasting to workers (None on worker/standalone).
    registry: Option<Arc<NodeRegistry>>,
}

/// Internal result of a forward attempt.
enum ForwardResult {
    /// The request was forwarded and the master returned a successful response.
    Forwarded(ForwardedResponse),
    /// Not forwarded (not a worker, or replaying, or no master addr).
    NotForwarded,
}

impl ClusterGateway {
    /// Build a gateway from the handler's AppState (fields from gremlin/mod.rs).
    pub fn new(
        is_worker: bool,
        master_addr: Option<String>,
        registry: Option<Arc<NodeRegistry>>,
    ) -> Self {
        Self {
            is_worker,
            master_addr,
            registry,
        }
    }

    // ── Forwarding ─────────────────────────────────────────────────────────

    /// Forward a write request to the master. Returns `Ok(Some(T))` with the
    /// deserialized response on success, `Ok(None)` if the request should be
    /// handled locally (master node, or replaying), or `Err(StatusCode)` on
    /// forward failure.
    ///
    /// **Checks the REPLAYING flag** — during cluster broadcast replay,
    /// forwarding is skipped to prevent recursion.
    pub async fn forward<T: DeserializeOwned>(
        &self,
        req: &ClusterRequest,
    ) -> Result<Option<T>, StatusCode> {
        // During cluster broadcast replay, skip forwarding to prevent recursion.
        if is_broadcast_replay() {
            return Ok(None);
        }
        match self.try_forward_inner(req).await {
            ForwardResult::Forwarded(resp) => {
                Self::parse_forwarded_response::<T>(&resp)
            }
            ForwardResult::NotForwarded => Ok(None),
        }
    }

    /// Forward a **read** request to the master. Same as `forward()` but
    /// **skips the REPLAYING check** — used for task polling and other
    /// read-only operations that must not be blocked by concurrent replays.
    pub async fn forward_read<T: DeserializeOwned>(
        &self,
        req: &ClusterRequest,
    ) -> Result<Option<T>, StatusCode> {
        match self.try_forward_inner(req).await {
            ForwardResult::Forwarded(resp) => {
                Self::parse_forwarded_response::<T>(&resp)
            }
            ForwardResult::NotForwarded => Ok(None),
        }
    }

    /// Low-level: attempt to forward the request to master, returning the
    /// raw ForwardedResponse. Returns NotForwarded if this is not a worker
    /// or no master address is configured.
    async fn try_forward_inner(&self, req: &ClusterRequest) -> ForwardResult {
        if !self.is_worker {
            return ForwardResult::NotForwarded;
        }
        let Some(ref master_addr) = self.master_addr else {
            return ForwardResult::NotForwarded;
        };
        let forwarded = req.to_forwarded();
        match forward_write(master_addr, &forwarded).await {
            Ok(resp) => ForwardResult::Forwarded(resp),
            Err(e) => {
                log::warn!("Forward to master failed: {}", e);
                ForwardResult::Forwarded(ForwardedResponse {
                    success: false,
                    status_code: 502,
                    body: None,
                    error: Some(format!("Proxy error: {}", e)),
                })
            }
        }
    }

    /// Parse a ForwardedResponse into the desired type or return an error StatusCode.
    fn parse_forwarded_response<T: DeserializeOwned>(
        resp: &ForwardedResponse,
    ) -> Result<Option<T>, StatusCode> {
        if resp.success && resp.status_code < 300 {
            // Success — parse the body
            if let Some(ref body_str) = resp.body {
                if body_str.is_empty() {
                    // Empty body: return Ok(None) so the handler
                    // knows there's nothing to return but no error.
                    // The caller should still treat this as "forwarded".
                    return Ok(None);
                }
                match serde_json::from_str::<T>(body_str) {
                    Ok(val) => Ok(Some(val)),
                    Err(_) => {
                        log::warn!(
                            "forward parse: body='{}' could not be deserialized as T",
                            body_str
                        );
                        Err(StatusCode::INTERNAL_SERVER_ERROR)
                    }
                }
            } else {
                // Success with no body — return Ok(None) but the caller
                // should consider it forwarded.
                Ok(None)
            }
        } else if resp.success {
            // Master returned non-2xx status (e.g. 404)
            Err(StatusCode::from_u16(resp.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
        } else {
            // Master returned an error
            Err(StatusCode::from_u16(resp.status_code)
                .unwrap_or(StatusCode::BAD_GATEWAY))
        }
    }

    // ── Broadcasting ───────────────────────────────────────────────────────

    /// Broadcast a request to all alive workers (fire-and-forget).
    ///
    /// Automatically routes tokenizer operations to `/cluster/tokenizer-sync`
    /// instead of `/cluster/execute`.
    ///
    /// **Skips** broadcasting during replay (prevent recursion).
    pub fn broadcast(&self, req: &ClusterRequest) {
        // During cluster broadcast replay, skip broadcast to prevent recursion.
        if is_broadcast_replay() {
            return;
        }
        let Some(ref registry) = self.registry else {
            log::warn!("broadcast: no registry available");
            return;
        };
        let workers = registry.alive_workers();
        if workers.is_empty() {
            return;
        }

        let forwarded = req.to_forwarded();
        let workers_for_broadcast: Vec<_> = workers
            .into_iter()
            .map(|w| (w.node_id.clone(), w.cluster_addr.clone()))
            .collect();

        // Tokenizer operations go to /cluster/tokenizer-sync (different body format).
        if req.is_tokenizer_op() {
            let op = match req.method.to_uppercase().as_str() {
                "POST" => "add",
                "DELETE" => "remove",
                _ => return,
            };
            if let Some(ref req_body) = req.body {
                let words: serde_json::Value = serde_json::from_str(req_body)
                    .ok()
                    .and_then(|v: serde_json::Value| v.get("words").cloned())
                    .unwrap_or(serde_json::Value::Null);
                tokio::spawn(async move {
                    for (node_id, cluster_addr) in &workers_for_broadcast {
                        let url = format!("http://{}/cluster/tokenizer-sync", cluster_addr);
                        let client = reqwest::Client::new();
                        let sync_body = serde_json::json!({
                            "operation": op,
                            "words": words,
                        });
                        if let Err(e) = client
                            .post(&url)
                            .json(&sync_body)
                            .send()
                            .await
                        {
                            log::warn!(
                                "Tokenizer sync to worker {} failed: {}",
                                node_id,
                                e
                            );
                        }
                    }
                });
            }
            return;
        }

        // Standard broadcast: POST to /cluster/execute
        let payload_json = serde_json::to_string(&forwarded).unwrap_or_default();
        tokio::spawn(async move {
            for (node_id, cluster_addr) in &workers_for_broadcast {
                let url = format!("http://{}/cluster/execute", cluster_addr);
                let client = reqwest::Client::new();
                if let Err(e) = client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .body(payload_json.clone())
                    .send()
                    .await
                {
                    log::warn!(
                        "Broadcast to worker {} failed: {}",
                        node_id,
                        e
                    );
                }
            }
        });
    }
}
