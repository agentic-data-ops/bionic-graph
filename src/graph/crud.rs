//! Vertex/Edge CRUD operations for the block-based graph engine.

use std::collections::HashMap;

use crate::graph::graph::Graph;
use crate::graph::profile;
use crate::graph::serialize::{self, deserialize_edge, deserialize_vertex, serialize_edge, serialize_vertex};
use crate::graph::tokenizer::Tokenizer;
use crate::storage::block_allocator::BlockAllocator;
use crate::storage::memory_index::MetaPointer;
use crate::storage::redo_log::RedoLogEntry;
use crate::storage::types::{
    BlockHeader, CHUNK_SIZE, CHUNKS_PER_BLOCK, DataHeader, DataStatus, EdgePayload, HistoryRecord,
    OpType, PropertyValue, StorageError, StorageResult, TokenPayload, TokenRef, VertexPayload,
    BLOCK_SIZE, DATA_HEADER_SIZE, timestamp_us,
};

const MAX_STORABLE_DATA: usize = (CHUNKS_PER_BLOCK - 1) * CHUNK_SIZE; // 255 * 64 = 16320
const MAX_TOKEN_PAYLOAD: usize = 14000;


/// Convert a PropertyValue to a string for property index lookup.
fn prop_val_str(pv: &PropertyValue) -> String {
    match pv {
        PropertyValue::String(s) => s.clone(),
        PropertyValue::Integer(i) => i.to_string(),
        PropertyValue::Float(f) => f.to_string(),
        PropertyValue::Boolean(b) => b.to_string(),
        PropertyValue::List(_) => String::new(),
        PropertyValue::Null => String::new(),
    }
}

/// Insert property index entries for all registered keys.
fn index_vertex_properties(mi: &mut crate::storage::memory_index::MemoryIndex, properties: &HashMap<String, PropertyValue>, ptr: MetaPointer) {
    for (key, val) in properties {
        if mi.has_vertex_property(key) {
            let s = prop_val_str(val);
            if !s.is_empty() {
                mi.insert_vertex_property(key, &s, ptr);
            }
        }
    }
}

fn index_edge_properties(mi: &mut crate::storage::memory_index::MemoryIndex, properties: &HashMap<String, PropertyValue>, ptr: MetaPointer) {
    for (key, val) in properties {
        if mi.has_edge_property(key) {
            let s = prop_val_str(val);
            if !s.is_empty() {
                mi.insert_edge_property(key, &s, ptr);
            }
        }
    }
}

/// Remove property index entries for all registered keys.
fn unindex_vertex_properties(mi: &mut crate::storage::memory_index::MemoryIndex, properties: &HashMap<String, PropertyValue>, ptr: &MetaPointer) {
    for (key, val) in properties {
        if mi.has_vertex_property(key) {
            let s = prop_val_str(val);
            if !s.is_empty() {
                mi.remove_vertex_property(key, &s, ptr);
            }
        }
    }
}

fn unindex_edge_properties(mi: &mut crate::storage::memory_index::MemoryIndex, properties: &HashMap<String, PropertyValue>, ptr: &MetaPointer) {
    for (key, val) in properties {
        if mi.has_edge_property(key) {
            let s = prop_val_str(val);
            if !s.is_empty() {
                mi.remove_edge_property(key, &s, ptr);
            }
        }
    }
}

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

    let serialized = profile::time("ser_vertex", || serialize_vertex(&payload))?;
    let header = DataHeader::new_vertex(vid, serialized.len() as u16);
    let ptr = profile::time("write_data_record", || write_data_record(graph, &header, &serialized))?;

    // ── Update memory index ──────────────────────────────────────────
    profile::time("idx_insert", || {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.vertex_id.insert(vid, ptr);
        mi.vertex_name.insert(payload.name.clone(), vid);
        mi.rank.insert(1, ptr);
        for l in &payload.labels {
            mi.add_vertex_label(l, ptr);
        }
        index_vertex_properties(&mut mi, &payload.properties, ptr);
    });

    // ── Tokenize attributes ──────────────────────────────────────────
    profile::time("tokenize_vertex", || -> StorageResult<()> {
        tokenize_vertex(&graph, vid, &payload)
    })?;

    // ── WAL ──────────────────────────────────────────────────────────
    profile::time("wal_append", || -> StorageResult<()> {
        graph.redo_log.append(OpType::VertexCreate, vid as u64, &serialized)
    })?;

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
        mi.edge_id.insert(eid, ptr);
        mi.edge_name.insert(payload.name.clone(), eid);
        mi.rank.insert(1, ptr);
        mi.vertex_adjacency.add_edge(eid, source, target, ptr);
        for l in &payload.labels {
            mi.add_edge_label(l, ptr);
        }
        index_edge_properties(&mut mi, &payload.properties, ptr);
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
        mi.vertex_id.get(vid).copied()
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
        mi.edge_id.get(eid).copied()
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
        mi.vertex_id.get(vid).copied()
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
            mi.rank.remove(old_rank, &ptr);
            mi.rank.insert(rank, ptr);
        }
        if old_atime != atime {
            mi.atime.remove(old_atime, &ptr);
            mi.atime.insert(atime, ptr);
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
        mi.edge_id.get(eid).copied()
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
            mi.rank.remove(old_rank, &ptr);
            mi.rank.insert(rank, ptr);
        }
        if old_atime != atime {
            mi.atime.remove(old_atime, &ptr);
            mi.atime.insert(atime, ptr);
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
        let ptr = mi.vertex_id.get(vid).copied()
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
        // Update vertex property index: remove old entries, add new ones.
        {
            let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
            unindex_vertex_properties(&mut mi, &old_payload.properties, &old_ptr);
            index_vertex_properties(&mut mi, p, old_ptr);
        }
        new_payload.properties = p.clone();
    }

    // Push old payload to history if requested.
    // The history entry's timestamp is the old header's mtime — the moment
    // this state snapshot was last current before being superseded.
    if record_history {
        let mut old_payload_core = old_payload.clone();
        old_payload_core.history.clear();
        let old_bytes = serialize_vertex(&old_payload_core)?;
        new_payload.history.push(HistoryRecord {
            timestamp: old_header.mtime,
            data: old_bytes,
        });
        // Cap history to prevent unbounded growth.
        let max_history = graph.config.storage.time_travel_max_history;
        while new_payload.history.len() > max_history {
            new_payload.history.remove(0);
        }
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
        mi.vertex_id.insert(vid, new_ptr);

        mi.rank.remove(old_header.rank, &old_ptr);
        mi.rank.insert(new_header.rank, new_ptr);
        mi.atime.remove(old_header.atime, &old_ptr);
        mi.atime.insert(now, new_ptr);
        if let Some(n) = name {
            mi.vertex_name.remove(&old_payload.name);
            mi.vertex_name.insert(n.to_string(), vid);
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
        let ptr = mi.edge_id.get(eid).copied()
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
        // Update edge property index: remove old entries, add new ones.
        {
            let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
            unindex_edge_properties(&mut mi, &old_payload.properties, &old_ptr);
            index_edge_properties(&mut mi, p, old_ptr);
        }
        new_payload.properties = p.clone();
    }

    if record_history {
        let mut old_payload_core = old_payload.clone();
        old_payload_core.history.clear();
        let old_bytes = serialize_edge(&old_payload_core)?;
        new_payload.history.push(HistoryRecord {
            timestamp: old_header.mtime,
            data: old_bytes,
        });
        let max_history = graph.config.storage.time_travel_max_history;
        while new_payload.history.len() > max_history {
            new_payload.history.remove(0);
        }
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
        mi.edge_id.insert(eid, new_ptr);

        mi.rank.remove(old_header.rank, &old_ptr);
        mi.rank.insert(new_header.rank, new_ptr);
        mi.atime.remove(old_header.atime, &old_ptr);
        mi.atime.insert(now, new_ptr);

        // Update adjacency index with new pointer.
        mi.vertex_adjacency.remove_edge(old_payload.source, old_payload.target, &old_ptr);
        mi.vertex_adjacency.add_edge(eid, old_payload.source, old_payload.target, new_ptr);
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
    // 级联软删除所有关联边
    cascade_delete_edges(graph, vid, false)?;

    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.vertex_id.get(vid).copied()
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
        mi.rank.remove(old_rank, &ptr);
    }

    graph.redo_log.append(OpType::VertexDelete, vid as u64, &[])?;
    Ok(())
}

/// Hard-delete a vertex: remove data entirely.
pub fn hard_delete_vertex(graph: &Graph, vid: u32) -> StorageResult<()> {
    // 级联硬删除所有关联边
    cascade_delete_edges(graph, vid, true)?;

    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.vertex_id.get(vid).copied()
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
            mi.vertex_name.remove(&payload.name);
            for l in &payload.labels {
                mi.remove_vertex_label(l, &ptr);
            }
            unindex_vertex_properties(&mut mi, &payload.properties, &ptr);
        }
        mi.vertex_id.remove(vid);
        mi.rank.remove(header.rank, &ptr);
    }

    graph.redo_log.append(OpType::VertexDelete, vid as u64, &[])?;
    Ok(())
}

/// Soft-delete an edge.
pub fn soft_delete_edge(graph: &Graph, eid: u32) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.edge_id.get(eid).copied()
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
        mi.rank.remove(old_rank, &ptr);
        // Keep edge in adjacency for time-travel traversal
    }

    graph.redo_log.append(OpType::EdgeDelete, eid as u64, &[])?;
    Ok(())
}

/// Hard-delete an edge.
pub fn hard_delete_edge(graph: &Graph, eid: u32) -> StorageResult<()> {
    let ptr = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.edge_id.get(eid).copied()
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
            mi.edge_name.remove(&payload.name);
            mi.vertex_adjacency.remove_edge(payload.source, payload.target, &ptr);
            for l in &payload.labels {
                mi.remove_edge_label(l, &ptr);
            }
            unindex_edge_properties(&mut mi, &payload.properties, &ptr);
        }
        mi.edge_id.remove(eid);
        mi.rank.remove(header.rank, &ptr);
    }

    graph.redo_log.append(OpType::EdgeDelete, eid as u64, &[])?;
    Ok(())
}

/// 级联删除顶点关联的所有边。
///
/// `hard: true` 表示硬删除（释放数据块），`false` 表示软删除（标记 Deleted）。
fn cascade_delete_edges(graph: &Graph, vid: u32, hard: bool) -> StorageResult<()> {
    // 收集所有关联边的 ID（outgoing + incoming），避免在遍历中修改索引
    let edge_ids: Vec<u32> = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        let out = mi.vertex_adjacency.out_edges(vid);
        let inc = mi.vertex_adjacency.in_edges(vid);
        let mut ids: Vec<u32> = Vec::with_capacity(out.len() + inc.len());
        for (eid, _, _) in out {
            ids.push(*eid);
        }
        for (eid, _, _) in inc {
            ids.push(*eid);
        }
        ids
    };

    for eid in &edge_ids {
        if hard {
            hard_delete_edge(graph, *eid)?;
        } else {
            soft_delete_edge(graph, *eid)?;
        }
    }
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
            // Read the vertex ptr from the rebuilt index.
            let ptr = graph.memory_index.read().unwrap_or_else(|e| e.into_inner())
                .vertex_id.get(id).copied();
            if let Some(ptr) = ptr {
                // Read payload to get name / labels / properties for index cleanup.
                if let Ok(header) = read_data_header(graph, ptr) {
                    let payload_len = header.payload_len as usize;
                    let data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, payload_len as u16)
                        .unwrap_or_default();
                    if let Ok(payload) = deserialize_vertex(&data) {
                        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                        mi.vertex_name.remove(&payload.name);
                        for l in &payload.labels {
                            mi.remove_vertex_label(l, &ptr);
                        }
                        unindex_vertex_properties(&mut mi, &payload.properties, &ptr);
                        mi.rank.remove(header.rank, &ptr);
                        mi.vertex_id.remove(id);
                        // Free the data chunks so the next restart doesn't find them.
                        drop(mi);
                        let total_len = DATA_HEADER_SIZE + payload_len;
                        let chunks = BlockAllocator::chunks_needed(total_len);
                        let _ = free_data_chunks(graph, ptr.block_idx, ptr.chunk_offset, chunks as u8);
                    } else {
                        // Couldn't read payload — remove what we can.
                        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                        mi.rank.remove(header.rank, &ptr);
                        mi.vertex_id.remove(id);
                    }
                }
            }
        }
        OpType::EdgeDelete => {
            let id = entry.op_id as u32;
            let ptr = graph.memory_index.read().unwrap_or_else(|e| e.into_inner())
                .edge_id.get(id).copied();
            if let Some(ptr) = ptr {
                // Read payload to get name / labels / source / target / properties.
                if let Ok(header) = read_data_header(graph, ptr) {
                    let payload_len = header.payload_len as usize;
                    let data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, payload_len as u16)
                        .unwrap_or_default();
                    if let Ok(payload) = deserialize_edge(&data) {
                        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                        mi.edge_name.remove(&payload.name);
                        mi.vertex_adjacency.remove_edge(payload.source, payload.target, &ptr);
                        for l in &payload.labels {
                            mi.remove_edge_label(l, &ptr);
                        }
                        unindex_edge_properties(&mut mi, &payload.properties, &ptr);
                        mi.rank.remove(header.rank, &ptr);
                        mi.edge_id.remove(id);
                        drop(mi);
                        let total_len = DATA_HEADER_SIZE + payload_len;
                        let chunks = BlockAllocator::chunks_needed(total_len);
                        let _ = free_data_chunks(graph, ptr.block_idx, ptr.chunk_offset, chunks as u8);
                    }
                }
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
        if mi.vertex_id.contains(id) {
            return Ok(());
        }
    }

    let serialized = wal_data.to_vec();
    let header = DataHeader::new_vertex(id, serialized.len() as u16);
    let ptr = write_data_record(graph, &header, &serialized)?;

    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    mi.vertex_id.insert(id, ptr);
    mi.vertex_name.insert(payload.name.clone(), id);
    mi.rank.insert(header.rank, ptr);
    for l in &payload.labels {
        mi.add_vertex_label(l, ptr);
    }
    index_vertex_properties(&mut mi, &payload.properties, ptr);
    drop(mi);

    tokenize_vertex(graph, id, payload)?;
    Ok(())
}

/// Replay helper: write a vertex data record unconditionally (no duplicate check).
/// Used for VertexUpdate replay, where the WAL entry may contain a newer state
/// than what's on disk (if the update's dirty blocks weren't flushed before crash).
/// Cleans up old index entries before inserting the new ones.
fn replay_create_vertex_always(graph: &Graph, id: u32, payload: &VertexPayload, wal_data: &[u8]) -> StorageResult<()> {
    // Remove old index entries if vertex exists in the rebuilt index.
    if let Some(&old_ptr) = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).vertex_id.get(id) {
        if let Ok(old_header) = read_data_header(graph, old_ptr) {
            let old_plen = old_header.payload_len as usize;
            let old_data = read_data_chunks(graph, old_ptr.block_idx, old_ptr.chunk_offset + 1, old_plen as u16)
                .unwrap_or_default();
            if let Ok(old_payload) = deserialize_vertex(&old_data) {
                let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                mi.vertex_name.remove(&old_payload.name);
                for l in &old_payload.labels {
                    mi.remove_vertex_label(l, &old_ptr);
                }
                unindex_vertex_properties(&mut mi, &old_payload.properties, &old_ptr);
                mi.rank.remove(old_header.rank, &old_ptr);
            }
        }
    }

    let serialized = wal_data.to_vec();
    let header = DataHeader::new_vertex(id, serialized.len() as u16);
    let ptr = write_data_record(graph, &header, &serialized)?;

    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    mi.vertex_id.insert(id, ptr);
    mi.vertex_name.insert(payload.name.clone(), id);
    mi.rank.insert(header.rank, ptr);
    for l in &payload.labels {
        mi.add_vertex_label(l, ptr);
    }
    index_vertex_properties(&mut mi, &payload.properties, ptr);
    drop(mi);

    tokenize_vertex(graph, id, payload)?;
    Ok(())
}

/// Replay helper: recreate an edge from WAL data during startup recovery.
fn replay_create_edge(graph: &Graph, id: u32, payload: &EdgePayload, wal_data: &[u8]) -> StorageResult<()> {
    // Skip if this edge was already re-created during build_memory_index.
    {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        if mi.edge_id.contains(id) {
            return Ok(());
        }
    }

    let serialized = wal_data.to_vec();
    let header = DataHeader::new_edge(id, serialized.len() as u16);
    let ptr = write_data_record(graph, &header, &serialized)?;

    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    mi.edge_id.insert(id, ptr);
    mi.edge_name.insert(payload.name.clone(), id);
    mi.rank.insert(header.rank, ptr);
    mi.vertex_adjacency.add_edge(id, payload.source, payload.target, ptr);
    for l in &payload.labels {
        mi.add_edge_label(l, ptr);
    }
    index_edge_properties(&mut mi, &payload.properties, ptr);
    drop(mi);

    tokenize_edge(graph, id, payload)?;
    Ok(())
}

/// Replay helper: write an edge data record unconditionally (no duplicate check).
/// Used for EdgeUpdate replay, same rationale as replay_create_vertex_always.
fn replay_create_edge_always(graph: &Graph, id: u32, payload: &EdgePayload, wal_data: &[u8]) -> StorageResult<()> {
    // Remove old index entries if edge exists in the rebuilt index.
    if let Some(&old_ptr) = graph.memory_index.read().unwrap_or_else(|e| e.into_inner()).edge_id.get(id) {
        if let Ok(old_header) = read_data_header(graph, old_ptr) {
            let old_plen = old_header.payload_len as usize;
            let old_data = read_data_chunks(graph, old_ptr.block_idx, old_ptr.chunk_offset + 1, old_plen as u16)
                .unwrap_or_default();
            if let Ok(old_payload) = deserialize_edge(&old_data) {
                let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                mi.edge_name.remove(&old_payload.name);
                mi.vertex_adjacency.remove_edge(old_payload.source, old_payload.target, &old_ptr);
                for l in &old_payload.labels {
                    mi.remove_edge_label(l, &old_ptr);
                }
                unindex_edge_properties(&mut mi, &old_payload.properties, &old_ptr);
                mi.rank.remove(old_header.rank, &old_ptr);
            }
        }
    }

    let serialized = wal_data.to_vec();
    let header = DataHeader::new_edge(id, serialized.len() as u16);
    let ptr = write_data_record(graph, &header, &serialized)?;

    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
    mi.edge_id.insert(id, ptr);
    mi.edge_name.insert(payload.name.clone(), id);
    mi.rank.insert(header.rank, ptr);
    mi.vertex_adjacency.add_edge(id, payload.source, payload.target, ptr);
    for l in &payload.labels {
        mi.add_edge_label(l, ptr);
    }
    index_edge_properties(&mut mi, &payload.properties, ptr);
    drop(mi);

    tokenize_edge(graph, id, payload)?;
    Ok(())
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Allocate chunks for a new piece of data. Returns (block_idx, chunk_offset).
fn allocate_chunks(graph: &Graph, chunks_needed: u8) -> StorageResult<(u32, u8)> {
    let mut bf = graph.bitmap_file.write().unwrap_or_else(|e| e.into_inner());

    // Track how many blocks we've tried this round. When every free
    // block has been tried and none has enough contiguous space, discard
    // one to free up a slot for a fresh block allocation.
    let mut tried = 0usize;

    loop {
        let block_idx = match bf.peek_free_block() {
            Some(idx) => idx,
            None => {
                bf.alloc_new_blocks(|count| {
                    graph.data_file.allocate_blocks(count)
                })?;
                bf.peek_free_block().expect("fresh blocks must exist")
            }
        };

        // Remove from free_blocks for this attempt
        bf.consume_free_block(block_idx);

        let block_data = graph.block_cache.read_block_data(block_idx,
            |idx| graph.data_file.read_block(idx),
            &|idx, data| graph.data_file.write_block(idx, data),
        )?;
        let mut block_buf = block_data;
        let mut header = BlockHeader::decode(&block_buf);
        if let Some(off) = BlockAllocator::alloc_chunks(&mut header.bitmap, &mut header.offset, chunks_needed) {
            header.encode(&mut block_buf);
            let was_full = BlockAllocator::is_block_full(&header.bitmap);
            graph.block_cache.write_block_data(block_idx, &block_buf,
                |idx| graph.data_file.read_block(idx),
                &|idx, data| graph.data_file.write_block(idx, data),
            )?;

            if was_full {
                bf.mark_full(block_idx)?;
            } else {
                bf.mark_partial(block_idx);
            }
            return Ok((block_idx, off));
        }

        // This block doesn't have enough contiguous free chunks.
        tried += 1;
        // Detect if we've been around all free blocks without success.
        let free_count = bf.free_block_count();
        if tried > free_count {
            // Full circle with no success — discard the fragmented block
            // so consume_free_block causes peek_free_block to return None,
            // triggering alloc_new_blocks at the top of the loop.
            bf.consume_free_block(block_idx);
            tried = 0;
        } else {
            bf.mark_partial(block_idx);
        }
        // Continue loop to try next block
    }
}

/// Write padded data into the allocated chunks.
fn write_data_chunks(graph: &Graph, block_idx: u32, chunk_offset: u8, chunks: u8, data: &[u8]) -> StorageResult<()> {
    graph.block_cache.with_block(block_idx,
        |idx| graph.data_file.read_block(idx),
        &|idx, data| graph.data_file.write_block(idx, data),
        |block| {
            let start = (chunk_offset as usize) * 64;
            let end = start + (chunks as usize) * 64;
            let write_len = data.len().min(end - start);
            block[start..start + write_len].copy_from_slice(&data[..write_len]);
        },
    )?;
    Ok(())
}

/// Read data from chunks given the total data length.
pub(crate) fn read_data_chunks(graph: &Graph, block_idx: u32, chunk_offset: u8, data_len: u16) -> StorageResult<Vec<u8>> {
    let _chunks = BlockAllocator::chunks_needed(data_len as usize);
    graph.block_cache.with_block(block_idx,
        |idx| graph.data_file.read_block(idx),
        &|idx, data| graph.data_file.write_block(idx, data),
        |block| {
            let start = (chunk_offset as usize) * 64;
            let read_len = data_len as usize;
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
            data
        },
    )
}

/// Free previously allocated data chunks.
fn free_data_chunks(graph: &Graph, block_idx: u32, chunk_offset: u8, chunks: u8) -> StorageResult<()> {
    // Load block (from cache or disk) and free the chunks.
    // Always load even if not cached — avoiding the load causes chunk leaks
    // during WAL replay and other bulk operations.
    let was_full = graph.block_cache.with_block(block_idx,
        |idx| graph.data_file.read_block(idx),
        &|idx, data| graph.data_file.write_block(idx, data).map_err(|e| e.into()),
        |block| {
            let mut header = BlockHeader::decode(block);
            let wf = BlockAllocator::is_block_full(&header.bitmap);
            BlockAllocator::free_chunks(&mut header.bitmap, chunk_offset, chunks);
            header.encode(block);
            wf && !BlockAllocator::is_block_full(&header.bitmap)
        },
    )?;

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
///
/// Add or update a token entry — dispatches to batch or immediate.
fn add_token(graph: &Graph, token_str: &str, ref_type: u8, ref_id: u32, hits: &[crate::storage::types::Hit]) -> StorageResult<()> {
    if crate::graph::token_batch::is_active() {
        crate::graph::token_batch::buffer_add(graph, token_str, ref_type, ref_id, hits)
    } else {
        add_token_immediate(graph, token_str, ref_type, ref_id, hits)
    }
}

/// Append multiple refs to an existing token in one write (called from token_batch::flush_batch).
/// Reads the existing token segment, appends all refs at once, and writes a single new record.
/// Falls back to per-ref writes if the combined payload would exceed MAX_TOKEN_PAYLOAD.
pub fn add_token_batch(graph: &Graph, token_str: &str, refs: &[crate::graph::token_batch::PendingRef]) -> StorageResult<()> {
    // Read the first existing token segment.
    let (ptr, header, mut token_payload) = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        let ptrs = mi.token.get(token_str);
        let ptr = match ptrs.and_then(|v| v.first().copied()) {
            Some(p) => p,
            None => {
                // Token doesn't exist yet — create it with all refs.
                drop(mi);
                let seg = TokenPayload {
                    id: graph.alloc_token_id(),
                    token: token_str.to_string(),
                    refs: refs.iter().map(|pr| TokenRef {
                        ref_type: pr.ref_type, ref_id: pr.ref_id,
                        ref_version: 1, ref_frequency: pr.hits.len() as u16,
                        hits: pr.hits.clone(),
                    }).collect(),
                };
                let data = serialize::serialize_token(&seg)?;
                let h = DataHeader::new_token(seg.id, data.len() as u16);
                let new_ptr = write_data_record(graph, &h, &data)?;
                let mut mi2 = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                mi2.token.insert(token_str.to_string(), new_ptr);
                return Ok(());
            }
        };
        let h = read_data_header(graph, ptr)?;
        let plen = h.payload_len as usize;
        let data = read_data_chunks(graph, ptr.block_idx, ptr.chunk_offset + 1, plen as u16)?;
        let payload = serialize::deserialize_token(&data)
            .map_err(|e| StorageError::Other(format!("token deser: {e}")))?;
        (ptr, h, payload)
    };

    let old_payload_len = header.payload_len as usize;

    // Append all refs.
    for pr in refs {
        token_payload.refs.push(TokenRef {
            ref_type: pr.ref_type, ref_id: pr.ref_id,
            ref_version: 1, ref_frequency: pr.hits.len() as u16,
            hits: pr.hits.clone(),
        });
    }

    let new_data = serialize::serialize_token(&token_payload)?;

    // If the combined payload would overflow, fall back to per-ref writes.
    if new_data.len() > MAX_TOKEN_PAYLOAD {
        for pr in refs {
            add_token_immediate(graph, token_str, pr.ref_type, pr.ref_id, &pr.hits)?;
        }
        return Ok(());
    }

    let new_header = DataHeader {
        chunk_type: crate::storage::types::ChunkType::Token,
        status: DataStatus::Normal, version: 0,
        entity_id: token_payload.id,
        ctime: header.ctime, mtime: 0, atime: 0, rank: 0,
        payload_len: new_data.len() as u16,
    };

    let new_ptr = write_data_record(graph, &new_header, &new_data)?;

    // Update memory index.
    {
        let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
        mi.token.remove_pointer(token_str, &ptr);
        mi.token.insert(token_str.to_string(), new_ptr);
    }

    // Free old chunks.
    if old_payload_len > 0 {
        let old_total = (DATA_HEADER_SIZE + old_payload_len) as u16;
        let old_chunks = BlockAllocator::chunks_needed(old_total as usize);
        free_data_chunks(graph, ptr.block_idx, ptr.chunk_offset, old_chunks)?;
    }

    Ok(())
}

/// Add or update a token entry — immediate write (no batching).
pub(crate) fn add_token_immediate(graph: &Graph, token_str: &str, ref_type: u8, ref_id: u32, hits: &[crate::storage::types::Hit]) -> StorageResult<()> {
    // Check if token already exists in memory index.
    let existing = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.token.get(token_str).map(|v| v.clone())
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

                // If appending would exceed the safe limit, create a new segment.
                if new_data.len() > MAX_TOKEN_PAYLOAD {
                    let seg_payload = TokenPayload {
                        id: graph.alloc_token_id(),
                        token: token_str.to_string(),
                        refs: vec![TokenRef {
                            ref_type, ref_id, ref_version: 1,
                            ref_frequency: hits.len() as u16,
                            hits: hits.to_vec(),
                        }],
                    };
                    let seg_data = crate::graph::serialize::serialize_token(&seg_payload)?;
                    let seg_header = DataHeader::new_token(seg_payload.id, seg_data.len() as u16);
                    let seg_ptr = profile::time("token_write", || write_data_record(graph, &seg_header, &seg_data))?;
                    let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                    mi.token.insert(token_str.to_string(), seg_ptr);
                    return Ok(());
                }

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
                let new_ptr = profile::time("token_write", || write_data_record(graph, &new_header, &new_data))?;

                // Update token pointer in memory index.
                let mut mi = graph.memory_index.write().unwrap_or_else(|e| e.into_inner());
                mi.token.remove_pointer(token_str, &ptr);
                mi.token.insert(token_str.to_string(), new_ptr);

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
        mi.token.insert(token_str.to_string(), ptr);
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
        mi.rank.remove(old_rank, ptr);
        mi.rank.insert(new_rank, *ptr);
        mi.atime.remove(old_atime, ptr);
        mi.atime.insert(now, *ptr);
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
    graph.block_cache.with_block(ptr.block_idx,
        |idx| graph.data_file.read_block(idx),
        &|idx, data| graph.data_file.write_block(idx, data).map_err(|e| e.into()),
        |block| {
            let start = (ptr.chunk_offset as usize) * 64;
            buf.copy_from_slice(&block[start..start + 64]);
        },
    )?;
    Ok(DataHeader::decode(&buf))
}

/// Update a DataHeader in-place in the data file (only rank/atime fields change).
///
/// This modifies the first 64-byte chunk of the record in the cached block
/// and marks the block dirty. No WAL entry is needed — the change is
/// persisted at the next checkpoint.
pub fn update_header_in_place(graph: &Graph, ptr: &MetaPointer, header: &DataHeader) -> StorageResult<()> {
    graph.block_cache.with_block(ptr.block_idx,
        |idx| graph.data_file.read_block(idx),
        &|idx, data| graph.data_file.write_block(idx, data).map_err(|e| e.into()),
        |block| {
            let start = (ptr.chunk_offset as usize) * 64;
            let mut buf = [0u8; 64];
            header.encode(&mut buf);
            block[start..start + 64].copy_from_slice(&buf);
        },
    )?;
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
    graph.block_cache.with_block(block_idx,
        |idx| graph.data_file.read_block(idx),
        &|idx, data| graph.data_file.write_block(idx, data).map_err(|e| e.into()),
        |block| {
            let start = (chunk_offset as usize) * 64;
            let end = start + padded.min(BLOCK_SIZE - start);
            let mut buf = vec![0u8; end - start];
            buf.copy_from_slice(&block[start..end]);
            buf
        },
    )
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
    if total_len > MAX_STORABLE_DATA {
        return Err(StorageError::Other(format!(
            "data record too large: {} bytes (max {})",
            total_len, MAX_STORABLE_DATA
        )));
    }
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
