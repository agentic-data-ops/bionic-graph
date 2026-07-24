//! Vertex/Edge CRUD operations for the block-based graph engine.

use std::collections::HashMap;

use crate::graph::graph::Graph;
use crate::graph::serialize::{self, deserialize_edge, deserialize_vertex, serialize_edge, serialize_vertex};
use crate::graph::tokenizer::Tokenizer;
use crate::storage::block_allocator::BlockAllocator;
use crate::storage::memory_index::MetaPointer;
use crate::storage::redo_log::RedoLogEntry;
use crate::storage::types::{
    BlockHeader, DataHeader, DataStatus, EdgePayload, HistoryRecord, OpType, PropertyValue,
    StorageError, StorageResult, TokenPayload, TokenRef, VertexPayload, BLOCK_SIZE, DATA_HEADER_SIZE,
    timestamp_us,
};

// ── Create ──────────────────────────────────────────────────────────────────

/// Create a vertex. Returns the new vertex ID.
pub fn create_vertex(
    graph: &Graph,
    name: &str,
    labels: &[String],
    keywords: &[String],
    properties: &HashMap<String, PropertyValue>,
) -> StorageResult<u32> {
    let vid = graph.alloc_vertex_id();

    let payload = VertexPayload {
        name: name.to_string(),
        labels: labels.to_vec(),
        keywords: keywords.to_vec(),
        properties: properties.clone(),
        history: Vec::new(),
    };

    let serialized = serialize_vertex(&payload)?;
    let header = DataHeader::new_vertex(vid, serialized.len() as u16);
    let ptr = write_data_record(graph, &header, &serialized)?;

    // ── Update memory index ──────────────────────────────────────────
    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.vertices.insert(vid, ptr);
        mi.vertex_names.insert(payload.name.clone(), vid);
        mi.ranks.insert(1, ptr);
    }

    // ── Tokenize attributes ──────────────────────────────────────────
    tokenize_vertex(&graph, vid, &payload)?;

    // ── WAL ──────────────────────────────────────────────────────────
    graph.redo_log.append(OpType::VertexCreate, vid as u64, &serialized)?;

    Ok(vid)
}

/// Create an edge. Returns the new edge ID.
pub fn create_edge(
    graph: &Graph,
    source: u32,
    target: u32,
    name: &str,
    labels: &[String],
    keywords: &[String],
    strength: f32,
    properties: &HashMap<String, PropertyValue>,
) -> StorageResult<u32> {
    let eid = graph.alloc_edge_id();

    let payload = EdgePayload {
        name: name.to_string(),
        labels: labels.to_vec(),
        keywords: keywords.to_vec(),
        strength,
        properties: properties.clone(),
        source,
        target,
        history: Vec::new(),
    };

    let serialized = serialize_edge(&payload)?;
    let header = DataHeader::new_edge(eid, serialized.len() as u16);
    let ptr = write_data_record(graph, &header, &serialized)?;

    // ── Update memory index ──────────────────────────────────────────
    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.edges.insert(eid, ptr);
        mi.edge_names.insert(payload.name.clone(), eid);
        mi.ranks.insert(1, ptr);
        mi.adjacency.add_edge(eid, source, target, ptr);
    }

    // ── Tokenize ─────────────────────────────────────────────────────
    tokenize_edge(&graph, eid, &payload)?;

    // ── WAL ──────────────────────────────────────────────────────────
    graph.redo_log.append(OpType::EdgeCreate, eid as u64, &serialized)?;

    Ok(eid)
}

// ── Read ────────────────────────────────────────────────────────────────────

/// Get a vertex by ID. Returns `None` if not found or soft-deleted.
pub fn get_vertex(graph: &Graph, vid: u32) -> StorageResult<Option<VertexPayload>> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.vertices.get(vid).copied()
    };
    let Some(ptr) = ptr else { return Ok(None) };

    let header = read_data_header(graph, ptr)?;
    if header.status == DataStatus::Deleted {
        return Ok(None);
    }

    let payload_len = header.payload_len as usize;
    let data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, payload_len as u16)?;
    let payload = deserialize_vertex(&data)?;

    // Update atime and rank.
    update_rank_and_atime(graph, vid, &ptr)?;

    Ok(Some(payload))
}

/// Get an edge by ID. Returns `None` if not found or soft-deleted.
pub fn get_edge(graph: &Graph, eid: u32) -> StorageResult<Option<EdgePayload>> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.edges.get(eid).copied()
    };
    let Some(ptr) = ptr else { return Ok(None) };

    let header = read_data_header(graph, ptr)?;
    if header.status == DataStatus::Deleted {
        return Ok(None);
    }

    let payload_len = header.payload_len as usize;
    let data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, payload_len as u16)?;
    let payload = deserialize_edge(&data)?;

    // Update atime and rank.
    update_rank_and_atime(graph, eid, &ptr)?;

    Ok(Some(payload))
}

/// Update a vertex's metadata (rank, atime). Name changes go through
/// `update_vertex` (full payload rewrite) instead.
/// Updates are persisted to the DataHeader in-place (no WAL entry needed).
pub fn update_vertex_meta(graph: &Graph, vid: u32, new_rank: Option<u32>, new_atime: Option<u64>) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.vertices.get(vid).copied()
    }.ok_or_else(|| StorageError::Other(format!("vertex {} not found", vid)))?;

    let header = read_data_header(graph, ptr)?;
    let old_rank = header.rank;
    let old_atime = header.atime;

    let rank = new_rank.unwrap_or(old_rank);
    let atime = new_atime.unwrap_or(old_atime);

    if rank == old_rank && atime == old_atime {
        return Ok(());
    }

    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        if old_rank != rank {
            mi.ranks.remove(old_rank, &ptr);
            mi.ranks.insert(rank, ptr);
        }
        if old_atime != atime {
            mi.atime_index.remove(old_atime, &ptr);
            mi.atime_index.insert(atime, ptr);
        }
    }

    // Persist to DataHeader in-place (no WAL — rank/atime are soft state).
    if let Ok(mut hdr) = read_header_by_ptr(graph, &ptr) {
        hdr.rank = rank;
        hdr.atime = atime;
        hdr.mtime = atime;
        let _ = update_header_in_place(graph, &ptr, &hdr);
    }

    Ok(())
}

/// Update an edge's metadata (rank, atime). Name changes go through
/// `update_edge` (full payload rewrite) instead.
pub fn update_edge_meta(graph: &Graph, eid: u32, new_rank: Option<u32>, new_atime: Option<u64>) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.edges.get(eid).copied()
    }.ok_or_else(|| StorageError::Other(format!("edge {} not found", eid)))?;

    let header = read_data_header(graph, ptr)?;
    let old_rank = header.rank;
    let old_atime = header.atime;

    let rank = new_rank.unwrap_or(old_rank);
    let atime = new_atime.unwrap_or(old_atime);

    if rank == old_rank && atime == old_atime {
        return Ok(());
    }

    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        if old_rank != rank {
            mi.ranks.remove(old_rank, &ptr);
            mi.ranks.insert(rank, ptr);
        }
        if old_atime != atime {
            mi.atime_index.remove(old_atime, &ptr);
            mi.atime_index.insert(atime, ptr);
        }
    }

    // Persist to DataHeader in-place.
    if let Ok(mut hdr) = read_header_by_ptr(graph, &ptr) {
        hdr.rank = rank;
        hdr.atime = atime;
        hdr.mtime = atime;
        let _ = update_header_in_place(graph, &ptr, &hdr);
    }

    Ok(())
}

// ── Update ──────────────────────────────────────────────────────────────────

/// Update a vertex's mutable fields.
pub fn update_vertex(
    graph: &Graph,
    vid: u32,
    name: Option<&str>,
    labels: Option<&[String]>,
    keywords: Option<&[String]>,
    properties: Option<&HashMap<String, PropertyValue>>,
    record_history: bool,
) -> StorageResult<()> {
    // Read current state.
    let (old_payload, old_ptr, old_header) = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        let ptr = mi.vertices.get(vid).copied()
            .ok_or_else(|| StorageError::Other(format!("vertex {} not found", vid)))?;
        let header = read_data_header(graph, ptr)?;
        let payload_len = header.payload_len as usize;
        let data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, payload_len as u16)?;
        let payload = deserialize_vertex(&data)?;
        (payload, ptr, header)
    };

    let mut new_payload = old_payload.clone();
    if let Some(n) = name {
        new_payload.name = n.to_string();
    }
    if let Some(l) = labels {
        new_payload.labels = l.to_vec();
    }
    if let Some(k) = keywords {
        new_payload.keywords = k.to_vec();
    }
    if let Some(p) = properties {
        new_payload.properties = p.clone();
    }

    // Push old payload to history if requested.
    // The history entry's timestamp is the old header's mtime — the moment
    // this state snapshot was last current before being superseded.
    if record_history {
        let old_bytes = serialize_vertex(&old_payload)?;
        new_payload.history.push(HistoryRecord {
            timestamp: old_header.mtime,
            data: old_bytes,
        });
    }

    // Serialize and allocate new chunks (copy-on-write).
    let serialized = serialize_vertex(&new_payload)?;
    let now = timestamp_us();
    let new_header = DataHeader {
        chunk_type: crate::storage::types::ChunkType::Vertex,
        status: DataStatus::Normal,
        version: old_header.version.wrapping_add(1),
        entity_id: vid,
        ctime: old_header.ctime,
        mtime: now,
        atime: now,
        rank: old_header.rank.wrapping_add(1),
        payload_len: serialized.len() as u16,
    };

    let new_ptr = write_data_record(graph, &new_header, &serialized)?;

    // Update cached metadata.
    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.vertices.insert(vid, new_ptr);

        mi.ranks.remove(old_header.rank, &old_ptr);
        mi.ranks.insert(new_header.rank, new_ptr);
        mi.atime_index.remove(old_header.atime, &old_ptr);
        mi.atime_index.insert(now, new_ptr);
        if let Some(n) = name {
            mi.vertex_names.remove(&old_payload.name);
            mi.vertex_names.insert(n.to_string(), vid);
        }
    }

    // Free old data chunks (header + payload).
    let old_total_len = DATA_HEADER_SIZE + old_header.payload_len as usize;
    let old_chunks = BlockAllocator::chunks_needed(old_total_len);
    free_data_chunks(graph, old_ptr.block_idx, old_ptr.chunk_offset, old_chunks)?;

    // Re-tokenize if relevant fields changed.
    tokenize_vertex(graph, vid, &new_payload)?;

    // WAL: log data payload update.
    graph.redo_log.append(OpType::VertexUpdate, vid as u64, &serialized)?;

    Ok(())
}

/// Update an edge's mutable fields.
pub fn update_edge(
    graph: &Graph,
    eid: u32,
    name: Option<&str>,
    labels: Option<&[String]>,
    keywords: Option<&[String]>,
    strength: Option<f32>,
    properties: Option<&HashMap<String, PropertyValue>>,
    record_history: bool,
) -> StorageResult<()> {
    let (old_payload, old_ptr, old_header) = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        let ptr = mi.edges.get(eid).copied()
            .ok_or_else(|| StorageError::Other(format!("edge {} not found", eid)))?;
        let header = read_data_header(graph, ptr)?;
        let payload_len = header.payload_len as usize;
        let data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, payload_len as u16)?;
        let payload = deserialize_edge(&data)?;
        (payload, ptr, header)
    };

    let mut new_payload = old_payload.clone();
    if let Some(n) = name {
        new_payload.name = n.to_string();
    }
    if let Some(l) = labels {
        new_payload.labels = l.to_vec();
    }
    if let Some(k) = keywords {
        new_payload.keywords = k.to_vec();
    }
    if let Some(s) = strength {
        new_payload.strength = s;
    }
    if let Some(p) = properties {
        new_payload.properties = p.clone();
    }

    if record_history {
        let old_bytes = serialize_edge(&old_payload)?;
        new_payload.history.push(HistoryRecord {
            timestamp: old_header.mtime,
            data: old_bytes,
        });
    }

    let serialized = serialize_edge(&new_payload)?;
    let now = timestamp_us();
    let new_header = DataHeader {
        chunk_type: crate::storage::types::ChunkType::Edge,
        status: DataStatus::Normal,
        version: old_header.version.wrapping_add(1),
        entity_id: eid,
        ctime: old_header.ctime,
        mtime: now,
        atime: now,
        rank: old_header.rank.wrapping_add(1),
        payload_len: serialized.len() as u16,
    };

    let new_ptr = write_data_record(graph, &new_header, &serialized)?;

    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.edges.insert(eid, new_ptr);

        mi.ranks.remove(old_header.rank, &old_ptr);
        mi.ranks.insert(new_header.rank, new_ptr);
        mi.atime_index.remove(old_header.atime, &old_ptr);
        mi.atime_index.insert(now, new_ptr);

        // Update adjacency index with new pointer.
        mi.adjacency.remove_edge(old_payload.source, old_payload.target, &old_ptr);
        mi.adjacency.add_edge(eid, old_payload.source, old_payload.target, new_ptr);
    }

    let old_total_len = DATA_HEADER_SIZE + old_header.payload_len as usize;
    let old_chunks = BlockAllocator::chunks_needed(old_total_len);
    free_data_chunks(graph, old_ptr.block_idx, old_ptr.chunk_offset, old_chunks)?;

    tokenize_edge(graph, eid, &new_payload)?;
    graph.redo_log.append(OpType::EdgeUpdate, eid as u64, &serialized)?;

    Ok(())
}

// ── Delete ──────────────────────────────────────────────────────────────────

/// Soft-delete a vertex: mark as deleted in header, but keep data for time-travel.
pub fn soft_delete_vertex(graph: &Graph, vid: u32) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.vertices.get(vid).copied()
            .ok_or_else(|| StorageError::Other(format!("vertex {} not found", vid)))?
    };

    let mut header = read_data_header(graph, ptr)?;
    let old_rank = header.rank;
    header.status = DataStatus::Deleted;
    header.mtime = timestamp_us();

    // Update header in data file.
    update_header_in_place(graph, &ptr, &header)?;

    // Remove from ranks in cache.
    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.ranks.remove(old_rank, &ptr);
    }

    graph.redo_log.append(OpType::VertexDelete, vid as u64, &[])?;
    Ok(())
}

/// Hard-delete a vertex: remove data entirely.
pub fn hard_delete_vertex(graph: &Graph, vid: u32) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.vertices.get(vid).copied()
            .ok_or_else(|| StorageError::Other(format!("vertex {} not found", vid)))?
    };

    let header = read_data_header(graph, ptr)?;

    // Free data chunks (header + payload).
    let total_len = DATA_HEADER_SIZE + header.payload_len as usize;
    let chunks = BlockAllocator::chunks_needed(total_len);
    free_data_chunks(graph, ptr.block_idx, ptr.chunk_offset, chunks)?;

    // Remove from all caches.
    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        // Name must be read from payload before removal.
        // Read the payload to get the name.
        let payload_len = header.payload_len as usize;
        let data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, payload_len as u16)?;
        if let Ok(payload) = deserialize_vertex(&data) {
            mi.vertex_names.remove(&payload.name);
        }
        mi.vertices.remove(vid);
        mi.ranks.remove(header.rank, &ptr);
    }

    graph.redo_log.append(OpType::VertexDelete, vid as u64, &[])?;
    Ok(())
}

/// Soft-delete an edge.
pub fn soft_delete_edge(graph: &Graph, eid: u32) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.edges.get(eid).copied()
            .ok_or_else(|| StorageError::Other(format!("edge {} not found", eid)))?
    };

    let mut header = read_data_header(graph, ptr)?;
    let old_rank = header.rank;
    header.status = DataStatus::Deleted;
    header.mtime = timestamp_us();

    // Update header in data file.
    update_header_in_place(graph, &ptr, &header)?;

    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.ranks.remove(old_rank, &ptr);
        // Keep edge in adjacency for time-travel traversal
    }

    graph.redo_log.append(OpType::EdgeDelete, eid as u64, &[])?;
    Ok(())
}

/// Hard-delete an edge.
pub fn hard_delete_edge(graph: &Graph, eid: u32) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.edges.get(eid).copied()
            .ok_or_else(|| StorageError::Other(format!("edge {} not found", eid)))?
    };

    let header = read_data_header(graph, ptr)?;

    let total_len = DATA_HEADER_SIZE + header.payload_len as usize;
    let chunks = BlockAllocator::chunks_needed(total_len);
    free_data_chunks(graph, ptr.block_idx, ptr.chunk_offset, chunks)?;

    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        // Read payload for name and source/target before removal.
        let payload_len = header.payload_len as usize;
        let data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, payload_len as u16)?;
        if let Ok(payload) = deserialize_edge(&data) {
            mi.edge_names.remove(&payload.name);
            mi.adjacency.remove_edge(payload.source, payload.target, &ptr);
        }
        mi.edges.remove(eid);
        mi.ranks.remove(header.rank, &ptr);
    }

    graph.redo_log.append(OpType::EdgeDelete, eid as u64, &[])?;
    Ok(())
}

// ── WAL replay ──────────────────────────────────────────────────────────────

/// Replay a single WAL entry during graph startup recovery.
pub fn replay_entry(graph: &Graph, entry: &RedoLogEntry) -> StorageResult<()> {
    match entry.op_type {
        OpType::VertexCreate => {
            let id = entry.op_id as u32;
            if id >= graph.next_vertex_id.load(std::sync::atomic::Ordering::Relaxed) {
                graph.next_vertex_id.store(id + 1, std::sync::atomic::Ordering::Relaxed);
            }
            if let Ok(payload) = deserialize_vertex(&entry.data) {
                // Always re-apply: data in dirty cache may have been lost.
                replay_create_vertex(graph, id, &payload, &entry.data)?;
            }
        }
        OpType::VertexUpdate => {
            let id = entry.op_id as u32;
            if id >= graph.next_vertex_id.load(std::sync::atomic::Ordering::Relaxed) {
                graph.next_vertex_id.store(id + 1, std::sync::atomic::Ordering::Relaxed);
            }
            if let Ok(payload) = deserialize_vertex(&entry.data) {
                // Always write the update — do NOT skip even if vertex exists,
                // because the data file may have the stale pre-update state
                // (the update's new data record was only in dirty cache).
                replay_create_vertex_always(graph, id, &payload, &entry.data)?;
            }
        }
        OpType::EdgeCreate => {
            let id = entry.op_id as u32;
            if id >= graph.next_edge_id.load(std::sync::atomic::Ordering::Relaxed) {
                graph.next_edge_id.store(id + 1, std::sync::atomic::Ordering::Relaxed);
            }
            if let Ok(payload) = deserialize_edge(&entry.data) {
                replay_create_edge(graph, id, &payload, &entry.data)?;
            }
        }
        OpType::EdgeUpdate => {
            let id = entry.op_id as u32;
            if id >= graph.next_edge_id.load(std::sync::atomic::Ordering::Relaxed) {
                graph.next_edge_id.store(id + 1, std::sync::atomic::Ordering::Relaxed);
            }
            if let Ok(payload) = deserialize_edge(&entry.data) {
                replay_create_edge_always(graph, id, &payload, &entry.data)?;
            }
        }
        OpType::VertexDelete => {
            let id = entry.op_id as u32;
            if graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).vertices.get(id).is_some() {
                let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                mi.vertices.remove(id);
            }
        }
        OpType::EdgeDelete => {
            let id = entry.op_id as u32;
            if let Some(&ptr) = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).edges.get(id) {
                // Read source/target from data header payload before removal.
                let (source, target) = {
                    let header = read_data_header(graph, ptr)?;
                    let payload_len = header.payload_len as usize;
                    let data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, payload_len as u16)
                        .unwrap_or_default();
                    if let Ok(payload) = deserialize_edge(&data) {
                        (payload.source, payload.target)
                    } else {
                        (0, 0)
                    }
                };
                let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                mi.edges.remove(id);
                // Use the real source/target vertex IDs, NOT edge_id, to properly
                // clean up the adjacency index.
                mi.adjacency.remove_edge(source, target, &ptr);
            }
        }
        OpType::TokenCreate | OpType::TokenUpdate | OpType::TokenDelete => {
            // Token state is rebuilt from data file at startup; no WAL replay needed.
        }
    }
    Ok(())
}

// ── Replay helpers ───────────────────────────────────────────────────────────

/// Replay helper: recreate a vertex from WAL data during startup recovery.
fn replay_create_vertex(graph: &Graph, id: u32, payload: &VertexPayload, wal_data: &[u8]) -> StorageResult<()> {
    // Skip if this vertex was already re-created during build_memory_index.
    {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        if mi.vertices.contains(id) {
            return Ok(());
        }
    }

    let serialized = wal_data.to_vec();
    let header = DataHeader::new_vertex(id, serialized.len() as u16);
    let ptr = write_data_record(graph, &header, &serialized)?;

    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    mi.vertices.insert(id, ptr);
    mi.vertex_names.insert(payload.name.clone(), id);
    mi.ranks.insert(header.rank, ptr);
    drop(mi);

    tokenize_vertex(graph, id, payload)?;
    Ok(())
}

/// Replay helper: write a vertex data record unconditionally (no duplicate check).
/// Used for VertexUpdate replay, where the WAL entry may contain a newer state
/// than what's on disk (if the update's dirty blocks weren't flushed before crash).
fn replay_create_vertex_always(graph: &Graph, id: u32, payload: &VertexPayload, wal_data: &[u8]) -> StorageResult<()> {
    let serialized = wal_data.to_vec();
    let header = DataHeader::new_vertex(id, serialized.len() as u16);
    let ptr = write_data_record(graph, &header, &serialized)?;

    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    mi.vertices.insert(id, ptr);
    mi.vertex_names.insert(payload.name.clone(), id);
    mi.ranks.insert(header.rank, ptr);
    drop(mi);

    tokenize_vertex(graph, id, payload)?;
    Ok(())
}

/// Replay helper: recreate an edge from WAL data during startup recovery.
fn replay_create_edge(graph: &Graph, id: u32, payload: &EdgePayload, wal_data: &[u8]) -> StorageResult<()> {
    // Skip if this edge was already re-created during build_memory_index.
    {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        if mi.edges.contains(id) {
            return Ok(());
        }
    }

    let serialized = wal_data.to_vec();
    let header = DataHeader::new_edge(id, serialized.len() as u16);
    let ptr = write_data_record(graph, &header, &serialized)?;

    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    mi.edges.insert(id, ptr);
    mi.edge_names.insert(payload.name.clone(), id);
    mi.ranks.insert(header.rank, ptr);
    mi.adjacency.add_edge(id, payload.source, payload.target, ptr);
    drop(mi);

    tokenize_edge(graph, id, payload)?;
    Ok(())
}

/// Replay helper: write an edge data record unconditionally (no duplicate check).
/// Used for EdgeUpdate replay, same rationale as replay_create_vertex_always.
fn replay_create_edge_always(graph: &Graph, id: u32, payload: &EdgePayload, wal_data: &[u8]) -> StorageResult<()> {
    let serialized = wal_data.to_vec();
    let header = DataHeader::new_edge(id, serialized.len() as u16);
    let ptr = write_data_record(graph, &header, &serialized)?;

    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    mi.edges.insert(id, ptr);
    mi.edge_names.insert(payload.name.clone(), id);
    mi.ranks.insert(header.rank, ptr);
    mi.adjacency.add_edge(id, payload.source, payload.target, ptr);
    drop(mi);

    tokenize_edge(graph, id, payload)?;
    Ok(())
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Allocate chunks for a new piece of data. Returns (block_idx, chunk_offset).
fn allocate_chunks(graph: &Graph, chunks_needed: u8) -> StorageResult<(u32, u8)> {
    let mut bf = graph.bitmap_file.write().unwrap_or_else(|e| e.into_inner());

    loop {
        let block_idx = bf.alloc_block(|count| {
            graph.data_file.allocate_blocks(count)
        })?;

        let mut cache = graph.block_cache.write().unwrap_or_else(|e| e.into_inner());
        let block = cache.get_or_load(block_idx,
            |idx| graph.data_file.read_block(idx),
            &|idx, data| graph.data_file.write_block(idx, data),
        )?;

        let mut header = BlockHeader::decode(block);
        if let Some(off) = BlockAllocator::alloc_chunks(&mut header.bitmap, chunks_needed) {
            header.encode(block);
            let was_full = BlockAllocator::is_block_full(&header.bitmap);
            cache.mark_dirty(block_idx);
            drop(cache);

            if was_full {
                bf.mark_full(block_idx)?;
            }
            return Ok((block_idx, off));
        }

        // This block doesn't have enough contiguous free chunks (fragmented).
        // Mark it full so alloc_block skips it, then try the next block.
        drop(cache);
        bf.mark_full(block_idx)?;
        // Continue loop to try next block
    }
}

/// Write padded data into the allocated chunks.
fn write_data_chunks(graph: &Graph, block_idx: u32, chunk_offset: u8, chunks: u8, data: &[u8]) -> StorageResult<()> {
    // Write data into the block through cache, then flush to disk.
    let _block_copy = {
        let mut cache = graph.block_cache.write().unwrap_or_else(|e| e.into_inner());
        cache.with_block(block_idx,
            |idx| graph.data_file.read_block(idx),
            &|idx, data| graph.data_file.write_block(idx, data),
            |block| {
                let start = (chunk_offset as usize) * 64;
                let end = start + (chunks as usize) * 64;
                let write_len = data.len().min(end - start);
                block[start..start + write_len].copy_from_slice(&data[..write_len]);
                *block  // copy for disk flush
            },
        )?
    };
    Ok(())
}

/// Read data from chunks given the total data length.
pub(crate) fn read_data_chunks(graph: &Graph, block_idx: u32, chunk_offset: u8, data_len: u16) -> StorageResult<Vec<u8>> {
    let _chunks = BlockAllocator::chunks_needed(data_len as usize);
    let mut cache = graph.block_cache.write().unwrap_or_else(|e| e.into_inner());
    let block = cache.get_or_load(block_idx, |idx| {
        graph.data_file.read_block(idx)
    }, &|idx, data| {
        graph.data_file.write_block(idx, data)
    })?;

    let start = (chunk_offset as usize) * 64;
    let read_len = data_len as usize;
    // Clamp to block boundary to avoid slice index out of bounds.
    let end = (start + read_len).min(BLOCK_SIZE);
    let avail = end - start;
    if avail < read_len {
        log::warn!(
            "read_data_chunks: truncated read at block={} chunk_offset={}: requested {} bytes, available {}",
            block_idx, chunk_offset, read_len, avail,
        );
    }
    let mut data = vec![0u8; avail];
    data.copy_from_slice(&block[start..end]);
    Ok(data)
}

/// Free previously allocated data chunks.
fn free_data_chunks(graph: &Graph, block_idx: u32, chunk_offset: u8, chunks: u8) -> StorageResult<()> {
    let was_full = {
        let mut cache = graph.block_cache.write().unwrap_or_else(|e| e.into_inner());
        if cache.contains(block_idx) {
            let block = cache.get_or_load(block_idx, |idx| {
                graph.data_file.read_block(idx)
            }, &|idx, data| {
                graph.data_file.write_block(idx, data)
            })?;
            let mut header = BlockHeader::decode(block);
            let was_full = BlockAllocator::is_block_full(&header.bitmap);
            BlockAllocator::free_chunks(&mut header.bitmap, chunk_offset, chunks);
            header.encode(block);
            cache.mark_dirty(block_idx);
            was_full && !BlockAllocator::is_block_full(&header.bitmap)
        } else {
            false
        }
    };

    if was_full {
        let mut bf = graph.bitmap_file.write().unwrap_or_else(|e| e.into_inner());
        bf.mark_free(block_idx)?;
    }
    Ok(())
}

/// Extract tokens from vertex attributes and index them.
fn tokenize_vertex(graph: &Graph, vid: u32, payload: &VertexPayload) -> StorageResult<()> {
    let mut attrs = Vec::new();
    attrs.push(("name", payload.name.as_str()));
    for label in &payload.labels {
        attrs.push(("labels", label.as_str()));
    }
    for kw in &payload.keywords {
        attrs.push(("keywords", kw.as_str()));
    }
    for (key, val) in &payload.properties {
        if let PropertyValue::String(s) = val {
            attrs.push((key, s.as_str()));
        }
    }

    let tokens = Tokenizer::extract_tokens(&attrs);
    for (token_str, hits) in &tokens {
        add_token(graph, token_str, 0u8, vid, hits)?;
    }
    Ok(())
}

/// Extract tokens from edge attributes and index them.
fn tokenize_edge(graph: &Graph, eid: u32, payload: &EdgePayload) -> StorageResult<()> {
    let mut attrs = Vec::new();
    attrs.push(("name", payload.name.as_str()));
    for lbl in &payload.labels {
        attrs.push(("labels", lbl.as_str()));
    }
    for kw in &payload.keywords {
        attrs.push(("keywords", kw.as_str()));
    }
    for (key, val) in &payload.properties {
        if let PropertyValue::String(s) = val {
            attrs.push((key, s.as_str()));
        }
    }

    let tokens = Tokenizer::extract_tokens(&attrs);
    log::debug!("tokenize_edge eid={}: attrs={:?} -> {} tokens: {:?}", eid, attrs.iter().map(|(k,v)| format!("{}={}",k,v)).collect::<Vec<_>>(), tokens.len(), tokens.iter().map(|(t,_)| t.clone()).collect::<Vec<_>>());
    for (token_str, hits) in &tokens {
        add_token(graph, token_str, 1u8, eid, hits)?;
    }
    Ok(())
}

/// Add or update a token entry.
fn add_token(graph: &Graph, token_str: &str, ref_type: u8, ref_id: u32, hits: &[crate::storage::types::Hit]) -> StorageResult<()> {
    // Check if token already exists in memory index.
    let existing = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.tokens.get(token_str).map(|v| v.clone())
    };

    if let Some(ptrs) = existing {
        // Update the existing token's TokenPayload in the data file.
        if let Some(&ptr) = ptrs.first() {
            // Read existing header to get payload length and location.
            let header = read_data_header(graph, ptr)?;
            let payload_len = header.payload_len as usize;
            let existing_data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, payload_len as u16)?;
            if let Ok(mut token_payload) = crate::graph::serialize::deserialize_token(&existing_data) {
                token_payload.refs.push(TokenRef {
                    ref_type,
                    ref_id,
                    ref_version: 1,
                    ref_frequency: hits.len() as u16,
                    hits: hits.to_vec(),
                });
                let new_data = crate::graph::serialize::serialize_token(&token_payload)?;
                let new_header = DataHeader {
                    chunk_type: crate::storage::types::ChunkType::Token,
                    status: DataStatus::Normal,
                    version: 0,
                    entity_id: token_payload.id,
                    ctime: header.ctime,
                    mtime: 0,
                    atime: 0,
                    rank: 0,
                    payload_len: new_data.len() as u16,
                };

                // Allocate new space and write DataHeader + payload.
                let new_ptr = write_data_record(graph, &new_header, &new_data)?;

                // Update token pointer in memory index.
                let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                mi.tokens.remove_pointer(token_str, &ptr);
                mi.tokens.insert(token_str.to_string(), new_ptr);

                // Free old data chunks (header + payload).
                let old_total = (DATA_HEADER_SIZE + payload_len) as u16;
                let old_chunks = BlockAllocator::chunks_needed(old_total as usize);
                free_data_chunks(graph, ptr.block_idx, ptr.chunk_offset, old_chunks)?;
            }
        }
    } else {
        // Create new token.
        let token_payload = TokenPayload {
            id: graph.alloc_token_id(),
            token: token_str.to_string(),
            refs: vec![TokenRef {
                ref_type,
                ref_id,
                ref_version: 1,
                ref_frequency: hits.len() as u16,
                hits: hits.to_vec(),
            }],
        };
        let data = serialize::serialize_token(&token_payload)?;
        let header = DataHeader::new_token(token_payload.id, data.len() as u16);
        let ptr = write_data_record(graph, &header, &data)?;

        // Update memory index.
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.tokens.insert(token_str.to_string(), ptr);
    }

    Ok(())
}

/// Update access time and increment rank for a vertex/edge, reading the DataHeader
/// directly from the data file and persisting the update in-place.
fn update_rank_and_atime(graph: &Graph, id: u32, ptr: &MetaPointer) -> StorageResult<()> {
    let now = timestamp_us();

    let mut header = read_data_header(graph, *ptr)?;
    let old_rank = header.rank;
    let old_atime = header.atime;
    let new_rank = header.rank.wrapping_add(1);
    header.atime = now;
    header.rank = new_rank;

    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.ranks.remove(old_rank, ptr);
        mi.ranks.insert(new_rank, *ptr);
        mi.atime_index.remove(old_atime, ptr);
        mi.atime_index.insert(now, *ptr);
    }

    // Persist to DataHeader in-place.
    update_header_in_place(graph, ptr, &header)?;

    Ok(())
}

// ── Read-by-ptr helpers (for Gremlin engine) ───────────────────────────────

/// Read a vertex payload given its pointer and optional query time.
/// Reads the DataHeader from the data file to determine status, timestamps,
/// and payload length.
pub fn read_vertex_by_ptr(
    graph: &Graph,
    ptr: MetaPointer,
    at: Option<u64>,
) -> StorageResult<Option<VertexPayload>> {
    let header = read_data_header(graph, ptr)?;
    let payload_len = header.payload_len as usize;

    // Time-travel: check existence/reachability at the query time.
    if let Some(timestamp) = at {
        if timestamp < header.ctime {
            return Ok(None); // didn't exist yet
        }
        let payload: VertexPayload = deserialize_vertex(&read_data_payload(
            graph,
            ptr.block_idx,
            ptr.chunk_offset + 1, // skip DataHeader
            payload_len,
        )?)?;

        // Walk history newest-first. Each history entry's timestamp is the
        // time this state snapshot became current (its start of validity).
        for h in payload.history.iter().rev() {
            if h.timestamp <= timestamp {
                if timestamp < header.mtime {
                    // This snapshot was valid at the query time.
                    let hist_payload = deserialize_vertex(&h.data)?;
                    return Ok(Some(hist_payload));
                }
                // at >= meta.mtime means the current state is the active one.
                break;
            }
        }
        // Query time falls within the current payload's validity, or
        // nothing exists yet. Check deletion.
        if header.status == DataStatus::Deleted && timestamp >= header.mtime {
            return Ok(None);
        }
        if timestamp >= header.ctime {
            return Ok(Some(payload));
        }
        // Fall through to normal path below
    }

    // Normal (non-time-travel) path: deleted entities are hidden.
    if header.status == DataStatus::Deleted {
        return Ok(None);
    }
    let payload: VertexPayload = deserialize_vertex(&read_data_payload(
        graph,
        ptr.block_idx,
        ptr.chunk_offset + 1, // skip DataHeader
        payload_len,
    )?)?;
    Ok(Some(payload))
}

/// Read an edge payload given its pointer and optional query time.
pub fn read_edge_by_ptr(
    graph: &Graph,
    ptr: MetaPointer,
    at: Option<u64>,
) -> StorageResult<Option<EdgePayload>> {
    let header = read_data_header(graph, ptr)?;
    let payload_len = header.payload_len as usize;

    if let Some(timestamp) = at {
        if timestamp < header.ctime {
            return Ok(None);
        }
        let payload: EdgePayload = deserialize_edge(&read_data_payload(
            graph,
            ptr.block_idx,
            ptr.chunk_offset + 1, // skip DataHeader
            payload_len,
        )?)?;

        for h in payload.history.iter().rev() {
            if h.timestamp <= timestamp {
                if timestamp < header.mtime {
                    let hist_payload = deserialize_edge(&h.data)?;
                    return Ok(Some(hist_payload));
                }
                break;
            }
        }
        if header.status == DataStatus::Deleted && timestamp >= header.mtime {
            return Ok(None);
        }
        if timestamp >= header.ctime {
            return Ok(Some(payload));
        }
    }

    if header.status == DataStatus::Deleted {
        return Ok(None);
    }
    let payload: EdgePayload = deserialize_edge(&read_data_payload(
        graph,
        ptr.block_idx,
        ptr.chunk_offset + 1, // skip DataHeader
        payload_len,
    )?)?;
    Ok(Some(payload))
}

/// Read a token payload given its pointer in the data file and payload length.
/// Replaces the old `read_token_by_record`.
pub fn read_token_by_ptr(
    graph: &Graph,
    ptr: MetaPointer,
    data_len: u16,
) -> StorageResult<Option<TokenPayload>> {
    let payload_len = data_len as usize;
    let payload: TokenPayload = crate::graph::serialize::deserialize_token(&read_data_payload(
        graph,
        ptr.block_idx,
        ptr.chunk_offset + 1, // skip DataHeader
        payload_len,
    )?)?;
    Ok(Some(payload))
}

/// Read a DataHeader from the data file at a given pointer location.
/// Used by Gremlin engine and rank decay to resolve entity identity from data pointers.
pub fn read_header_by_ptr(graph: &Graph, ptr: &MetaPointer) -> StorageResult<DataHeader> {
    let mut buf = [0u8; 64];
    // Fast path: read lock.
    {
        let cache = graph.block_cache.read().unwrap_or_else(|e| e.into_inner());
        if let Some(block) = cache.peek(ptr.block_idx) {
            let start = (ptr.chunk_offset as usize) * 64;
            buf.copy_from_slice(&block[start..start + 64]);
            return Ok(DataHeader::decode(&buf));
        }
    }
    // Slow path: write lock on cache miss.
    let mut cache = graph.block_cache.write().unwrap_or_else(|e| e.into_inner());
    let block = cache.get_or_load(ptr.block_idx, |idx| graph.data_file.read_block(idx), &|idx, data| {
        graph.data_file.write_block(idx, data).map_err(|e| e.into())
    })?;
    let start = (ptr.chunk_offset as usize) * 64;
    buf.copy_from_slice(&block[start..start + 64]);
    Ok(DataHeader::decode(&buf))
}

/// Update a DataHeader in-place in the data file (only rank/atime fields change).
///
/// This modifies the first 64-byte chunk of the record in the cached block
/// and marks the block dirty. No WAL entry is needed — the change is
/// persisted at the next checkpoint.
pub fn update_header_in_place(graph: &Graph, ptr: &MetaPointer, header: &DataHeader) -> StorageResult<()> {
    let mut cache = graph.block_cache.write().unwrap_or_else(|e| e.into_inner());
    let block = cache.get_or_load(ptr.block_idx, |idx| graph.data_file.read_block(idx), &|idx, data| {
        graph.data_file.write_block(idx, data).map_err(|e| e.into())
    })?;
    let start = (ptr.chunk_offset as usize) * 64;
    let mut buf = [0u8; 64];
    header.encode(&mut buf);
    block[start..start + 64].copy_from_slice(&buf);
    cache.mark_dirty(ptr.block_idx);
    Ok(())
}

/// Read raw data payload from data file chunks.
/// Callers pass `chunk_offset + 1` to skip the DataHeader when reading payload.
fn read_data_payload(
    graph: &Graph,
    block_idx: u32,
    chunk_offset: u8,
    data_len: usize,
) -> StorageResult<Vec<u8>> {
    let padded = BlockAllocator::padded_length(data_len);
    let mut buf = vec![0u8; padded];

    // Fast path: read lock — block may already be cached.
    {
        let cache = graph.block_cache.read().unwrap_or_else(|e| e.into_inner());
        if let Some(block) = cache.peek(block_idx) {
            let start = (chunk_offset as usize) * 64;
            let end = start + padded.min(BLOCK_SIZE - start);
            buf[..(end - start)].copy_from_slice(&block[start..end]);
            return Ok(buf[..data_len].to_vec());
        }
    }

    // Slow path: write lock — load from disk on cache miss.
    let mut cache = graph.block_cache.write().unwrap_or_else(|e| e.into_inner());
    let block = cache.get_or_load(block_idx, |idx| graph.data_file.read_block(idx), &|idx, data| {
        graph.data_file.write_block(idx, data).map_err(|e| e.into())
    })?;

    let start = (chunk_offset as usize) * 64;
    let end = start + padded.min(BLOCK_SIZE - start);
    buf[..(end - start)].copy_from_slice(&block[start..end]);
    Ok(buf[..data_len].to_vec())
}

// ── New DataHeader-based helpers ─────────────────────────────────────────────

/// Write a DataHeader + bincode payload to the data file as a single record.
/// Returns an MetaPointer MetaPointer pointing to the DataHeader chunk.
fn write_data_record(
    graph: &Graph,
    header: &DataHeader,
    payload_bytes: &[u8],
) -> StorageResult<MetaPointer> {
    let total_len = DATA_HEADER_SIZE + payload_bytes.len();
    let chunks_needed = BlockAllocator::chunks_needed(total_len);
    let padded_len = BlockAllocator::padded_length(total_len);
    let mut buf = vec![0u8; padded_len];

    // Write header into first 64 bytes.
    let mut header_buf = [0u8; 64];
    header.encode(&mut header_buf);
    buf[..64].copy_from_slice(&header_buf);

    // Write payload after header.
    buf[64..64 + payload_bytes.len()].copy_from_slice(payload_bytes);

    let (block_idx, chunk_offset) = allocate_chunks(graph, chunks_needed)?;
    write_data_chunks(graph, block_idx, chunk_offset, chunks_needed, &buf)?;

    Ok(MetaPointer::new(block_idx, chunk_offset))
}

/// Read a DataHeader from the data file at the given pointer.
fn read_data_header(graph: &Graph, ptr: MetaPointer) -> StorageResult<DataHeader> {
    let raw = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset, DATA_HEADER_SIZE as u16)?;
    let mut buf = [0u8; 64];
    buf.copy_from_slice(&raw);
    Ok(DataHeader::decode(&buf))
}
