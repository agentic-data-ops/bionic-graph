//! Fundamental types for the block-based storage engine.
//!
//! Every vertex, edge, and token is stored as a serialized payload inside
//! 64-byte chunks within 16 KB blocks. Chunks are tracked via a bitmap in
//! each block's header.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Constants ────────────────────────────────────────────────────────────────

/// Total size of one data/index block: 16 KB.
pub const BLOCK_SIZE: usize = 16384;

/// Size of one allocation unit within a block: 64 B.
pub const CHUNK_SIZE: usize = 64;

/// How many chunks fit in one block (16384 / 64 = 256).
pub const CHUNKS_PER_BLOCK: usize = 256;

/// The first chunk (bytes 0..64) is always the block header.
pub const BLOCK_HEADER_SIZE: usize = 64;

// ── Identifier types ─────────────────────────────────────────────────────────

/// Unique vertex identifier within a graph.
pub type VertexId = u32;
/// Unique edge identifier within a graph.
pub type EdgeId = u32;
/// Unique token identifier within a graph.
pub type TokenId = u32;

/// Zero-based index of a 16 KB block within the data/index file.
pub type BlockIdx = u32;

/// Monotonically increasing version number for MVCC.
pub type Version = u16;

/// Wall-clock timestamp in microseconds since Unix epoch.
pub type TimestampUs = u64;

/// 1-based chunk offset inside a block (0 = invalid/unset).
pub type ChunkOffset = u8;

// ── Small enums (stored as u8 in binary layouts) ──────────────────────────────

/// What kind of record lives in an index chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ChunkType {
    Empty = 0x00,
    Vertex = 0x01,
    Edge = 0x02,
    Token = 0x03,
}

impl From<u8> for ChunkType {
    fn from(v: u8) -> Self {
        match v {
            0x01 => ChunkType::Vertex,
            0x02 => ChunkType::Edge,
            0x03 => ChunkType::Token,
            _ => ChunkType::Empty,
        }
    }
}

/// Data visibility status in an index record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DataStatus {
    Normal = 0x00,
    Deleted = 0x01,
}

impl From<u8> for DataStatus {
    fn from(v: u8) -> Self {
        match v {
            0x01 => DataStatus::Deleted,
            _ => DataStatus::Normal,
        }
    }
}

/// Whether a cached block is clean or needs flushing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BlockStatus {
    Normal = 0x00,
    Dirty = 0x01,
}

impl From<u8> for BlockStatus {
    fn from(v: u8) -> Self {
        match v {
            0x01 => BlockStatus::Dirty,
            _ => BlockStatus::Normal,
        }
    }
}

/// Redo-log operation type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum OpType {
    VertexCreate = 0x00,
    VertexDelete = 0x01,
    VertexUpdate = 0x02,
    VertexIndexUpdate = 0x03,
    EdgeCreate = 0x04,
    EdgeDelete = 0x05,
    EdgeUpdate = 0x06,
    EdgeIndexUpdate = 0x07,
    TokenCreate = 0x08,
    TokenUpdate = 0x09,
    TokenDelete = 0x0A,
    TokenIndexUpdate = 0x0B,
}

impl TryFrom<u8> for OpType {
    type Error = ();
    fn try_from(v: u8) -> Result<Self, ()> {
        match v {
            0x00 => Ok(OpType::VertexCreate),
            0x01 => Ok(OpType::VertexDelete),
            0x02 => Ok(OpType::VertexUpdate),
            0x03 => Ok(OpType::VertexIndexUpdate),
            0x04 => Ok(OpType::EdgeCreate),
            0x05 => Ok(OpType::EdgeDelete),
            0x06 => Ok(OpType::EdgeUpdate),
            0x07 => Ok(OpType::EdgeIndexUpdate),
            0x08 => Ok(OpType::TokenCreate),
            0x09 => Ok(OpType::TokenUpdate),
            0x0A => Ok(OpType::TokenDelete),
            0x0B => Ok(OpType::TokenIndexUpdate),
            _ => Err(()),
        }
    }
}

// ── Block header ─────────────────────────────────────────────────────────────

/// First 64 bytes of every 16 KB data/index block.
///
/// The `bitmap` field uses 256 bits — one per chunk. Bit 0 is reserved for the
/// header chunk itself and should always be set.
#[derive(Clone, Debug)]
pub struct BlockHeader {
    /// Allocation cursor: next free 1-based chunk offset to scan from.
    pub offset: u8,
    /// 256-bit bitmap: 1 = allocated, 0 = free. Bit 0 reserved for header.
    pub bitmap: [u8; 32],
    /// Block status (Normal / Dirty).
    pub status: BlockStatus,
    /// Last-update timestamp in microseconds.
    pub timestamp: u64,
    /// Reserved for future use.
    pub padding: [u8; 22],
}

impl BlockHeader {
    /// Encode the header into the first 64 bytes of a block buffer.
    pub fn encode(&self, buf: &mut [u8; BLOCK_SIZE]) {
        buf[0] = self.offset;
        buf[1..33].copy_from_slice(&self.bitmap);
        buf[33] = self.status as u8;
        buf[34..42].copy_from_slice(&self.timestamp.to_le_bytes());
        // padding[22] is already zeroed
    }

    /// Decode the header from the first 64 bytes of a block buffer.
    pub fn decode(buf: &[u8; BLOCK_SIZE]) -> Self {
        let mut bitmap = [0u8; 32];
        bitmap.copy_from_slice(&buf[1..33]);
        let mut ts_bytes = [0u8; 8];
        ts_bytes.copy_from_slice(&buf[34..42]);
        Self {
            offset: buf[0],
            bitmap,
            status: BlockStatus::from(buf[33]),
            timestamp: u64::from_le_bytes(ts_bytes),
            padding: [0u8; 22],
        }
    }

    /// Create a fresh header for a newly allocated block.
    pub fn fresh() -> Self {
        let mut bitmap = [0u8; 32];
        bitmap[0] = 0x01; // bit 0 = header chunk is allocated
        Self {
            offset: 1,
            bitmap,
            status: BlockStatus::Normal,
            timestamp: 0,
            padding: [0u8; 22],
        }
    }
}

// ── Property value ───────────────────────────────────────────────────────────

/// A dynamic property value stored on vertices and edges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    List(Vec<PropertyValue>),
    Null,
}

// ── Data payloads (variable-length, serialized with bincode) ─────────────────

/// The full vertex payload stored in data file chunks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VertexPayload {
    pub id: u32,
    pub name: String,
    pub labels: Vec<String>,
    pub keywords: Vec<String>,
    pub properties: HashMap<String, PropertyValue>,
    pub history: Vec<HistoryRecord>,
}

/// The full edge payload stored in data file chunks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgePayload {
    pub id: u32,
    /// Relationship name between source and target (e.g. "knows", "works_at").
    pub name: String,
    /// Relation type labels (e.g. "social", "professional").
    pub labels: Vec<String>,
    pub keywords: Vec<String>,
    pub strength: f32,
    pub properties: HashMap<String, PropertyValue>,
    pub source: u32,
    pub target: u32,
    pub history: Vec<HistoryRecord>,
}

/// Token payload — maps a token ID to its references across vertices/edges.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenPayload {
    pub id: u32,
    pub refs: Vec<TokenRef>,
}

/// A reference from a token back to a vertex or edge.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenRef {
    pub ref_type: u8, // 0 = vertex, 1 = edge
    pub ref_id: u32,
    pub ref_version: u16,
    pub ref_frequency: u16,
    pub hits: Vec<Hit>,
    /// Microsecond timestamp when this ref was created (for time-travel filtering).
    pub timestamp: u64,
}

/// A specific hit position (key + offset) within a vertex/edge attribute.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hit {
    pub hit_key: String,
    pub hit_offset: u16,
}

/// A historical snapshot of vertex or edge data at a given timestamp.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub timestamp: u64,
    /// Raw serialized VertexPayload or EdgePayload bytes.
    pub data: Vec<u8>,
}

// ── Error type ───────────────────────────────────────────────────────────────

/// Storage-layer errors.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialize(#[from] bincode::Error),

    #[error("Block {0} not found in cache")]
    BlockNotCached(BlockIdx),

    #[error("Not enough free chunks: requested {requested}, available {available}")]
    InsufficientChunks { requested: u8, available: u8 },

    #[error("Invalid chunk offset {0}")]
    InvalidChunkOffset(ChunkOffset),

    #[error("Block {0} is full")]
    BlockFull(BlockIdx),

    #[error("Redo log replay error at entry {seq}: {message}")]
    RedoLogReplay { seq: u64, message: String },

    #[error("Graph not found: {0}")]
    GraphNotFound(String),

    #[error("Other: {0}")]
    Other(String),
}

/// Convenience result alias.
pub type StorageResult<T> = Result<T, StorageError>;

impl StorageError {
    /// Clone a StorageError by remapping variants (needed since `std::io::Error`
    /// and `bincode::Error` do not implement Clone).
    pub fn to_error(&self) -> Self {
        match self {
            StorageError::Io(e) => StorageError::Io(std::io::Error::new(e.kind(), format!("{e}"))),
            StorageError::Serialize(e) => StorageError::Other(format!("serialize: {e}")),
            StorageError::BlockNotCached(idx) => StorageError::BlockNotCached(*idx),
            StorageError::InsufficientChunks { requested, available } => StorageError::InsufficientChunks { requested: *requested, available: *available },
            StorageError::InvalidChunkOffset(o) => StorageError::InvalidChunkOffset(*o),
            StorageError::BlockFull(idx) => StorageError::BlockFull(*idx),
            StorageError::RedoLogReplay { seq, message } => StorageError::RedoLogReplay { seq: *seq, message: message.clone() },
            StorageError::GraphNotFound(s) => StorageError::GraphNotFound(s.clone()),
            StorageError::Other(s) => StorageError::Other(s.clone()),
        }
    }
}

/// A 16 KB block with 512-byte alignment for O_DIRECT I/O.
#[repr(C, align(512))]
pub struct AlignedBlock(pub [u8; BLOCK_SIZE]);

impl AlignedBlock {
    pub fn new() -> Self { AlignedBlock([0u8; BLOCK_SIZE]) }
    pub fn as_bytes(&self) -> &[u8; BLOCK_SIZE] { &self.0 }
    pub fn as_mut_bytes(&mut self) -> &mut [u8; BLOCK_SIZE] { &mut self.0 }
}

impl Default for AlignedBlock { fn default() -> Self { Self::new() } }


