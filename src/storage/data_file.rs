//! Raw 16 KB block I/O on the data/index file.
//!
//! Provides positional read/write of fixed-size blocks and file extension for
//! new block allocation. All operations go through a `Mutex` to allow shared
//! access from the block cache and redo-log checkpoint paths.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Mutex,
};

use crate::storage::types::{BlockIdx, BLOCK_SIZE, StorageResult};

/// A file storing fixed-size 16 KB blocks.
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

    /// Read one full block into a fixed-size buffer.
    ///
    /// If `idx` is beyond the current file length, the block is assumed to be
    /// unallocated and a zero-filled buffer is returned (no error).
    pub fn read_block(&self, idx: BlockIdx) -> StorageResult<[u8; BLOCK_SIZE]> {
        let mut file = self.file.lock().unwrap();
        let offset = (idx as u64) * (BLOCK_SIZE as u64);
        let file_len = file.metadata()?.len();

        if offset >= file_len {
            // Block beyond EOF → not yet allocated → return zeros
            return Ok([0u8; BLOCK_SIZE]);
        }

        file.seek(SeekFrom::Start(offset))?;
        let mut buf = [0u8; BLOCK_SIZE];
        file.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Write one full block.
    ///
    /// If `idx` is beyond the current file length, the file is extended (holes
    /// are filled with zeros up to the write position).
    pub fn write_block(&self, idx: BlockIdx, data: &[u8; BLOCK_SIZE]) -> StorageResult<()> {
        let mut file = self.file.lock().unwrap();
        let offset = (idx as u64) * (BLOCK_SIZE as u64);
        let file_len = file.metadata()?.len();

        // If writing past EOF, extend the file with zeros first.
        if offset > file_len {
            file.seek(SeekFrom::End(0))?;
            let zeros = vec![0u8; (offset - file_len) as usize];
            file.write_all(&zeros)?;
        }

        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)?;
        Ok(())
    }

    /// Allocate `count` new blocks by extending the file.
    ///
    /// Returns the index of the first newly-allocated block.
    pub fn allocate_blocks(&self, count: u32) -> StorageResult<BlockIdx> {
        let mut file = self.file.lock().unwrap();
        let file_len = file.metadata()?.len();
        let blocks_before = (file_len / (BLOCK_SIZE as u64)) as u32;

        // Seek to end and write zeros
        file.seek(SeekFrom::End(0))?;
        let zeros = vec![0u8; (count as usize) * BLOCK_SIZE];
        file.write_all(&zeros)?;
        file.sync_all()?;

        Ok(blocks_before)
    }

    /// Return the number of blocks currently in the file (based on file size).
    pub fn block_count(&self) -> StorageResult<u64> {
        let file = self.file.lock().unwrap();
        let len = file.metadata()?.len();
        Ok(len / (BLOCK_SIZE as u64))
    }

    /// Flush and fsync all buffered data to disk.
    pub fn sync_all(&self) -> StorageResult<()> {
        let file = self.file.lock().unwrap();
        file.sync_all()?;
        Ok(())
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
