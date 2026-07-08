//! Raw 16 KB block I/O on the data/index file with O_DIRECT.
//!
//! All reads and writes use 512-byte aligned buffers internally and bypass
//! the OS page cache. The public API preserves the `[u8; BLOCK_SIZE]` type
//! for backward compatibility — alignment conversion happens internally.
//! Data durability is provided by the WAL checkpoint mechanism instead of
//! per-operation fsync.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::Mutex,
};

use crate::storage::types::{AlignedBlock, BlockIdx, StorageResult, BLOCK_SIZE};

/// A file storing fixed-size 16 KB blocks, opened with O_DIRECT.
///
/// # Layout
///
/// Block `N` is at byte offset `N × BLOCK_SIZE` in the file. The file grows
/// as blocks are appended; it is never shrunk (free blocks are tracked by the
/// `BitmapFile`, not by truncation).
pub struct DataFile {
    file: Mutex<File>,
    path: std::path::PathBuf,
}

impl DataFile {
    /// Open an existing data file at `path`, or create a new empty one.
    pub fn open<P: AsRef<Path>>(path: P) -> StorageResult<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .custom_flags(libc::O_DIRECT)
            .open(path.as_ref())?;
        let path = path.as_ref().to_path_buf();
        Ok(Self {
            file: Mutex::new(file),
            path,
        })
    }

    /// Return the file path (useful for diagnostics).
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Read one full block (backward-compatible wrapper).
    ///
    /// If `idx` is beyond EOF, returns zeros.
    pub fn read_block(&self, idx: BlockIdx) -> StorageResult<[u8; BLOCK_SIZE]> {
        self.read_block_aligned(idx).map(|a| a.0)
    }

    /// Read into a pre-allocated aligned buffer.
    fn read_block_aligned(&self, idx: BlockIdx) -> StorageResult<AlignedBlock> {
        let mut file = self.file.lock().unwrap();
        let offset = (idx as u64) * (BLOCK_SIZE as u64);
        let file_len = file.metadata()?.len();

        if offset >= file_len {
            return Ok(AlignedBlock::new());
        }

        file.seek(SeekFrom::Start(offset))?;
        let mut block = AlignedBlock::new();
        file.read_exact(&mut block.0)?;
        Ok(block)
    }

    /// Write one full block (backward-compatible wrapper).
    ///
    /// If `idx` is beyond EOF, the file is extended.
    pub fn write_block(&self, idx: BlockIdx, data: &[u8; BLOCK_SIZE]) -> StorageResult<()> {
        let mut aligned = AlignedBlock::new();
        aligned.0.copy_from_slice(data);
        self.write_block_aligned(idx, &aligned)
    }

    /// Write from a pre-aligned buffer.
    fn write_block_aligned(&self, idx: BlockIdx, data: &AlignedBlock) -> StorageResult<()> {
        let mut file = self.file.lock().unwrap();
        let offset = (idx as u64) * (BLOCK_SIZE as u64);
        let file_len = file.metadata()?.len();

        if offset > file_len {
            file.seek(SeekFrom::End(0))?;
            let extend = (offset - file_len) as usize;
            if extend > 0 {
                let zero_block = AlignedBlock::new();
                for _ in (0..extend).step_by(BLOCK_SIZE) {
                    file.write_all(&zero_block.0)?;
                }
            }
        }

        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&data.0)?;
        Ok(())
    }

    /// Allocate `count` new blocks by extending the file.
    pub fn allocate_blocks(&self, count: u32) -> StorageResult<BlockIdx> {
        let mut file = self.file.lock().unwrap();
        let file_len = file.metadata()?.len();
        let blocks_before = (file_len / (BLOCK_SIZE as u64)) as u32;

        file.seek(SeekFrom::End(0))?;
        let zero_block = AlignedBlock::new();
        for _ in 0..count {
            file.write_all(&zero_block.0)?;
        }

        Ok(blocks_before)
    }

    /// Return the number of blocks currently in the file (based on file size).
    pub fn block_count(&self) -> StorageResult<u64> {
        let file = self.file.lock().unwrap();
        let len = file.metadata()?.len();
        Ok(len / (BLOCK_SIZE as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_write_then_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.data");
        let df = DataFile::open(&path).unwrap();

        let mut block = [0u8; BLOCK_SIZE];
        block[0..4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

        df.write_block(0, &block).unwrap();
        let read_back = df.read_block(0).unwrap();
        assert_eq!(&read_back[0..4], &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_read_unallocated_block_returns_zeros() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.data");
        let df = DataFile::open(&path).unwrap();

        let buf = df.read_block(999).unwrap();
        assert_eq!(&buf[..], &[0u8; BLOCK_SIZE]);
    }

    #[test]
    fn test_allocate_blocks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.data");
        let df = DataFile::open(&path).unwrap();

        assert_eq!(df.block_count().unwrap(), 0);
        let start = df.allocate_blocks(5).unwrap();
        assert_eq!(start, 0);
        assert_eq!(df.block_count().unwrap(), 5);

        let start2 = df.allocate_blocks(3).unwrap();
        assert_eq!(start2, 5);
        assert_eq!(df.block_count().unwrap(), 8);
    }
}
