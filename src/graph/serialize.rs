//! Bincode serialization helpers for vertex/edge/token payloads.
//!
//! Properties (`HashMap<String, PropertyValue>`) are stored as JSON strings
//! inside the bincode payload because `PropertyValue` uses `#[serde(untagged)]`
//! which is incompatible with bincode's binary format.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::storage::types::{EdgePayload, HistoryRecord, PropertyValue, StorageResult, TokenPayload, VertexPayload};

// ── Storage-friendly structs (properties stored as JSON strings) ────────────

#[derive(Serialize, Deserialize)]
struct StoredVertex {
    pub id: u32,
    pub labels: Vec<String>,
    pub keywords: Vec<String>,
    pub properties_json: String,
    pub history: Vec<StoredHistoryRecord>,
}

#[derive(Serialize, Deserialize)]
struct StoredEdge {
    pub id: u32,
    pub labels: Vec<String>,
    pub keywords: Vec<String>,
    pub strength: f32,
    pub properties_json: String,
    pub source: u32,
    pub target: u32,
    pub history: Vec<StoredHistoryRecord>,
}

#[derive(Serialize, Deserialize)]
struct StoredHistoryRecord {
    pub timestamp: u64,
    pub data: Vec<u8>,
}

// ── Conversion helpers ──────────────────────────────────────────────────────

fn props_to_json(props: &HashMap<String, PropertyValue>) -> String {
    serde_json::to_string(props).unwrap_or_else(|_| "{}".to_string())
}

fn props_from_json(s: &str) -> HashMap<String, PropertyValue> {
    serde_json::from_str(s).unwrap_or_default()
}

fn vertex_to_stored(v: &VertexPayload) -> StoredVertex {
    StoredVertex {
        id: v.id,
        labels: v.labels.clone(),
        keywords: v.keywords.clone(),
        properties_json: props_to_json(&v.properties),
        history: v.history.iter().map(|h| StoredHistoryRecord {
            timestamp: h.timestamp,
            data: h.data.clone(),
        }).collect(),
    }
}

fn stored_to_vertex(s: StoredVertex) -> VertexPayload {
    VertexPayload {
        id: s.id,
        name: String::new(),
        labels: s.labels,
        keywords: s.keywords,
        properties: props_from_json(&s.properties_json),
        history: s.history.into_iter().map(|h| HistoryRecord {
            timestamp: h.timestamp,
            data: h.data,
        }).collect(),
    }
}

fn edge_to_stored(e: &EdgePayload) -> StoredEdge {
    StoredEdge {
        id: e.id,
        labels: e.labels.clone(),
        keywords: e.keywords.clone(),
        strength: e.strength,
        properties_json: props_to_json(&e.properties),
        source: e.source,
        target: e.target,
        history: e.history.iter().map(|h| StoredHistoryRecord {
            timestamp: h.timestamp,
            data: h.data.clone(),
        }).collect(),
    }
}

fn stored_to_edge(s: StoredEdge) -> EdgePayload {
    EdgePayload {
        id: s.id,
        name: String::new(),
        labels: s.labels,
        keywords: s.keywords,
        strength: s.strength,
        properties: props_from_json(&s.properties_json),
        source: s.source,
        target: s.target,
        history: s.history.into_iter().map(|h| HistoryRecord {
            timestamp: h.timestamp,
            data: h.data,
        }).collect(),
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

pub fn serialize_vertex(v: &VertexPayload) -> StorageResult<Vec<u8>> {
    Ok(bincode::serialize(&vertex_to_stored(v))?)
}

pub fn deserialize_vertex(data: &[u8]) -> StorageResult<VertexPayload> {
    let stored: StoredVertex = bincode::deserialize(data)?;
    Ok(stored_to_vertex(stored))
}

pub fn serialize_edge(e: &EdgePayload) -> StorageResult<Vec<u8>> {
    Ok(bincode::serialize(&edge_to_stored(e))?)
}

pub fn deserialize_edge(data: &[u8]) -> StorageResult<EdgePayload> {
    let stored: StoredEdge = bincode::deserialize(data)?;
    Ok(stored_to_edge(stored))
}

pub fn serialize_token(t: &TokenPayload) -> StorageResult<Vec<u8>> {
    Ok(bincode::serialize(t)?)
}

pub fn deserialize_token(data: &[u8]) -> StorageResult<TokenPayload> {
    Ok(bincode::deserialize(data)?)
}

/// Compute the token data length (padded to 64-byte boundary).
pub fn token_data_len(data: &[u8]) -> u16 {
    let padded = if data.len() % 64 == 0 {
        data.len()
    } else {
        ((data.len() / 64) + 1) * 64
    };
    padded as u16
}
