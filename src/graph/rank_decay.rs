//! Periodic rank decay — scans for inactive vertices/edges and decrements
//! their rank so that popularity naturally fades over time.
//!
//! Uses the `atime_index` in `MemoryIndex` for efficient range scans.

use std::sync::Arc;
use std::time::Duration;

use crate::graph::graph::Graph;
use crate::storage::types::OpType;

/// Spawn a background task that periodically decays rank for inactive entities.
///
/// Runs only when `config.auto_dec_rank_when_inactive` is `true`.
/// Checks every `config.inactive_rank_update_period` seconds.
/// Entities whose `atime` is older than `inactive_after_accessed_secs` seconds
/// get their rank decremented by 1 (minimum 0).
pub fn spawn_rank_decay(
    graph: Arc<Graph>,
    enabled: bool,
    inactive_after_accessed_secs: u64,
    period_secs: u64,
) {
    if !enabled || period_secs == 0 {
        return;
    }

    let period = Duration::from_secs(period_secs);
    let inactive_threshold_us = inactive_after_accessed_secs * 1_000_000;

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(period).await;
            let now = crate::graph::crud::timestamp_us();
            let threshold = now.saturating_sub(inactive_threshold_us);

            // Collect inactive pointers under a read lock.
            let inactive: Vec<(u64, crate::storage::memory_index::IndexPointer)> = {
                let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
                mi.atime_index.range_up_to(threshold)
            };

            if inactive.is_empty() {
                continue;
            }

            log::debug!("Rank decay: scanning {} inactive entities", inactive.len());

            for (old_atime, ptr) in &inactive {
                // Read current record and decrement rank.
                let result = try_decay(&graph, *old_atime, ptr);
                if let Err(e) = result {
                    log::debug!("Rank decay failed for {:?}: {}", ptr, e);
                }
            }
        }
    });
}

/// Attempt to decrement rank for a single entity by its index pointer.
fn try_decay(
    graph: &Arc<Graph>,
    _old_atime: u64,
    ptr: &crate::storage::memory_index::IndexPointer,
) -> Result<(), String> {
    // Try vertex first.
    if let Ok(mut rec) = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset) {
        if rec.rank == 0 {
            return Ok(()); // already at minimum
        }
        let old_rank = rec.rank;
        rec.rank = rec.rank.saturating_sub(1);

        graph.index_file
            .update_vertex_record(ptr.block_idx, ptr.chunk_offset, &rec)
            .map_err(|e| format!("index write: {}", e))?;

        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.ranks.remove(old_rank, ptr);
        mi.ranks.insert(rec.rank, *ptr);
        // atime_index unchanged — we only decayed rank, not atime.

        // Append decay entry to redo log.
        let mut data = Vec::with_capacity(12);
        data.extend_from_slice(&rec.rank.to_le_bytes());
        data.extend_from_slice(&rec.atime.to_le_bytes());
        let _ = graph.redo_log.append(OpType::VertexIndexUpdate, rec.vertex_id as u64, &data);

        return Ok(());
    }

    // Try edge.
    if let Ok(mut rec) = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset) {
        if rec.rank == 0 {
            return Ok(());
        }
        let old_rank = rec.rank;
        rec.rank = rec.rank.saturating_sub(1);

        graph.index_file
            .update_edge_record(ptr.block_idx, ptr.chunk_offset, &rec)
            .map_err(|e| format!("index write: {}", e))?;

        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.ranks.remove(old_rank, ptr);
        mi.ranks.insert(rec.rank, *ptr);

        let mut data = Vec::with_capacity(12);
        data.extend_from_slice(&rec.rank.to_le_bytes());
        data.extend_from_slice(&rec.atime.to_le_bytes());
        let _ = graph.redo_log.append(OpType::EdgeIndexUpdate, rec.edge_id as u64, &data);

        return Ok(());
    }

    Err("neither vertex nor edge record found".to_string())
}
