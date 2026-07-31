//! Unified cluster request model — a single request object used for both
//! forwarding (worker → master) and broadcasting (master → workers).
//!
//! `ClusterRequest` captures the complete HTTP request information (method,
//! path, headers, body) at the handler level, so forwarding and broadcasting
//! always use the identical payload — eliminating the drift that existed when
//! handlers manually constructed separate forwarding and broadcast requests.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cluster::forward::ForwardedRequest;

/// A unified cluster request capturing the full HTTP request context.
///
/// Constructed once in each handler, then used for both forwarding
/// (worker → master) and broadcasting (master → workers).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClusterRequest {
    /// HTTP method (GET, POST, PUT, DELETE).
    pub method: String,
    /// Request path including any query string (e.g. "/vertices/1?force=true").
    pub path: String,
    /// Original request headers (X-Graph-Name, X-Time-Travel, etc.).
    pub headers: HashMap<String, String>,
    /// Request body as a JSON string.
    pub body: Option<String>,
}

impl ClusterRequest {
    /// Create a new cluster request with the given HTTP method and path.
    pub fn new(method: &str, path: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            headers: HashMap::new(),
            body: None,
        }
    }

    /// Set the request body.
    pub fn with_body(mut self, body: &str) -> Self {
        self.body = Some(body.to_string());
        self
    }

    /// Set the X-Graph-Name header.
    pub fn with_graph(mut self, graph: &str) -> Self {
        self.headers
            .insert("X-Graph-Name".to_string(), graph.to_string());
        self
    }

    /// Set the X-Graph-Name header. No-op when `graph` is None.
    pub fn with_opt_graph(mut self, graph: Option<&str>) -> Self {
        if let Some(g) = graph {
            self.headers.insert("X-Graph-Name".to_string(), g.to_string());
        }
        self
    }

    /// Set the X-Time-Travel header.
    pub fn with_time_travel(mut self, tt: u64) -> Self {
        self.headers
            .insert("X-Time-Travel".to_string(), tt.to_string());
        self
    }

    /// Add an arbitrary header.
    pub fn with_header(mut self, key: &str, val: &str) -> Self {
        self.headers.insert(key.to_string(), val.to_string());
        self
    }

    /// Append a query string to the path (e.g. "force=true" → "?force=true").
    /// Pass `None` to skip (no-op).
    pub fn with_query_str(mut self, qs: Option<&str>) -> Self {
        if let Some(q) = qs {
            if !q.is_empty() {
                self.path.push('?');
                self.path.push_str(q);
            }
        }
        self
    }

    /// Convert to a `ForwardedRequest` for the cluster forwarding protocol.
    /// The `graph` field is extracted from `X-Graph-Name` header.
    pub fn to_forwarded(&self) -> ForwardedRequest {
        let (path_only, query) = split_path_query(&self.path);
        ForwardedRequest {
            method: self.method.clone(),
            path: path_only,
            query: query.map(|s| s.to_string()),
            body: self.body.clone(),
            graph: self
                .headers
                .get("X-Graph-Name")
                .cloned(),
        }
    }

    /// Shortcut: true if this is a tokenizer-sync operation.
    pub fn is_tokenizer_op(&self) -> bool {
        self.path == "/settings/tokenizer/words"
    }
}

/// Split a path+query string into (path, optional query).
/// e.g. "/vertices/1?force=true" → ("/vertices/1", Some("force=true"))
fn split_path_query(path: &str) -> (String, Option<&str>) {
    if let Some(pos) = path.find('?') {
        (path[..pos].to_string(), Some(&path[pos + 1..]))
    } else {
        (path.to_string(), None)
    }
}
