//! Block-level allocation bitmap for the data file.
//!
//! Each bit in the bitmap file represents one 16 KB block in the data file:
//! - `0` → block has free chunks (available for allocation)
//! - `1` → block is full (no free chunks)
//!
//! The bitmap is fully loaded into memory on startup and synced to disk
//! immediately on every change. A sorted `free_blocks` list (containing up to
//! 128 block indices with free space) is maintained for O(1) allocation.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Mutex,
};

use crate::storage::types::{BlockIdx, StorageResult};

/// Number of free-block slots to keep pre-filled in memory.
const FREE_LIST_TARGET: usize = 128;

/// Block-level bitmap manager.
///
/// # Invariants
///
/// - `free_blocks` is always sorted ascending.
/// - `free_blocks.len()` ≤ `FREE_LIST_TARGET`.
/// - The on-disk bitmap is always in sync with the in-memory `bitmap` vec.
pub struct BitmapFile {
    file: Mutex<File>,
    #[allow(dead_code)]
    path: std::path::PathBuf,
    bitmap: Vec<u8>,
    /// Sorted list of block indices that have at least one free chunk.
    free_blocks: Vec<BlockIdx>,
    /// Position in the bitmap where the last 0-bit was found during scan.
    last_scan_pos: usize,
}

impl BitmapFile {
    /// Open (or create) the bitmap file and scan for free blocks.
    ///
    /// `initial_data_blocks` is the current number of 16 KB blocks in the
    /// associated data file. The bitmap is sized to cover that many blocks
    /// (rounded up to the nearest byte).
    pub fn open<P: AsRef<Path>>(path: P, initial_data_blocks: u64) -> StorageResult<Self> {
        let path = path.as_ref().to_path_buf();
        let bitmap_len = (initial_data_blocks as usize).div_ceil(8);
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)?;

        // Determine target bitmap length (at least 1 byte for empty graphs).
        let target_len = bitmap_len.max(1);
        let file_len = file.metadata()?.len() as usize;

        let bitmap = if file_len >= target_len {
            // Read existing bitmap from disk.
            let mut buf = vec![0u8; file_len];
            file.seek(SeekFrom::Start(0))?;
            file.read_exact(&mut buf)?;
            buf
        } else {
            // Create a fresh zero-filled bitmap.
            let buf = vec![0u8; target_len];
            file.set_len(target_len as u64)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&buf)?;
            file.sync_all()?;
            buf
        };

        // Scan for free blocks (0 bits).
        let mut free_blocks = Vec::new();
        for block_idx in 0..initial_data_blocks as u32 {
            if !Self::is_bit_set(&bitmap, block_idx) {
                free_blocks.push(block_idx);
                if free_blocks.len() >= FREE_LIST_TARGET {
                    break;
                }
            }
        }

        // If not enough free blocks found, extend.
        let last_scan_pos = free_blocks.last().copied().unwrap_or(0) as usize;

        Ok(Self {
            file: Mutex::new(file),
            path,
            bitmap,
            free_blocks,
            last_scan_pos,
        })
    }

    /// Allocate one block — returns a block index with free space.
    ///
    /// If the free list is empty, extends the data file (via the callback) and
    /// adds new blocks to the free list.
    pub fn alloc_block<F>(&mut self, allocate_new: F) -> StorageResult<BlockIdx>
    where
        F: FnOnce(u32) -> StorageResult<BlockIdx>,
    {
        if !self.free_blocks.is_empty() {
            return Ok(self.free_blocks.remove(0));
        }

        // No free blocks — allocate a batch of new blocks from the data file.
        let count = FREE_LIST_TARGET as u32;
        let start = allocate_new(count)?;

        // Extend bitmap if needed.
        let needed_bytes = ((start + count) as usize).div_ceil(8);
        if needed_bytes > self.bitmap.len() {
            self.bitmap.resize(needed_bytes, 0u8);
            self.sync_bitmap()?;
        }

        // Add new blocks to free list (bits are already 0 in a fresh bitmap).
        for i in start..start + count {
            self.free_blocks.push(i);
        }
        self.last_scan_pos = (start + count) as usize;

        Ok(self.free_blocks.remove(0))
    }

    /// Mark a block as full (all chunks used).
    pub fn mark_full(&mut self, idx: BlockIdx) -> StorageResult<()> {
        Self::set_bit(&mut self.bitmap, idx, true);
        self.free_blocks.retain(|&b| b != idx);
        self.sync_bitmap()?;
        // Replenish free list by scanning forward.
        self.scan_for_free_blocks();
        Ok(())
    }

    /// Mark a block as having free space again (e.g. after cleanup).
    pub fn mark_free(&mut self, idx: BlockIdx) -> StorageResult<()> {
        if !Self::is_bit_set(&self.bitmap, idx) {
            return Ok(()); // already free
        }
        Self::set_bit(&mut self.bitmap, idx, false);
        // Insert into free_blocks maintaining sorted order.
        if let Err(pos) = self.free_blocks.binary_search(&idx) {
            self.free_blocks.insert(pos, idx);
        }
        // Trim if we have too many.
        while self.free_blocks.len() > FREE_LIST_TARGET {
            self.free_blocks.pop();
        }
        self.sync_bitmap()?;
        Ok(())
    }

    /// Return the current count of free blocks tracked.
    pub fn free_block_count(&self) -> usize {
        self.free_blocks.len()
    }

    /// Flush the in-memory bitmap to disk.
    fn sync_bitmap(&self) -> StorageResult<()> {
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&self.bitmap)?;
        file.sync_all()?;
        Ok(())
    }

    /// Scan from `last_scan_pos` forward, collecting 0-bits into free_blocks
    /// until we have `FREE_LIST_TARGET` entries.
    fn scan_for_free_blocks(&mut self) {
        let start = self.last_scan_pos;
        let total_blocks = self.bitmap.len() * 8;

        for block_idx in start..total_blocks {
            if self.free_blocks.len() >= FREE_LIST_TARGET {
                break;
            }
            if !Self::is_bit_set(&self.bitmap, block_idx as u32) {
                self.free_blocks.push(block_idx as u32);
                self.last_scan_pos = block_idx;
            }
        }
    }

    // ── Bit helpers ─────────────────────────────────────────────────────────

    fn is_bit_set(bitmap: &[u8], idx: u32) -> bool {
        let byte = idx as usize / 8;
        let bit = idx as usize % 8;
        byte < bitmap.len() && (bitmap[byte] & (1 << bit)) != 0
    }

    fn set_bit(bitmap: &mut [u8], idx: u32, value: bool) {
        let byte = idx as usize / 8;
        let bit = idx as usize % 8;
        if byte < bitmap.len() {
            if value {
                bitmap[byte] |= 1 << bit;
            } else {
                bitmap[byte] &= !(1 << bit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_alloc() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bitmap");

        let mut bf = BitmapFile::open(&path, 0).unwrap();
        assert_eq!(bf.free_block_count(), 0);

        // Allocate — should trigger extension.
        let idx = bf
            .alloc_block(|_count| {
                // Pretend we allocated blocks 0..count
                Ok(0)
            })
            .unwrap();
        assert_eq!(idx, 0);
    }

    #[test]
    fn test_mark_full_then_free() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bitmap");

        let mut bf = BitmapFile::open(&path, 10).unwrap();
        // Initially all 10 blocks are free.
        assert!(bf.free_block_count() > 0);

        bf.mark_full(3).unwrap();
        let full_list: Vec<_> = bf.free_blocks.iter().copied().collect();
        assert!(!full_list.contains(&3));

        bf.mark_free(3).unwrap();
        let free_list: Vec<_> = bf.free_blocks.iter().copied().collect();
        assert!(free_list.contains(&3));
    }
}
