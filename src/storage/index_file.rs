//! Block-based index file for vertex, edge, and token index records.
//!
//! Each block is 16 KB and uses the same `BlockHeader` format as the data
//! file. Each 64-byte chunk stores exactly one fixed-size index record.
//! One block holds 255 index records (chunk 0 = header).
//!
//! ## Record types (all 64 bytes)
//!
//! - `VertexIndexRecord`: maps VertexId → data location + rank
//! - `EdgeIndexRecord`:   maps EdgeId → data location + rank + source/target
//! - `TokenIndexRecord`:  maps token string → data location

use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::Path,
    sync::Mutex,
};

use crate::storage::block_allocator::BlockAllocator;
use crate::storage::types::{
    BlockHeader, BlockIdx, ChunkOffset, ChunkType, DataStatus, StorageResult, BLOCK_SIZE,
};

// ── Fixed-size index records (each exactly 64 bytes = 1 chunk) ───────────────

/// Fixed-size index records (vertex/edge = 128 bytes = 2 chunks, token = 64 bytes = 1 chunk).
///
/// Vertex/edge records span two consecutive 64-byte chunks:
///   - Chunk 0: existing compact fields (same layout as before)
///   - Chunk 1: name (64 bytes, null-terminated)

/// Index record for a vertex (128 bytes = 2 chunks).
#[derive(Clone, Debug, PartialEq)]
pub struct VertexIndexRecord {
    pub chunk_type: ChunkType,
    pub vertex_id: u32,
    pub data_block_idx: u32,
    pub data_chunk_offset: u8,
    pub data_len: u16,
    pub status: DataStatus,
    pub version: u16,
    pub ctime: u64,
    pub mtime: u64,
    pub atime: u64,
    pub rank: u32,
    /// 64-byte null-terminated name. Truncated if > 63 bytes.
    pub name: [u8; 64],
}

/// Number of chunks used by vertex/edge index records.
pub const INDEX_RECORD_CHUNKS: u8 = 2;

impl VertexIndexRecord {
    pub fn new(vertex_id: u32, data_block_idx: u32, data_chunk_offset: u8, data_len: u16) -> Self {
        let now = timestamp_us();
        Self {
            chunk_type: ChunkType::Vertex,
            vertex_id,
            data_block_idx,
            data_chunk_offset,
            data_len,
            status: DataStatus::Normal,
            version: 1,
            ctime: now,
            mtime: now,
            atime: now,
            rank: 1,
            name: [0u8; 64],
        }
    }

    /// Set the name field from a string (truncates to 63 bytes + NUL).
    pub fn set_name(&mut self, name: &str) {
        let mut buf = [0u8; 64];
        let bytes = name.as_bytes();
        let len = bytes.len().min(63);
        buf[..len].copy_from_slice(&bytes[..len]);
        self.name = buf;
    }

    /// Get the name as a string (up to null terminator).
    pub fn get_name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(64);
        std::str::from_utf8(&self.name[..end]).unwrap_or("")
    }

    /// Encode into two consecutive 64-byte chunks.
    /// `buf` must be exactly 128 bytes.
    pub fn encode(&self, buf: &mut [u8; 128]) {
        // Chunk 0: compact fields (same layout as before)
        buf[0] = self.chunk_type as u8;
        buf[1..5].copy_from_slice(&self.vertex_id.to_le_bytes());
        buf[5..9].copy_from_slice(&self.data_block_idx.to_le_bytes());
        buf[9] = self.data_chunk_offset;
        buf[10..12].copy_from_slice(&self.data_len.to_le_bytes());
        buf[12] = self.status as u8;
        buf[13..15].copy_from_slice(&self.version.to_le_bytes());
        buf[15..23].copy_from_slice(&self.ctime.to_le_bytes());
        buf[23..31].copy_from_slice(&self.mtime.to_le_bytes());
        buf[31..39].copy_from_slice(&self.atime.to_le_bytes());
        buf[39..43].copy_from_slice(&self.rank.to_le_bytes());
        // Chunk 1: name
        buf[64..128].copy_from_slice(&self.name);
    }

    /// Decode from two consecutive 64-byte chunks.
    /// `buf` must be exactly 128 bytes.
    pub fn decode(buf: &[u8; 128]) -> Self {
        let mut name = [0u8; 64];
        name.copy_from_slice(&buf[64..128]);
        Self {
            chunk_type: ChunkType::from(buf[0]),
            vertex_id: u32::from_le_bytes(buf[1..5].try_into().unwrap()),
            data_block_idx: u32::from_le_bytes(buf[5..9].try_into().unwrap()),
            data_chunk_offset: buf[9],
            data_len: u16::from_le_bytes(buf[10..12].try_into().unwrap()),
            status: DataStatus::from(buf[12]),
            version: u16::from_le_bytes(buf[13..15].try_into().unwrap()),
            ctime: u64::from_le_bytes(buf[15..23].try_into().unwrap()),
            mtime: u64::from_le_bytes(buf[23..31].try_into().unwrap()),
            atime: u64::from_le_bytes(buf[31..39].try_into().unwrap()),
            rank: u32::from_le_bytes(buf[39..43].try_into().unwrap()),
            name,
        }
    }

    /// Mark record as deleted (soft-delete for history queries).
    pub fn mark_deleted(&mut self) {
        self.status = DataStatus::Deleted;
    }
}

/// Index record for an edge (128 bytes = 2 chunks).
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeIndexRecord {
    pub chunk_type: ChunkType,
    pub edge_id: u32,
    pub data_block_idx: u32,
    pub data_chunk_offset: u8,
    pub data_len: u16,
    pub status: DataStatus,
    pub version: u16,
    pub ctime: u64,
    pub mtime: u64,
    pub atime: u64,
    pub rank: u32,
    pub source: u32,
    pub target: u32,
    /// 64-byte null-terminated name. Truncated if > 63 bytes.
    pub name: [u8; 64],
}

impl EdgeIndexRecord {
    pub fn new(
        edge_id: u32,
        source: u32,
        target: u32,
        data_block_idx: u32,
        data_chunk_offset: u8,
        data_len: u16,
    ) -> Self {
        let now = timestamp_us();
        Self {
            chunk_type: ChunkType::Edge,
            edge_id,
            data_block_idx,
            data_chunk_offset,
            data_len,
            status: DataStatus::Normal,
            version: 1,
            ctime: now,
            mtime: now,
            atime: now,
            rank: 1,
            source,
            target,
            name: [0u8; 64],
        }
    }

    /// Set the name field from a string (truncates to 63 bytes + NUL).
    pub fn set_name(&mut self, name: &str) {
        let mut buf = [0u8; 64];
        let bytes = name.as_bytes();
        let len = bytes.len().min(63);
        buf[..len].copy_from_slice(&bytes[..len]);
        self.name = buf;
    }

    /// Get the name as a string (up to null terminator).
    pub fn get_name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(64);
        std::str::from_utf8(&self.name[..end]).unwrap_or("")
    }

    pub fn encode(&self, buf: &mut [u8; 128]) {
        buf[0] = self.chunk_type as u8;
        buf[1..5].copy_from_slice(&self.edge_id.to_le_bytes());
        buf[5..9].copy_from_slice(&self.data_block_idx.to_le_bytes());
        buf[9] = self.data_chunk_offset;
        buf[10..12].copy_from_slice(&self.data_len.to_le_bytes());
        buf[12] = self.status as u8;
        buf[13..15].copy_from_slice(&self.version.to_le_bytes());
        buf[15..23].copy_from_slice(&self.ctime.to_le_bytes());
        buf[23..31].copy_from_slice(&self.mtime.to_le_bytes());
        buf[31..39].copy_from_slice(&self.atime.to_le_bytes());
        buf[39..43].copy_from_slice(&self.rank.to_le_bytes());
        buf[43..47].copy_from_slice(&self.source.to_le_bytes());
        buf[47..51].copy_from_slice(&self.target.to_le_bytes());
        // Chunk 1: name
        buf[64..128].copy_from_slice(&self.name);
    }

    pub fn decode(buf: &[u8; 128]) -> Self {
        let mut name = [0u8; 64];
        name.copy_from_slice(&buf[64..128]);
        Self {
            chunk_type: ChunkType::from(buf[0]),
            edge_id: u32::from_le_bytes(buf[1..5].try_into().unwrap()),
            data_block_idx: u32::from_le_bytes(buf[5..9].try_into().unwrap()),
            data_chunk_offset: buf[9],
            data_len: u16::from_le_bytes(buf[10..12].try_into().unwrap()),
            status: DataStatus::from(buf[12]),
            version: u16::from_le_bytes(buf[13..15].try_into().unwrap()),
            ctime: u64::from_le_bytes(buf[15..23].try_into().unwrap()),
            mtime: u64::from_le_bytes(buf[23..31].try_into().unwrap()),
            atime: u64::from_le_bytes(buf[31..39].try_into().unwrap()),
            rank: u32::from_le_bytes(buf[39..43].try_into().unwrap()),
            source: u32::from_le_bytes(buf[43..47].try_into().unwrap()),
            target: u32::from_le_bytes(buf[47..51].try_into().unwrap()),
            name,
        }
    }

    pub fn mark_deleted(&mut self) {
        self.status = DataStatus::Deleted;
    }
}

/// Index record for a token (64 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct TokenIndexRecord {
    pub chunk_type: ChunkType,
    pub token_id: u32,
    pub data_block_idx: u32,
    pub data_chunk_offset: u8,
    pub data_len: u16,
    pub status: DataStatus,
    pub ctime: u64,
    /// Inline token string, null-terminated, max 42 chars + NUL.
    pub token: [u8; 43],
}

impl TokenIndexRecord {
    pub fn new(token_id: u32, token_str: &str, data_block_idx: u32, data_chunk_offset: u8, data_len: u16) -> Self {
        let now = timestamp_us();
        let mut token = [0u8; 43];
        let bytes = token_str.as_bytes();
        let len = bytes.len().min(42);
        token[..len].copy_from_slice(&bytes[..len]);
        Self {
            chunk_type: ChunkType::Token,
            token_id,
            data_block_idx,
            data_chunk_offset,
            data_len,
            status: DataStatus::Normal,
            ctime: now,
            token,
        }
    }

    /// Return the token string (up to null terminator).
    pub fn token_str(&self) -> &str {
        let end = self.token.iter().position(|&b| b == 0).unwrap_or(43);
        std::str::from_utf8(&self.token[..end]).unwrap_or("")
    }

    pub fn encode(&self, buf: &mut [u8; 64]) {
        buf[0] = self.chunk_type as u8;
        buf[1..5].copy_from_slice(&self.token_id.to_le_bytes());
        buf[5..9].copy_from_slice(&self.data_block_idx.to_le_bytes());
        buf[9] = self.data_chunk_offset;
        buf[10..12].copy_from_slice(&self.data_len.to_le_bytes());
        buf[12] = self.status as u8;
        buf[13..21].copy_from_slice(&self.ctime.to_le_bytes());
        buf[21..64].copy_from_slice(&self.token);
    }

    pub fn decode(buf: &[u8; 64]) -> Self {
        let mut token = [0u8; 43];
        token.copy_from_slice(&buf[21..64]);
        Self {
            chunk_type: ChunkType::from(buf[0]),
            token_id: u32::from_le_bytes(buf[1..5].try_into().unwrap()),
            data_block_idx: u32::from_le_bytes(buf[5..9].try_into().unwrap()),
            data_chunk_offset: buf[9],
            data_len: u16::from_le_bytes(buf[10..12].try_into().unwrap()),
            status: DataStatus::from(buf[12]),
            ctime: u64::from_le_bytes(buf[13..21].try_into().unwrap()),
            token,
        }
    }
}

// ── Index file ───────────────────────────────────────────────────────────────

/// A file storing fixed-size index records in 16 KB blocks.
///
/// Each block's 255 data chunks each contain one 64-byte index record.
pub struct IndexFile {
    file: Mutex<File>,
    path: std::path::PathBuf,
    /// Cached block count.
    block_count: std::sync::atomic::AtomicU64,
}

impl IndexFile {
    /// Open (or create) an index file at `path`.
    pub fn open<P: AsRef<Path>>(path: P) -> StorageResult<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path.as_ref())?;
        let block_count = file.metadata()?.len() / (BLOCK_SIZE as u64);
        Ok(Self {
            file: Mutex::new(file),
            path: path.as_ref().to_path_buf(),
            block_count: std::sync::atomic::AtomicU64::new(block_count),
        })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn block_count(&self) -> u64 {
        self.block_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    // ── Allocate ─────────────────────────────────────────────────────────────

    /// Allocate a new index record (any type). Writes the 64-byte `record`
    /// into a free chunk and returns its location.
    pub fn alloc_record(&self, record: &[u8; 64]) -> StorageResult<(BlockIdx, ChunkOffset)> {
        self.alloc_chunks(1, record)
    }

    /// Allocate a multi-chunk (128-byte) index record across 2 consecutive chunks.
    /// `record` must be exactly 128 bytes.
    pub fn alloc_record_128(&self, record: &[u8; 128]) -> StorageResult<(BlockIdx, ChunkOffset)> {
        self.alloc_chunks(2, record)
    }

    /// Allocate `num_chunks` consecutive chunks and write `data`.
    fn alloc_chunks(&self, num_chunks: u8, data: &[u8]) -> StorageResult<(BlockIdx, ChunkOffset)> {
        let mut file = self.file.lock().unwrap();
        let count = self.block_count();

        // If no blocks yet, create one.
        if count == 0 {
            let _idx = Self::append_block(&mut file)?;
            self.block_count.store(1, std::sync::atomic::Ordering::Relaxed);
        }

        let data_len = (num_chunks as usize) * 64;

        // Scan blocks from end to find free chunks.
        let total = self.block_count();
        for scan_idx in (0..total).rev() {
            let idx = scan_idx as u32;
            let mut block = Self::read_block(&file, idx)?;
            let mut header = BlockHeader::decode(&block);

            if let Some(chunk_off) = BlockAllocator::alloc_chunks(&mut header.bitmap, num_chunks) {
                // Found free space — write header + data.
                header.encode(&mut block);
                let start = (chunk_off as usize) * 64;
                block[start..start + data_len].copy_from_slice(data);
                Self::write_block(&mut file, idx, &block)?;
                return Ok((idx, chunk_off));
            }
        }

        // All existing blocks are full — append a new one.
        let idx = Self::append_block(&mut file)?;
        self.block_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut block = Self::read_block(&file, idx)?;
        let mut header = BlockHeader::decode(&block);
        let chunk_off = BlockAllocator::alloc_chunks(&mut header.bitmap, num_chunks)
            .expect("fresh block must have free chunks");
        header.encode(&mut block);
        let start = (chunk_off as usize) * 64;
        block[start..start + data_len].copy_from_slice(data);
        Self::write_block(&mut file, idx, &block)?;
        Ok((idx, chunk_off))
    }

    // ── Read ─────────────────────────────────────────────────────────────────

    /// Read a 64-byte chunk.
    fn read_chunk(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset) -> StorageResult<[u8; 64]> {
        let file = self.file.lock().unwrap();
        let block = Self::read_block(&file, block_idx)?;
        let start = (chunk_offset as usize) * 64;
        let mut chunk = [0u8; 64];
        chunk.copy_from_slice(&block[start..start + 64]);
        Ok(chunk)
    }

    /// Read a 128-byte (2-chunk) record starting at `chunk_offset`.
    fn read_chunk2(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset) -> StorageResult<[u8; 128]> {
        let file = self.file.lock().unwrap();
        let block = Self::read_block(&file, block_idx)?;
        let start = (chunk_offset as usize) * 64;
        let mut buf = [0u8; 128];
        buf[..64].copy_from_slice(&block[start..start + 64]);
        buf[64..128].copy_from_slice(&block[start + 64..start + 128]);
        Ok(buf)
    }

    pub fn read_vertex_record(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset) -> StorageResult<VertexIndexRecord> {
        let buf = self.read_chunk2(block_idx, chunk_offset)?;
        Ok(VertexIndexRecord::decode(&buf))
    }

    pub fn read_edge_record(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset) -> StorageResult<EdgeIndexRecord> {
        let buf = self.read_chunk2(block_idx, chunk_offset)?;
        Ok(EdgeIndexRecord::decode(&buf))
    }

    pub fn read_token_record(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset) -> StorageResult<TokenIndexRecord> {
        let chunk = self.read_chunk(block_idx, chunk_offset)?;
        Ok(TokenIndexRecord::decode(&chunk))
    }

    // ── Update ────────────────────────────────────────────────────────────────

    pub fn update_vertex_record(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset, record: &VertexIndexRecord) -> StorageResult<()> {
        let mut buf = [0u8; 128];
        record.encode(&mut buf);
        self.write_chunks(block_idx, chunk_offset, &buf)
    }

    pub fn update_edge_record(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset, record: &EdgeIndexRecord) -> StorageResult<()> {
        let mut buf = [0u8; 128];
        record.encode(&mut buf);
        self.write_chunks(block_idx, chunk_offset, &buf)
    }

    pub fn update_token_record(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset, record: &TokenIndexRecord) -> StorageResult<()> {
        let mut chunk = [0u8; 64];
        record.encode(&mut chunk);
        self.write_chunk(block_idx, chunk_offset, &chunk)
    }

    // ── Delete ────────────────────────────────────────────────────────────────

    /// Clear an index record by zeroing its chunk.
    pub fn delete_record(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset) -> StorageResult<()> {
        self.write_chunk(block_idx, chunk_offset, &[0u8; 64])
    }

    // ── Scan (for index rebuild at startup) ──────────────────────────────────

    /// Iterate over all non-empty index records, calling `visitor` for each.
    /// For vertex/edge records (2 chunks), passes a 128-byte slice.
    /// For token records (1 chunk), passes a 64-byte slice.
    pub fn scan<F>(&self, mut visitor: F) -> StorageResult<()>
    where
        F: FnMut(BlockIdx, ChunkOffset, &[u8]) -> StorageResult<()>,
    {
        let file = self.file.lock().unwrap();
        let count = self.block_count();

        for block_idx in 0..count as u32 {
            let block = Self::read_block(&file, block_idx)?;
            let header = BlockHeader::decode(&block);

            let mut chunk_off: u16 = 1;
            while chunk_off <= 255u16 {
                let bit_pos = chunk_off as usize;
                if (header.bitmap[bit_pos / 8] & (1 << (bit_pos % 8))) == 0 {
                    chunk_off += 1;
                    continue;
                }
                let start = (chunk_off as usize) * 64;
                let chunk_type = block[start];
                if chunk_type == 0x00 {
                    chunk_off += 1;
                    continue;
                }

                if (chunk_type == ChunkType::Vertex as u8 || chunk_type == ChunkType::Edge as u8) && chunk_off < 255 {
                    // 2-chunk record: read 128 bytes
                    let mut buf = [0u8; 128];
                    buf[..64].copy_from_slice(&block[start..start + 64]);
                    buf[64..128].copy_from_slice(&block[start + 64..start + 128]);
                    visitor(block_idx, chunk_off as u8, &buf)?;
                    chunk_off += 2;
                } else {
                    // 1-chunk record (token)
                    let mut chunk = [0u8; 64];
                    chunk.copy_from_slice(&block[start..start + 64]);
                    visitor(block_idx, chunk_off as u8, &chunk)?;
                    chunk_off += 1;
                }
            }
        }
        Ok(())
    }

    // ── Sync ─────────────────────────────────────────────────────────────────

    /// Flush all buffered data to disk.
    pub fn sync_all(&self) -> StorageResult<()> {
        let file = self.file.lock().unwrap();
        file.sync_all()?;
        Ok(())
    }

    /// Flush dirty index blocks to disk (forward-compat wrapper for checkpoint).
    pub fn flush_dirty(&self) -> StorageResult<()> {
        self.sync_all()
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    /// Write a single 64-byte chunk.
    fn write_chunk(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset, data: &[u8; 64]) -> StorageResult<()> {
        let mut file = self.file.lock().unwrap();
        let mut block = Self::read_block(&file, block_idx)?;
        let start = (chunk_offset as usize) * 64;
        block[start..start + 64].copy_from_slice(data);
        Self::write_block(&mut file, block_idx, &block)?;
        Ok(())
    }

    /// Write a 128-byte (2-chunk) record starting at `chunk_offset`.
    fn write_chunks(&self, block_idx: BlockIdx, chunk_offset: ChunkOffset, data: &[u8; 128]) -> StorageResult<()> {
        let mut file = self.file.lock().unwrap();
        let mut block = Self::read_block(&file, block_idx)?;
        let start = (chunk_offset as usize) * 64;
        block[start..start + 128].copy_from_slice(data);
        Self::write_block(&mut file, block_idx, &block)?;
        Ok(())
    }

    fn read_block(file: &File, idx: BlockIdx) -> std::io::Result<Box<[u8; BLOCK_SIZE]>> {
        let mut buf = Box::new([0u8; BLOCK_SIZE]);
        let offset = (idx as u64) * (BLOCK_SIZE as u64);
        let file_len = file.metadata()?.len();
        if offset >= file_len {
            return Ok(buf);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            file.read_exact_at(&mut *buf, offset)?;
        }
        #[cfg(not(unix))]
        {
            let mut dup = file.try_clone()?;
            dup.seek(SeekFrom::Start(offset))?;
            dup.read_exact(&mut *buf)?;
        }
        Ok(buf)
    }

    fn write_block(file: &mut File, idx: BlockIdx, data: &[u8; BLOCK_SIZE]) -> std::io::Result<()> {
        let offset = (idx as u64) * (BLOCK_SIZE as u64);
        let file_len = file.metadata()?.len();
        if offset + BLOCK_SIZE as u64 > file_len {
            file.set_len(offset + BLOCK_SIZE as u64)?;
        }
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)?;
        Ok(())
    }

    fn append_block(file: &mut File) -> std::io::Result<u32> {
        let file_len = file.metadata()?.len();
        let idx = (file_len / (BLOCK_SIZE as u64)) as u32;
        let mut buf = Box::new([0u8; BLOCK_SIZE]);
        let header = BlockHeader::fresh();
        header.encode(&mut buf);
        Self::write_block(file, idx, &buf)?;
        Ok(idx)
    }
}

fn timestamp_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_vertex_record_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("index.idx");
        let idx = IndexFile::open(&path).unwrap();

        let rec = VertexIndexRecord::new(42, 7, 3, 128);
        let (block, chunk) = idx.alloc_record_128(&{
            let mut buf = [0u8; 128];
            rec.encode(&mut buf);
            buf
        }).unwrap();

        let read_back = idx.read_vertex_record(block, chunk).unwrap();
        assert_eq!(read_back.vertex_id, 42);
        assert_eq!(read_back.data_block_idx, 7);
        assert_eq!(read_back.data_chunk_offset, 3);
        assert_eq!(read_back.data_len, 128);
        assert_eq!(read_back.status, DataStatus::Normal);
    }

    #[test]
    fn test_edge_record_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("index.idx");
        let idx = IndexFile::open(&path).unwrap();

        let rec = EdgeIndexRecord::new(100, 1, 2, 0, 1, 64);
        let (block, chunk) = idx.alloc_record_128(&{
            let mut buf = [0u8; 128];
            rec.encode(&mut buf);
            buf
        }).unwrap();

        let read_back = idx.read_edge_record(block, chunk).unwrap();
        assert_eq!(read_back.edge_id, 100);
        assert_eq!(read_back.source, 1);
        assert_eq!(read_back.target, 2);
    }

    #[test]
    fn test_token_record_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("index.idx");
        let idx = IndexFile::open(&path).unwrap();

        let rec = TokenIndexRecord::new(5, "hello", 0, 1, 12);
        let (block, chunk) = idx.alloc_record(&{
            let mut buf = [0u8; 64];
            rec.encode(&mut buf);
            buf
        }).unwrap();

        let read_back = idx.read_token_record(block, chunk).unwrap();
        assert_eq!(read_back.token_id, 5);
        assert_eq!(read_back.token_str(), "hello");
    }

    #[test]
    fn test_scan_records() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("index.idx");
        let idx = IndexFile::open(&path).unwrap();

        // Insert three vertex records.
        for vid in 0..3 {
            let rec = VertexIndexRecord::new(vid, 0, 1, 64);
            let mut buf = [0u8; 128];
            rec.encode(&mut buf);
            idx.alloc_record_128(&buf).unwrap();
        }

        let mut count = 0;
        idx.scan(|_block, _chunk, data| {
            if data[0] == ChunkType::Vertex as u8 {
                let buf: &[u8; 128] = data.try_into().unwrap();
                let rec = VertexIndexRecord::decode(buf);
                assert_eq!(rec.vertex_id, count);
                count += 1;
            }
            Ok(())
        }).unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_delete_record() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("index.idx");
        let idx = IndexFile::open(&path).unwrap();

        let rec = VertexIndexRecord::new(1, 0, 1, 64);
        let mut buf = [0u8; 128];
        rec.encode(&mut buf);
        let (block, chunk) = idx.alloc_record_128(&buf).unwrap();

        // Delete it.
        idx.delete_record(block, chunk).unwrap();

        // Should read as empty chunk type.
        let read_back = idx.read_vertex_record(block, chunk).unwrap();
        assert_eq!(read_back.chunk_type, ChunkType::Empty);
    }

    #[test]
    fn test_multiple_blocks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("index.idx");
        let idx = IndexFile::open(&path).unwrap();

        // Insert enough records to force multiple blocks (255 per block).
        let n = 300u32;
        for i in 0..n {
            let rec = VertexIndexRecord::new(i, 0, 1, 64);
            let mut buf = [0u8; 128];
            rec.encode(&mut buf);
            idx.alloc_record_128(&buf).unwrap();
        }

        assert!(idx.block_count() >= 2);

        let mut found = Vec::new();
        idx.scan(|_block, _chunk, data| {
            if data[0] == ChunkType::Vertex as u8 {
                let buf: &[u8; 128] = data.try_into().unwrap();
                let rec = VertexIndexRecord::decode(buf);
                found.push(rec.vertex_id);
            }
            Ok(())
        }).unwrap();

        found.sort();
        assert_eq!(found.len(), n as usize);
        for i in 0..n {
            assert_eq!(found[i as usize], i);
        }
    }
}
