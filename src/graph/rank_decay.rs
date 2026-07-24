//! Periodic rank decay — scans for inactive vertices/edges and decrements
//! their rank so that popularity naturally fades over time.
//!
//! Uses the `atime_index` in `MemoryIndex` for efficient range scans.

use std::sync::Arc;
use std::time::Duration;

use crate::graph::graph::Graph;

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
            let now = crate::storage::types::timestamp_us();
            let threshold = now.saturating_sub(inactive_threshold_us);

            // Collect inactive pointers under a read lock.
            let inactive: Vec<(u64, crate::storage::memory_index::MetaPointer)> = {
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

/// Attempt to decrement rank for a single entity by its data pointer.
fn try_decay(
    graph: &Arc<Graph>,
    _old_atime: u64,
    ptr: &crate::storage::memory_index::MetaPointer,
) -> Result<(), String> {
    // Read the DataHeader to determine entity type and ID.
    let dh = crate::graph::crud::read_header_by_ptr(graph, ptr)
        .map_err(|e| format!("read_header: {}", e))?;
    if dh.rank == 0 {
        return Ok(()); // already at minimum
    }
    let old_rank = dh.rank;
    let new_rank = old_rank.saturating_sub(1);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    match dh.chunk_type {
        crate::storage::types::ChunkType::Vertex => {
            // Update rank/atime indexes.
            let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
            mi.ranks.remove(old_rank, ptr);
            mi.ranks.insert(new_rank, *ptr);
            mi.atime_index.remove(_old_atime, ptr);
            mi.atime_index.insert(now, *ptr);
            drop(mi);

            // Persist to DataHeader in-place (no WAL — rank decay is soft state).
            let mut hdr = dh;
            hdr.rank = new_rank;
            hdr.atime = now;
            hdr.mtime = now;
            let _ = crate::graph::crud::update_header_in_place(graph, ptr, &hdr);
        }
        crate::storage::types::ChunkType::Edge => {
            let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
            mi.ranks.remove(old_rank, ptr);
            mi.ranks.insert(new_rank, *ptr);
            mi.atime_index.remove(_old_atime, ptr);
            mi.atime_index.insert(now, *ptr);
            drop(mi);

            // Persist to DataHeader in-place.
            let mut hdr = dh;
            hdr.rank = new_rank;
            hdr.atime = now;
            hdr.mtime = now;
            let _ = crate::graph::crud::update_header_in_place(graph, ptr, &hdr);
        }
        _ => return Err("unknown chunk type".to_string()),
    }

    Ok(())
}
