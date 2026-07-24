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
///
/// NOTE: rank/atime metadata changes are persisted via in-place DataHeader
/// updates (no separate WAL entries needed). Only structural changes
/// (create/delete/update vertex/edge/token payloads) are logged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum OpType {
    VertexCreate = 0x00,
    VertexDelete = 0x01,
    VertexUpdate = 0x02,
    EdgeCreate = 0x03,
    EdgeDelete = 0x04,
    EdgeUpdate = 0x05,
    TokenCreate = 0x06,
    TokenUpdate = 0x07,
    TokenDelete = 0x08,
}

impl TryFrom<u8> for OpType {
    type Error = ();
    fn try_from(v: u8) -> Result<Self, ()> {
        match v {
            0x00 => Ok(OpType::VertexCreate),
            0x01 => Ok(OpType::VertexDelete),
            0x02 => Ok(OpType::VertexUpdate),
            0x03 => Ok(OpType::EdgeCreate),
            0x04 => Ok(OpType::EdgeDelete),
            0x05 => Ok(OpType::EdgeUpdate),
            0x06 => Ok(OpType::TokenCreate),
            0x07 => Ok(OpType::TokenUpdate),
            0x08 => Ok(OpType::TokenDelete),
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

// ── Data header (fixed 64-byte prefix on every data record) ────────────────

/// Size of the fixed data header at the start of every data record (1 chunk).
pub const DATA_HEADER_SIZE: usize = 64;

/// Fixed 64-byte header at the start of every data record in the data file.
///
/// Layout (1 chunk = 64 bytes):
///   [0]    chunk_type (u8)
///   [1]    status (u8)
///   [2..4]  version (u16 LE)
///   [4..8]  entity_id (u32 LE)
///   [8..16] ctime (u64 LE)
///  [16..24] mtime (u64 LE)
///  [24..32] atime (u64 LE)
///  [32..36] rank (u32 LE)
///  [36..38] payload_len (u16 LE) — length of bincode payload following the header
///  [38..64] padding (zeros)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataHeader {
    pub chunk_type: ChunkType,
    pub status: DataStatus,
    pub version: u16,
    pub entity_id: u32,
    pub ctime: u64,
    pub mtime: u64,
    pub atime: u64,
    pub rank: u32,
    pub payload_len: u16,
}

impl DataHeader {
    pub fn new_vertex(vid: u32, payload_len: u16) -> Self {
        let now = timestamp_us();
        Self {
            chunk_type: ChunkType::Vertex,
            status: DataStatus::Normal,
            version: 1,
            entity_id: vid,
            ctime: now,
            mtime: now,
            atime: now,
            rank: 1,
            payload_len,
        }
    }

    pub fn new_edge(eid: u32, payload_len: u16) -> Self {
        let now = timestamp_us();
        Self {
            chunk_type: ChunkType::Edge,
            status: DataStatus::Normal,
            version: 1,
            entity_id: eid,
            ctime: now,
            mtime: now,
            atime: now,
            rank: 1,
            payload_len,
        }
    }

    pub fn new_token(tid: u32, payload_len: u16) -> Self {
        let now = timestamp_us();
        Self {
            chunk_type: ChunkType::Token,
            status: DataStatus::Normal,
            version: 0,
            entity_id: tid,
            ctime: now,
            mtime: 0,
            atime: 0,
            rank: 0,
            payload_len,
        }
    }

    /// Encode into a 64-byte chunk buffer.
    pub fn encode(&self, buf: &mut [u8; 64]) {
        buf[0] = self.chunk_type as u8;
        buf[1] = self.status as u8;
        buf[2..4].copy_from_slice(&self.version.to_le_bytes());
        buf[4..8].copy_from_slice(&self.entity_id.to_le_bytes());
        buf[8..16].copy_from_slice(&self.ctime.to_le_bytes());
        buf[16..24].copy_from_slice(&self.mtime.to_le_bytes());
        buf[24..32].copy_from_slice(&self.atime.to_le_bytes());
        buf[32..36].copy_from_slice(&self.rank.to_le_bytes());
        buf[36..38].copy_from_slice(&self.payload_len.to_le_bytes());
        // bytes 38..64 remain zeroed (padding)
    }

    /// Decode from a 64-byte chunk buffer.
    pub fn decode(buf: &[u8; 64]) -> Self {
        Self {
            chunk_type: ChunkType::from(buf[0]),
            status: DataStatus::from(buf[1]),
            version: u16::from_le_bytes([buf[2], buf[3]]),
            entity_id: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            ctime: u64::from_le_bytes([
                buf[8], buf[9], buf[10], buf[11],
                buf[12], buf[13], buf[14], buf[15],
            ]),
            mtime: u64::from_le_bytes([
                buf[16], buf[17], buf[18], buf[19],
                buf[20], buf[21], buf[22], buf[23],
            ]),
            atime: u64::from_le_bytes([
                buf[24], buf[25], buf[26], buf[27],
                buf[28], buf[29], buf[30], buf[31],
            ]),
            rank: u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]),
            payload_len: u16::from_le_bytes([buf[36], buf[37]]),
        }
    }
}

// ── Data payloads (variable-length, serialized with bincode) ─────────────────

/// The full vertex payload stored in data file chunks (after the DataHeader).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VertexPayload {
    pub name: String,
    pub labels: Vec<String>,
    pub keywords: Vec<String>,
    pub properties: HashMap<String, PropertyValue>,
    pub history: Vec<HistoryRecord>,
}

/// The full edge payload stored in data file chunks (after the DataHeader).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgePayload {
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
/// Stored after the DataHeader in the data file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenPayload {
    pub id: u32,
    pub token: String,
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

/// Current wall-clock time in microseconds since Unix epoch.
pub fn timestamp_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
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


