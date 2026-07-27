//! Token batch buffer — defers ref appends for existing token so that
//! multiple refs to the same token are written in a single operation,
//! dramatically reducing `allocate_chunks` / `write_data_chunks` calls.
//!
//! Crash-safe: token index is rebuilt from vertex/edge WAL replay.
//! See REASONIX.md "P1 safety" for detailed analysis.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::graph::crud;
use crate::graph::graph::Graph;
use crate::storage::types::{Hit, StorageResult};

thread_local! {
    /// Per-thread buffer of pending token refs. `None` = not in batch mode.
    static TOKEN_BUF: RefCell<Option<Vec<PendingRef>>> = const { RefCell::new(None) };
}

/// A single pending token ref awaiting batch flush.
#[derive(Clone, Debug)]
pub struct PendingRef {
    pub token: String,
    pub ref_type: u8,
    pub ref_id: u32,
    pub hits: Vec<Hit>,
}

/// Enable token batch mode on the current thread.
pub fn start_batch() {
    TOKEN_BUF.with(|b| {
        if b.borrow().is_none() {
            *b.borrow_mut() = Some(Vec::with_capacity(256));
        }
    });
}

/// Buffer a ref for batch flush (existing token case).
/// New token creations still happen immediately.
pub fn buffer_add(graph: &Graph, token: &str, ref_type: u8, ref_id: u32, hits: &[Hit]) -> StorageResult<()> {
    // Check if token exists in memory index (fast, no IO).
    let exists = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.token.contains(token)
    };

    if !exists {
        // New token: write immediately (no existing record to update).
        crud::add_token_immediate(graph, token, ref_type, ref_id, hits)
    } else {
        // Existing token: buffer for batch flush.
        TOKEN_BUF.with(|b| {
            if let Some(ref mut buf) = *b.borrow_mut() {
                buf.push(PendingRef {
                    token: token.to_string(),
                    ref_type,
                    ref_id,
                    hits: hits.to_vec(),
                });
            }
            // If not in batch mode (e.g. single API call), caller should
            // still work — fall through to immediate write.
        });

        // If batch buffer is active, we're done for now.
        let in_batch = TOKEN_BUF.with(|b| b.borrow().is_some());
        if in_batch {
            Ok(())
        } else {
            // Not in batch mode — write immediately.
            crud::add_token_immediate(graph, token, ref_type, ref_id, hits)
        }
    }
}

/// Flush all buffered refs — grouped by token, written in one shot per token.
/// Call at the end of `batch_import` (and periodically for large batches).
pub fn flush_batch(graph: &Graph) -> StorageResult<()> {
    let pending: Vec<PendingRef> = TOKEN_BUF.with(|b| {
        b.borrow_mut().as_mut().map(|buf| std::mem::take(buf)).unwrap_or_default()
    });

    if pending.is_empty() {
        return Ok(());
    }

    // Group by token string.
    let mut grouped: HashMap<&str, Vec<PendingRef>> = HashMap::new();
    for p in &pending {
        grouped.entry(p.token.as_str()).or_default().push(p.clone());
    }

    let mut total = 0usize;
    for (token, refs) in &grouped {
        crud::add_token_batch(graph, token, refs)?;
        total += refs.len();
    }

    log::debug!("token_batch: flushed {} refs across {} token ({} avg)",
        total, grouped.len(), if grouped.len() > 0 { total / grouped.len() } else { 0 });

    Ok(())
}

/// End batch mode (flush + disable buffering).
pub fn end_batch(graph: &Graph) -> StorageResult<()> {
    let result = flush_batch(graph);
    TOKEN_BUF.with(|b| *b.borrow_mut() = None);
    result
}

/// Number of pending refs in the buffer.
pub fn pending_count() -> usize {
    TOKEN_BUF.with(|b| {
        b.borrow().as_ref().map(|buf| buf.len()).unwrap_or(0)
    })
}

/// Whether token batch mode is active on the current thread.
pub fn is_active() -> bool {
    TOKEN_BUF.with(|b| b.borrow().is_some())
}
