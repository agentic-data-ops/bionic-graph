//! Vertex/Edge CRUD operations for the block-based graph engine.

use std::collections::HashMap;

use crate::graph::graph::Graph;
use crate::graph::serialize::{self, deserialize_edge, deserialize_vertex, serialize_edge, serialize_vertex};
use crate::graph::tokenizer::Tokenizer;
use crate::storage::block_allocator::BlockAllocator;
use crate::storage::index_file::{EdgeIndexRecord, TokenIndexRecord, VertexIndexRecord};
use crate::storage::memory_index::IndexPointer;
use crate::storage::redo_log::RedoLogEntry;
use crate::storage::types::{
    BlockHeader, DataStatus, EdgePayload, HistoryRecord, OpType, PropertyValue,
    StorageError, StorageResult, TokenPayload, TokenRef, VertexPayload, BLOCK_SIZE,
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
        id: vid,
        name: name.to_string(),
        labels: labels.to_vec(),
        keywords: keywords.to_vec(),
        properties: properties.clone(),
        history: Vec::new(),
    };

    let serialized = serialize_vertex(&payload)?;
    let data_len = serialized.len();
    let chunks_needed = BlockAllocator::chunks_needed(data_len);
    let padded = BlockAllocator::padded_length(data_len);
    let mut padded_data = serialized.clone();
    padded_data.resize(padded, 0);

    // ── Allocate data chunks ─────────────────────────────────────────
    let (data_block, data_chunk_offset) = allocate_chunks(graph, chunks_needed)?;
    write_data_chunks(graph, data_block, data_chunk_offset, chunks_needed, &padded_data)?;

    // ── Create index record ──────────────────────────────────────────
    let idx_rec = VertexIndexRecord::new(vid, data_block, data_chunk_offset, data_len as u16);
    let (idx_block, idx_chunk) = {
        let mut buf = [0u8; 64];
        idx_rec.encode(&mut buf);
        graph.index_file.alloc_record(&buf)?
    };
    let idx_ptr = IndexPointer::new(idx_block, idx_chunk);

    // ── Update memory index ──────────────────────────────────────────
    {
        let mut mi = graph.memory_index.write().unwrap();
        mi.vertices.insert(vid, idx_ptr);
        mi.ranks.insert(idx_rec.rank, idx_ptr);
    }

    // ── Tokenize attributes ──────────────────────────────────────────
    tokenize_vertex(&graph, vid, &payload, chunks_needed as u8)?;

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
        id: eid,
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
    let data_len = serialized.len();
    let chunks_needed = BlockAllocator::chunks_needed(data_len);
    let padded = BlockAllocator::padded_length(data_len);
    let mut padded_data = serialized.clone();
    padded_data.resize(padded, 0);

    // ── Allocate data chunks ─────────────────────────────────────────
    let (data_block, data_chunk_offset) = allocate_chunks(graph, chunks_needed)?;
    write_data_chunks(graph, data_block, data_chunk_offset, chunks_needed, &padded_data)?;

    // ── Create index record ──────────────────────────────────────────
    let idx_rec = EdgeIndexRecord::new(eid, source, target, data_block, data_chunk_offset, data_len as u16);
    let (idx_block, idx_chunk) = {
        let mut buf = [0u8; 64];
        idx_rec.encode(&mut buf);
        graph.index_file.alloc_record(&buf)?
    };
    let idx_ptr = IndexPointer::new(idx_block, idx_chunk);

    // ── Update memory index ──────────────────────────────────────────
    {
        let mut mi = graph.memory_index.write().unwrap();
        mi.edges.insert(eid, idx_ptr);
        mi.ranks.insert(idx_rec.rank, idx_ptr);
        mi.adjacency.add_edge(eid, source, target, idx_ptr);
    }

    // ── Tokenize ─────────────────────────────────────────────────────
    tokenize_edge(&graph, eid, &payload, chunks_needed as u8)?;

    // ── WAL ──────────────────────────────────────────────────────────
    graph.redo_log.append(OpType::EdgeCreate, eid as u64, &serialized)?;

    Ok(eid)
}

// ── Read ────────────────────────────────────────────────────────────────────

/// Get a vertex by ID. Returns `None` if not found or soft-deleted.
pub fn get_vertex(graph: &Graph, vid: u32) -> StorageResult<Option<VertexPayload>> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap();
        mi.vertices.get(vid).copied()
    };
    let Some(ptr) = ptr else { return Ok(None) };

    let rec = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset)?;
    if rec.status == DataStatus::Deleted {
        return Ok(None);
    }

    let data = read_data_chunks(graph, rec.data_block_idx, rec.data_chunk_offset, rec.data_len)?;
    let payload = deserialize_vertex(&data)?;

    // Update atime and rank.
    update_rank_and_atime(graph, &ptr, &rec)?;

    Ok(Some(payload))
}

/// Get an edge by ID. Returns `None` if not found or soft-deleted.
pub fn get_edge(graph: &Graph, eid: u32) -> StorageResult<Option<EdgePayload>> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap();
        mi.edges.get(eid).copied()
    };
    let Some(ptr) = ptr else { return Ok(None) };

    let rec = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset)?;
    if rec.status == DataStatus::Deleted {
        return Ok(None);
    }

    let data = read_data_chunks(graph, rec.data_block_idx, rec.data_chunk_offset, rec.data_len)?;
    let payload = deserialize_edge(&data)?;

    // Update atime and rank.
    update_rank_and_atime(graph, &ptr, &rec)?;

    Ok(Some(payload))
}

/// Read a vertex's index record (rank + atime) without updating anything.
/// Safe for introspection — does NOT call `update_rank_and_atime`.
pub fn get_vertex_index_record(graph: &Graph, vid: u32) -> StorageResult<Option<VertexIndexRecord>> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap();
        mi.vertices.get(vid).copied()
    };
    let Some(ptr) = ptr else { return Ok(None) };
    let rec = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset)?;
    if rec.status == DataStatus::Deleted {
        return Ok(None);
    }
    Ok(Some(rec))
}

/// Read an edge's index record (rank + atime) without updating anything.
/// Safe for introspection — does NOT call `update_rank_and_atime`.
pub fn get_edge_index_record(graph: &Graph, eid: u32) -> StorageResult<Option<EdgeIndexRecord>> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap();
        mi.edges.get(eid).copied()
    };
    let Some(ptr) = ptr else { return Ok(None) };
    let rec = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset)?;
    if rec.status == DataStatus::Deleted {
        return Ok(None);
    }
    Ok(Some(rec))
}

/// Update a vertex's metadata (rank and/or atime). Creates an IndexUpdate
/// redo log entry. If a field is `None`, its current value is preserved.
pub fn update_vertex_meta(graph: &Graph, vid: u32, new_rank: Option<u32>, new_atime: Option<u64>) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap();
        mi.vertices.get(vid).copied()
    }.ok_or_else(|| StorageError::Other(format!("vertex {} not found", vid)))?;

    let mut rec = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset)?;
    let old_rank = rec.rank;
    let old_atime = rec.atime;

    if let Some(r) = new_rank {
        rec.rank = r;
    }
    if let Some(a) = new_atime {
        rec.atime = a;
    }

    if rec.rank == old_rank && rec.atime == old_atime {
        return Ok(()); // nothing changed
    }

    graph.index_file.update_vertex_record(ptr.block_idx, ptr.chunk_offset, &rec)?;

    let mut mi = graph.memory_index.write().unwrap();
    if old_rank != rec.rank {
        mi.ranks.remove(old_rank, &ptr);
        mi.ranks.insert(rec.rank, ptr);
    }
    if old_atime != rec.atime {
        mi.atime_index.remove(old_atime, &ptr);
        mi.atime_index.insert(rec.atime, ptr);
    }

    // Write IndexUpdate redo log entry (always full rank+atime).
    let mut data = Vec::with_capacity(12);
    data.extend_from_slice(&rec.rank.to_le_bytes());
    data.extend_from_slice(&rec.atime.to_le_bytes());
    graph.redo_log.append(OpType::VertexIndexUpdate, vid as u64, &data)?;

    Ok(())
}

/// Update an edge's metadata (rank and/or atime). Creates an IndexUpdate
/// redo log entry. If a field is `None`, its current value is preserved.
pub fn update_edge_meta(graph: &Graph, eid: u32, new_rank: Option<u32>, new_atime: Option<u64>) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap();
        mi.edges.get(eid).copied()
    }.ok_or_else(|| StorageError::Other(format!("edge {} not found", eid)))?;

    let mut rec = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset)?;
    let old_rank = rec.rank;
    let old_atime = rec.atime;

    if let Some(r) = new_rank {
        rec.rank = r;
    }
    if let Some(a) = new_atime {
        rec.atime = a;
    }

    if rec.rank == old_rank && rec.atime == old_atime {
        return Ok(());
    }

    graph.index_file.update_edge_record(ptr.block_idx, ptr.chunk_offset, &rec)?;

    let mut mi = graph.memory_index.write().unwrap();
    if old_rank != rec.rank {
        mi.ranks.remove(old_rank, &ptr);
        mi.ranks.insert(rec.rank, ptr);
    }
    if old_atime != rec.atime {
        mi.atime_index.remove(old_atime, &ptr);
        mi.atime_index.insert(rec.atime, ptr);
    }

    let mut data = Vec::with_capacity(12);
    data.extend_from_slice(&rec.rank.to_le_bytes());
    data.extend_from_slice(&rec.atime.to_le_bytes());
    graph.redo_log.append(OpType::EdgeIndexUpdate, eid as u64, &data)?;

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
    let (old_payload, old_ptr, old_rec) = {
        let mi = graph.memory_index.read().unwrap();
        let ptr = mi.vertices.get(vid).copied()
            .ok_or_else(|| StorageError::Other(format!("vertex {} not found", vid)))?;
        let rec = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset)?;
        let data = read_data_chunks(graph, rec.data_block_idx, rec.data_chunk_offset, rec.data_len)?;
        let payload = deserialize_vertex(&data)?;
        (payload, ptr, rec)
    };

    // Build new payload.
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
    if record_history {
        let old_bytes = serialize_vertex(&old_payload)?;
        new_payload.history.push(HistoryRecord {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
            data: old_bytes,
        });
    }

    // Serialize and allocate new chunks (copy-on-write).
    let serialized = serialize_vertex(&new_payload)?;
    let chunks_needed = BlockAllocator::chunks_needed(serialized.len());
    let padded = BlockAllocator::padded_length(serialized.len());
    let mut padded_data = serialized.clone();
    padded_data.resize(padded, 0);

    let (new_data_block, new_data_chunk) = allocate_chunks(graph, chunks_needed)?;
    write_data_chunks(graph, new_data_block, new_data_chunk, chunks_needed, &padded_data)?;

    // Update index record.
    let mut new_rec = old_rec.clone();
    new_rec.data_block_idx = new_data_block;
    new_rec.data_chunk_offset = new_data_chunk;
    new_rec.data_len = serialized.len() as u16;
    new_rec.version += 1;
    new_rec.mtime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    new_rec.atime = new_rec.mtime;
    new_rec.rank += 1;

    graph.index_file.update_vertex_record(old_ptr.block_idx, old_ptr.chunk_offset, &new_rec)?;

    // Update rank in memory index.
    {
        let mut mi = graph.memory_index.write().unwrap();
        mi.ranks.remove(old_rec.rank, &old_ptr);
        mi.ranks.insert(new_rec.rank, old_ptr);
        mi.atime_index.remove(old_rec.atime, &old_ptr);
        mi.atime_index.insert(new_rec.atime, old_ptr);
    }

    // Free old data chunks.
    free_data_chunks(graph, old_rec.data_block_idx, old_rec.data_chunk_offset,
        BlockAllocator::chunks_needed(old_rec.data_len as usize))?;

    // Re-tokenize if relevant fields changed.
    tokenize_vertex(graph, vid, &new_payload, chunks_needed as u8)?;

    // WAL.
    graph.redo_log.append(OpType::VertexUpdate, vid as u64, &padded_data)?;

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
    let (old_payload, old_ptr, old_rec) = {
        let mi = graph.memory_index.read().unwrap();
        let ptr = mi.edges.get(eid).copied()
            .ok_or_else(|| StorageError::Other(format!("edge {} not found", eid)))?;
        let rec = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset)?;
        let data = read_data_chunks(graph, rec.data_block_idx, rec.data_chunk_offset, rec.data_len)?;
        let payload = deserialize_edge(&data)?;
        (payload, ptr, rec)
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
            timestamp: timestamp_us(),
            data: old_bytes,
        });
    }

    let serialized = serialize_edge(&new_payload)?;
    let chunks_needed = BlockAllocator::chunks_needed(serialized.len());
    let padded = BlockAllocator::padded_length(serialized.len());
    let mut padded_data = serialized.clone();
    padded_data.resize(padded, 0);

    let (new_data_block, new_data_chunk) = allocate_chunks(graph, chunks_needed)?;
    write_data_chunks(graph, new_data_block, new_data_chunk, chunks_needed, &padded_data)?;

    let mut new_rec = old_rec.clone();
    new_rec.data_block_idx = new_data_block;
    new_rec.data_chunk_offset = new_data_chunk;
    new_rec.data_len = serialized.len() as u16;
    new_rec.version += 1;
    new_rec.mtime = timestamp_us();
    new_rec.atime = new_rec.mtime;
    new_rec.rank += 1;

    graph.index_file.update_edge_record(old_ptr.block_idx, old_ptr.chunk_offset, &new_rec)?;

    {
        let mut mi = graph.memory_index.write().unwrap();
        mi.ranks.remove(old_rec.rank, &old_ptr);
        mi.ranks.insert(new_rec.rank, old_ptr);
        mi.atime_index.remove(old_rec.atime, &old_ptr);
        mi.atime_index.insert(new_rec.atime, old_ptr);
    }

    free_data_chunks(graph, old_rec.data_block_idx, old_rec.data_chunk_offset,
        BlockAllocator::chunks_needed(old_rec.data_len as usize))?;

    tokenize_edge(graph, eid, &new_payload, chunks_needed as u8)?;
    graph.redo_log.append(OpType::EdgeUpdate, eid as u64, &serialized)?;

    Ok(())
}

// ── Delete ──────────────────────────────────────────────────────────────────

/// Soft-delete a vertex: mark as deleted in index, but keep data for time-travel.
pub fn soft_delete_vertex(graph: &Graph, vid: u32) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap();
        mi.vertices.get(vid).copied()
            .ok_or_else(|| StorageError::Other(format!("vertex {} not found", vid)))?
    };

    let mut rec = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset)?;
    rec.mark_deleted();
    rec.mtime = timestamp_us();
    graph.index_file.update_vertex_record(ptr.block_idx, ptr.chunk_offset, &rec)?;

    // Keep vertex in memory index (for time-travel queries).
    // DataStatus::Deleted flag + read_vertex_by_record handle visibility.
    {
        let mut mi = graph.memory_index.write().unwrap();
        mi.ranks.remove(rec.rank, &ptr);
    }

    graph.redo_log.append(OpType::VertexDelete, vid as u64, &[])?;
    Ok(())
}

/// Hard-delete a vertex: remove data and index entirely.
pub fn hard_delete_vertex(graph: &Graph, vid: u32) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap();
        mi.vertices.get(vid).copied()
            .ok_or_else(|| StorageError::Other(format!("vertex {} not found", vid)))?
    };

    let rec = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset)?;

    // Free data chunks.
    free_data_chunks(graph, rec.data_block_idx, rec.data_chunk_offset,
        BlockAllocator::chunks_needed(rec.data_len as usize))?;

    // Clear index record.
    graph.index_file.delete_record(ptr.block_idx, ptr.chunk_offset)?;

    // Remove from memory index.
    {
        let mut mi = graph.memory_index.write().unwrap();
        mi.vertices.remove(vid);
        mi.ranks.remove(rec.rank, &ptr);
    }

    graph.redo_log.append(OpType::VertexDelete, vid as u64, &[])?;
    Ok(())
}

/// Soft-delete an edge.
pub fn soft_delete_edge(graph: &Graph, eid: u32) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap();
        mi.edges.get(eid).copied()
            .ok_or_else(|| StorageError::Other(format!("edge {} not found", eid)))?
    };

    let rec = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset)?;
    let mut rec2 = rec.clone();
    rec2.mark_deleted();
    rec2.mtime = timestamp_us();
    graph.index_file.update_edge_record(ptr.block_idx, ptr.chunk_offset, &rec2)?;

    {
        let mut mi = graph.memory_index.write().unwrap();
        mi.ranks.remove(rec.rank, &ptr);
        mi.adjacency.remove_edge(rec.source, rec.target, &ptr);
    }

    graph.redo_log.append(OpType::EdgeDelete, eid as u64, &[])?;
    Ok(())
}

/// Hard-delete an edge.
pub fn hard_delete_edge(graph: &Graph, eid: u32) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap();
        mi.edges.get(eid).copied()
            .ok_or_else(|| StorageError::Other(format!("edge {} not found", eid)))?
    };

    let rec = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset)?;

    free_data_chunks(graph, rec.data_block_idx, rec.data_chunk_offset,
        BlockAllocator::chunks_needed(rec.data_len as usize))?;

    graph.index_file.delete_record(ptr.block_idx, ptr.chunk_offset)?;

    {
        let mut mi = graph.memory_index.write().unwrap();
        mi.edges.remove(eid);
        mi.ranks.remove(rec.rank, &ptr);
        mi.adjacency.remove_edge(rec.source, rec.target, &ptr);
    }

    graph.redo_log.append(OpType::EdgeDelete, eid as u64, &[])?;
    Ok(())
}

// ── WAL replay ──────────────────────────────────────────────────────────────

/// Replay a single WAL entry during graph startup recovery.
pub fn replay_entry(graph: &Graph, entry: &RedoLogEntry) -> StorageResult<()> {
    match entry.op_type {
        OpType::VertexCreate => {
            if let Ok(payload) = deserialize_vertex(&entry.data) {
                if payload.id >= graph.next_vertex_id.load(std::sync::atomic::Ordering::Relaxed) {
                    graph.next_vertex_id.store(payload.id + 1, std::sync::atomic::Ordering::Relaxed);
                }
                // Always re-apply: data in dirty cache may have been lost.
                replay_create_vertex(graph, &payload, &entry.data)?;
            }
        }
        OpType::VertexUpdate => {
            if let Ok(payload) = deserialize_vertex(&entry.data) {
                if payload.id >= graph.next_vertex_id.load(std::sync::atomic::Ordering::Relaxed) {
                    graph.next_vertex_id.store(payload.id + 1, std::sync::atomic::Ordering::Relaxed);
                }
                replay_create_vertex(graph, &payload, &entry.data)?;
            }
        }
        OpType::EdgeCreate => {
            if let Ok(payload) = deserialize_edge(&entry.data) {
                if payload.id >= graph.next_edge_id.load(std::sync::atomic::Ordering::Relaxed) {
                    graph.next_edge_id.store(payload.id + 1, std::sync::atomic::Ordering::Relaxed);
                }
                replay_create_edge(graph, &payload, &entry.data)?;
            }
        }
        OpType::EdgeUpdate => {
            if let Ok(payload) = deserialize_edge(&entry.data) {
                if payload.id >= graph.next_edge_id.load(std::sync::atomic::Ordering::Relaxed) {
                    graph.next_edge_id.store(payload.id + 1, std::sync::atomic::Ordering::Relaxed);
                }
                replay_create_edge(graph, &payload, &entry.data)?;
            }
        }
        OpType::VertexDelete => {
            if let Some(&ptr) = graph.memory_index.read().unwrap().vertices.get(entry.op_id as u32) {
                let _ = graph.index_file.delete_record(ptr.block_idx, ptr.chunk_offset);
                let mut mi = graph.memory_index.write().unwrap();
                mi.vertices.remove(entry.op_id as u32);
            }
        }
        OpType::EdgeDelete => {
            if let Some(&ptr) = graph.memory_index.read().unwrap().edges.get(entry.op_id as u32) {
                // Read the edge record to obtain source/target before deletion.
                let (source, target) = if let Ok(rec) = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset) {
                    (rec.source, rec.target)
                } else {
                    (0, 0)
                };
                let _ = graph.index_file.delete_record(ptr.block_idx, ptr.chunk_offset);
                let mut mi = graph.memory_index.write().unwrap();
                mi.edges.remove(entry.op_id as u32);
                // Use the real source/target vertex IDs, NOT edge_id, to properly
                // clean up the adjacency index.
                mi.adjacency.remove_edge(source, target, &ptr);
            }
        }
        OpType::VertexIndexUpdate => {
            // data = [rank: u32 LE (4)] [atime: u64 LE (8)] — 12 bytes
            if entry.data.len() >= 12 {
                let new_rank = u32::from_le_bytes(entry.data[0..4].try_into().unwrap());
                let new_atime = u64::from_le_bytes(entry.data[4..12].try_into().unwrap());
            // Drop read guard before write guard to avoid RwLock deadlock.
                let found = graph.memory_index.read().unwrap().vertices.get(entry.op_id as u32).copied();
                if let Some(ptr) = found {
                    if let Ok(mut rec) = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset) {
                        let old_rank = rec.rank;
                        rec.rank = new_rank;
                        rec.atime = new_atime;
                        if let Ok(()) = graph.index_file.update_vertex_record(ptr.block_idx, ptr.chunk_offset, &rec) {
                            let mut mi = graph.memory_index.write().unwrap();
                            mi.ranks.remove(old_rank, &ptr);
                            mi.ranks.insert(new_rank, ptr);
                            mi.atime_index.remove(rec.atime, &ptr);
                            mi.atime_index.insert(new_atime, ptr);
                        }
                    }
                }
            }
        }
        OpType::EdgeIndexUpdate => {
            // data = [rank: u32 LE (4)] [atime: u64 LE (8)] — 12 bytes
            if entry.data.len() >= 12 {
                let new_rank = u32::from_le_bytes(entry.data[0..4].try_into().unwrap());
                let new_atime = u64::from_le_bytes(entry.data[4..12].try_into().unwrap());
                // Drop read guard before write guard to avoid RwLock deadlock.
                let found = graph.memory_index.read().unwrap().edges.get(entry.op_id as u32).copied();
                if let Some(ptr) = found {
                    if let Ok(mut rec) = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset) {
                        let old_rank = rec.rank;
                        rec.rank = new_rank;
                        rec.atime = new_atime;
                        if let Ok(()) = graph.index_file.update_edge_record(ptr.block_idx, ptr.chunk_offset, &rec) {
                            let mut mi = graph.memory_index.write().unwrap();
                            mi.ranks.remove(old_rank, &ptr);
                            mi.ranks.insert(new_rank, ptr);
                            mi.atime_index.remove(rec.atime, &ptr);
                            mi.atime_index.insert(new_atime, ptr);
                        }
                    }
                }
            }
        }
        OpType::TokenCreate | OpType::TokenUpdate | OpType::TokenDelete
        | OpType::TokenIndexUpdate => {}
    }
    Ok(())
}

// ── Replay helpers ───────────────────────────────────────────────────────────

/// Replay helper: recreate a vertex from WAL data during startup recovery.
fn replay_create_vertex(graph: &Graph, payload: &VertexPayload, wal_data: &[u8]) -> StorageResult<()> {
    // Skip if this vertex was already re-created from the index file during
    // build_memory_index. This prevents duplicates after unclean shutdown.
    {
        let mi = graph.memory_index.read().unwrap();
        if mi.vertices.contains(payload.id) {
            return Ok(());
        }
    }

    let data_len = wal_data.len();
    let chunks_needed = BlockAllocator::chunks_needed(data_len);
    let padded = BlockAllocator::padded_length(data_len);
    let mut padded_data = wal_data.to_vec();
    padded_data.resize(padded, 0);

    let (data_block, data_chunk_offset) = allocate_chunks(graph, chunks_needed)?;
    write_data_chunks(graph, data_block, data_chunk_offset, chunks_needed, &padded_data)?;

    let idx_rec = VertexIndexRecord::new(payload.id, data_block, data_chunk_offset, data_len as u16);
    let (idx_block, idx_chunk) = {
        let mut buf = [0u8; 64];
        idx_rec.encode(&mut buf);
        graph.index_file.alloc_record(&buf)?
    };
    let idx_ptr = IndexPointer::new(idx_block, idx_chunk);

    let mut mi = graph.memory_index.write().unwrap();
    mi.vertices.insert(payload.id, idx_ptr);
    mi.ranks.insert(idx_rec.rank, idx_ptr);
    drop(mi);

    tokenize_vertex(graph, payload.id, payload, chunks_needed as u8)?;
    Ok(())
}

/// Replay helper: recreate an edge from WAL data during startup recovery.
fn replay_create_edge(graph: &Graph, payload: &EdgePayload, wal_data: &[u8]) -> StorageResult<()> {
    // Skip if this edge was already re-created from the index file during
    // build_memory_index. This prevents duplicate adjacency entries when
    // the WAL contains entries that were checkpointed before an unclean
    // shutdown.
    {
        let mi = graph.memory_index.read().unwrap();
        if mi.edges.contains(payload.id) {
            return Ok(());
        }
    }

    let data_len = wal_data.len();
    let chunks_needed = BlockAllocator::chunks_needed(data_len);
    let padded = BlockAllocator::padded_length(data_len);
    let mut padded_data = wal_data.to_vec();
    padded_data.resize(padded, 0);

    let (data_block, data_chunk_offset) = allocate_chunks(graph, chunks_needed)?;
    write_data_chunks(graph, data_block, data_chunk_offset, chunks_needed, &padded_data)?;

    let idx_rec = EdgeIndexRecord::new(
        payload.id, payload.source, payload.target,
        data_block, data_chunk_offset, data_len as u16,
    );
    let (idx_block, idx_chunk) = {
        let mut buf = [0u8; 64];
        idx_rec.encode(&mut buf);
        graph.index_file.alloc_record(&buf)?
    };
    let idx_ptr = IndexPointer::new(idx_block, idx_chunk);

    let mut mi = graph.memory_index.write().unwrap();
    mi.edges.insert(payload.id, idx_ptr);
    mi.ranks.insert(idx_rec.rank, idx_ptr);
    mi.adjacency.add_edge(payload.id, payload.source, payload.target, idx_ptr);
    drop(mi);

    tokenize_edge(graph, payload.id, payload, chunks_needed as u8)?;
    Ok(())
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Allocate chunks for a new piece of data. Returns (block_idx, chunk_offset).
fn allocate_chunks(graph: &Graph, chunks_needed: u8) -> StorageResult<(u32, u8)> {
    let mut bf = graph.bitmap_file.write().unwrap();

    let block_idx = bf.alloc_block(|count| {
        graph.data_file.allocate_blocks(count)
    })?;

    let (chunk_off, was_full) = {
        let mut cache = graph.block_cache.write().unwrap();
        cache.with_block(block_idx,
            |idx| graph.data_file.read_block(idx),
            &|idx, data| graph.data_file.write_block(idx, data),
            |block| {
                let mut header = BlockHeader::decode(block);
                let off = BlockAllocator::alloc_chunks(&mut header.bitmap, chunks_needed)
                    .expect("block should have free chunks");
                header.encode(block);
                let full = BlockAllocator::is_block_full(&header.bitmap);
                (off, full)
            },
        )?
    };

    if was_full {
        bf.mark_full(block_idx)?;
    }

    Ok((block_idx, chunk_off))
}

/// Write padded data into the allocated chunks.
fn write_data_chunks(graph: &Graph, block_idx: u32, chunk_offset: u8, chunks: u8, data: &[u8]) -> StorageResult<()> {
    // Write data into the block through cache, then flush to disk.
    let block_copy = {
        let mut cache = graph.block_cache.write().unwrap();
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
    let chunks = BlockAllocator::chunks_needed(data_len as usize);
    let mut cache = graph.block_cache.write().unwrap();
    let block = cache.get_or_load(block_idx, |idx| {
        graph.data_file.read_block(idx)
    }, &|idx, data| {
        graph.data_file.write_block(idx, data)
    })?;

    let start = (chunk_offset as usize) * 64;
    let end = start + (chunks as usize) * 64;
    let mut data = vec![0u8; data_len as usize];
    let read_len = data_len as usize;
    data.copy_from_slice(&block[start..start + read_len]);
    Ok(data)
}

/// Free previously allocated data chunks.
fn free_data_chunks(graph: &Graph, block_idx: u32, chunk_offset: u8, chunks: u8) -> StorageResult<()> {
    let mut cache = graph.block_cache.write().unwrap();
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

        if was_full && !BlockAllocator::is_block_full(&header.bitmap) {
            let mut bf = graph.bitmap_file.write().unwrap();
            bf.mark_free(block_idx)?;
        }
    }
    Ok(())
}

/// Extract tokens from vertex attributes and index them.
fn tokenize_vertex(graph: &Graph, vid: u32, payload: &VertexPayload, chunks: u8) -> StorageResult<()> {
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
fn tokenize_edge(graph: &Graph, eid: u32, payload: &EdgePayload, _chunks: u8) -> StorageResult<()> {
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
        let mi = graph.memory_index.read().unwrap();
        mi.tokens.get(token_str).map(|v| v.clone())
    };

    if let Some(ptrs) = existing {
        // Update the existing token's TokenPayload in the data file.
        // For now, we append a new ref to the token payload.
        if let Some(ptr) = ptrs.first() {
            let token_rec = graph.index_file.read_token_record(ptr.block_idx, ptr.chunk_offset)?;
            let existing_data = read_data_chunks(graph, token_rec.data_block_idx, token_rec.data_chunk_offset, token_rec.data_len)?;
            if let Ok(mut token_payload) = crate::graph::serialize::deserialize_token(&existing_data) {
                token_payload.refs.push(TokenRef {
                    ref_type,
                    ref_id,
                    ref_version: 1,
                    ref_frequency: hits.len() as u16,
                    hits: hits.to_vec(),
                    timestamp: timestamp_us(),
                });
                let new_data = crate::graph::serialize::serialize_token(&token_payload)?;
                let chunks_needed = BlockAllocator::chunks_needed(new_data.len());
                let padded = BlockAllocator::padded_length(new_data.len());
                let mut padded_data = new_data.clone();
                padded_data.resize(padded, 0);

                // Allocate new space and update index.
                let (new_dblock, new_dchunk) = allocate_chunks(graph, chunks_needed)?;
                write_data_chunks(graph, new_dblock, new_dchunk, chunks_needed, &padded_data)?;

                let mut new_rec = token_rec.clone();
                new_rec.data_block_idx = new_dblock;
                new_rec.data_chunk_offset = new_dchunk;
                new_rec.data_len = padded_data.len() as u16;
                graph.index_file.update_token_record(ptr.block_idx, ptr.chunk_offset, &new_rec)?;
                graph.redo_log.append(OpType::TokenUpdate, token_payload.id as u64, &new_data)?;

                free_data_chunks(graph, token_rec.data_block_idx, token_rec.data_chunk_offset,
                    BlockAllocator::chunks_needed(token_rec.data_len as usize))?;
            }
        }
    } else {
        // Create new token.
        let token_payload = TokenPayload {
            id: graph.alloc_token_id(),
            refs: vec![TokenRef {
                ref_type,
                ref_id,
                ref_version: 1,
                ref_frequency: hits.len() as u16,
                hits: hits.to_vec(),
                timestamp: timestamp_us(),
            }],
        };
        let data = serialize::serialize_token(&token_payload)?;
        let chunks_needed = BlockAllocator::chunks_needed(data.len());
        let padded = BlockAllocator::padded_length(data.len());
        let mut padded_data = data.clone();
        padded_data.resize(padded, 0);

        let (dblock, dchunk) = allocate_chunks(graph, chunks_needed)?;
        write_data_chunks(graph, dblock, dchunk, chunks_needed, &padded_data)?;

        let token_rec = TokenIndexRecord::new(token_payload.id, token_str, dblock, dchunk, padded_data.len() as u16);
        let (tblock, tchunk) = {
            let mut buf = [0u8; 64];
            token_rec.encode(&mut buf);
            graph.index_file.alloc_record(&buf)?
        };
        let tptr = IndexPointer::new(tblock, tchunk);

        // Update memory index.
        let mut mi = graph.memory_index.write().unwrap();
        mi.tokens.insert(token_str.to_string(), tptr);

        graph.redo_log.append(OpType::TokenCreate, token_payload.id as u64, &data)?;
    }

    Ok(())
}

/// Update access time and increment rank for an index record.
fn update_rank_and_atime(graph: &Graph, ptr: &IndexPointer, rec: &impl HasRankAndTime) -> StorageResult<()> {
    let now = timestamp_us();
    let new_rank = rec.rank().wrapping_add(1);

    match rec.chunk_type() {
        crate::storage::types::ChunkType::Vertex => {
            let mut new_rec = graph.index_file.read_vertex_record(ptr.block_idx, ptr.chunk_offset)?;
            new_rec.atime = now;
            new_rec.rank = new_rank;
            graph.index_file.update_vertex_record(ptr.block_idx, ptr.chunk_offset, &new_rec)?;

            let mut mi = graph.memory_index.write().unwrap();
            mi.ranks.remove(rec.rank(), ptr);
            mi.ranks.insert(new_rank, *ptr);
            mi.atime_index.remove(rec.atime(), ptr);
            mi.atime_index.insert(now, *ptr);
        }
        crate::storage::types::ChunkType::Edge => {
            let mut new_rec = graph.index_file.read_edge_record(ptr.block_idx, ptr.chunk_offset)?;
            new_rec.atime = now;
            new_rec.rank = new_rank;
            graph.index_file.update_edge_record(ptr.block_idx, ptr.chunk_offset, &new_rec)?;

            let mut mi = graph.memory_index.write().unwrap();
            mi.ranks.remove(rec.rank(), ptr);
            mi.ranks.insert(new_rank, *ptr);
            mi.atime_index.remove(rec.atime(), ptr);
            mi.atime_index.insert(now, *ptr);
        }
        _ => {}
    }
    Ok(())
}

/// Trait to extract common fields from VertexIndexRecord and EdgeIndexRecord.
trait HasRankAndTime {
    fn rank(&self) -> u32;
    fn atime(&self) -> u64;
    fn chunk_type(&self) -> crate::storage::types::ChunkType;
}

impl HasRankAndTime for VertexIndexRecord {
    fn rank(&self) -> u32 { self.rank }
    fn atime(&self) -> u64 { self.atime }
    fn chunk_type(&self) -> crate::storage::types::ChunkType { self.chunk_type }
}

impl HasRankAndTime for EdgeIndexRecord {
    fn rank(&self) -> u32 { self.rank }
    fn atime(&self) -> u64 { self.atime }
    fn chunk_type(&self) -> crate::storage::types::ChunkType { self.chunk_type }
}

pub(crate) fn timestamp_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

// ── Read-by-record helpers (for Gremlin engine) ──────────────────────────────

/// Read a vertex payload given its index record pointer.
pub fn read_vertex_by_record(
    graph: &Graph,
    rec: &VertexIndexRecord,
    at: Option<u64>,
) -> StorageResult<Option<VertexPayload>> {
    // Time-travel: check existence/reachability at the query time.
    if let Some(timestamp) = at {
        if timestamp < rec.ctime {
            return Ok(None); // didn't exist yet
        }
        let mut payload: VertexPayload = deserialize_vertex(&read_data_payload(
            graph,
            rec.data_block_idx,
            rec.data_chunk_offset,
            rec.data_len as usize,
        )?)?;

        // Walk history: newest entry h where `at < h.timestamp` means h's state was current.
        for h in payload.history.iter().rev() {
            if timestamp < h.timestamp {
                return Ok(Some(deserialize_vertex(&h.data)?));
            }
        }
        // Nothing in history covers this — check deletion time.
        if rec.status == DataStatus::Deleted && timestamp >= rec.mtime {
            return Ok(None); // deleted by this time
        }
        return Ok(Some(payload));
    }

    // Normal (non-time-travel) path: deleted vertices are hidden.
    if rec.status == DataStatus::Deleted {
        return Ok(None);
    }
    let mut payload: VertexPayload = deserialize_vertex(&read_data_payload(
        graph,
        rec.data_block_idx,
        rec.data_chunk_offset,
        rec.data_len as usize,
    )?)?;
    Ok(Some(payload))
}

/// Read an edge payload given its index record pointer.
pub fn read_edge_by_record(
    graph: &Graph,
    rec: &EdgeIndexRecord,
    at: Option<u64>,
) -> StorageResult<Option<EdgePayload>> {
    if let Some(timestamp) = at {
        if timestamp < rec.ctime {
            return Ok(None);
        }
        let mut payload: EdgePayload = deserialize_edge(&read_data_payload(
            graph,
            rec.data_block_idx,
            rec.data_chunk_offset,
            rec.data_len as usize,
        )?)?;

        for h in payload.history.iter().rev() {
            if timestamp < h.timestamp {
                return Ok(Some(deserialize_edge(&h.data)?));
            }
        }
        if rec.status == DataStatus::Deleted && timestamp >= rec.mtime {
            return Ok(None);
        }
        return Ok(Some(payload));
    }

    if rec.status == DataStatus::Deleted {
        return Ok(None);
    }
    let payload: EdgePayload = deserialize_edge(&read_data_payload(
        graph,
        rec.data_block_idx,
        rec.data_chunk_offset,
        rec.data_len as usize,
    )?)?;
    Ok(Some(payload))
}

/// Read a token payload given its index record pointer.
pub fn read_token_by_record(
    graph: &Graph,
    rec: &TokenIndexRecord,
) -> StorageResult<Option<TokenPayload>> {
    if rec.status == DataStatus::Deleted {
        return Ok(None);
    }
    let payload: TokenPayload = crate::graph::serialize::deserialize_token(&read_data_payload(
        graph,
        rec.data_block_idx,
        rec.data_chunk_offset,
        rec.data_len as usize,
    )?)?;
    Ok(Some(payload))
}

/// Read raw data payload from data file chunks.
fn read_data_payload(
    graph: &Graph,
    block_idx: u32,
    chunk_offset: u8,
    data_len: usize,
) -> StorageResult<Vec<u8>> {
    let padded = BlockAllocator::padded_length(data_len);
    let mut buf = vec![0u8; padded];

    let mut cache = graph.block_cache.write().unwrap();
    let block = cache.get_or_load(block_idx, |idx| graph.data_file.read_block(idx), &|idx, data| {
        graph.data_file.write_block(idx, data).map_err(|e| e.into())
    })?;

    let start = (chunk_offset as usize) * 64;
    let end = start + padded.min(BLOCK_SIZE - start);
    buf[..(end - start)].copy_from_slice(&block[start..end]);
    Ok(buf[..data_len].to_vec())
}
