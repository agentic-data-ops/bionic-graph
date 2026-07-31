//! Chunk-level allocator for a single 16 KB block.
//!
//! Each block is divided into 256 × 64-byte chunks. The first chunk (offset 0)
//! is the block header. Chunks 1..255 are data. The 256-bit bitmap tracks
//! allocation: 1 = allocated, 0 = free. Bit 0 (header) is always set.
//!
//! All functions operate on the raw `[u8; 32]` bitmap array from `BlockHeader`.

use crate::storage::types::{ChunkOffset, CHUNKS_PER_BLOCK};

/// Helpers for chunk-level bitmap manipulation.
pub struct BlockAllocator;

impl BlockAllocator {
    /// Find `count` contiguous free chunks in the bitmap and return the
    /// 1-based data-chunk offset of the first chunk. Returns `None` when
    /// there aren't enough contiguous free slots.
    ///
    /// Two-phase allocation:
    ///   Phase 1 — sequential fast path: try from `*offset` to end-of-block.
    ///   Phase 2 — fallback scan: scan `[1, *offset - count)` for any run.
    ///
    /// On success, allocates the chunks (sets bits) and advances `*offset`
    /// past them (Phase 1 only; Phase 2 leaves offset unchanged so that
    /// future sequential allocations still grow from the tail).
    ///
    /// `offset` starts at 1 for a fresh block and tracks the next expected
    /// free position. All chunks at `offset..` are implicitly free.
    ///
    /// Bit positions 0..=255 map to chunks as follows:
    /// - Bit 0 = always-set header bit (chunk 1, never allocated as data)
    /// - Bits 1..=255 = data chunks, where bit N → data offset N
    pub fn alloc_chunks(bitmap: &mut [u8; 32], offset: &mut u8, count: u8) -> Option<ChunkOffset> {
        if count == 0 || count as usize >= CHUNKS_PER_BLOCK {
            return None;
        }
        let run = count as usize;

        // ── Phase 1: Sequential allocation from offset ───────────────────
        let start = (*offset as usize).max(1);
        if start + run <= CHUNKS_PER_BLOCK {
            // The space after offset is implicitly free (never allocated yet).
            let all_free = (start..start + run).all(|i| !Self::test_bit(bitmap, i));
            if all_free {
                for i in start..start + run {
                    Self::set_bit(bitmap, i, true);
                }
                *offset = (start + run) as u8;
                return Some(start as u8);
            }
        }

        // ── Phase 2: Fallback scan for already-fragmented prefix ─────────
        // Scan from the beginning up to `offset - run`, looking for any
        // contiguous run. This catches space freed by token updates etc.
        let scan_end = start.saturating_sub(run).max(1);
        let mut current_run = 0usize;
        for pos in 1..=scan_end + run - 1 {
            if !Self::test_bit(bitmap, pos) {
                current_run += 1;
                if current_run == run {
                    let found = pos + 1 - run;
                    for i in found..=pos {
                        Self::set_bit(bitmap, i, true);
                    }
                    // Keep offset unchanged — leave cursor at the tail
                    // so future Phase-1 allocations still use the fresh region.
                    return Some(found as u8);
                }
            } else {
                current_run = 0;
            }
        }

        None
    }

    /// Free `count` chunks starting at 1-based `offset`.
    ///
    /// Data offset N ↔ bit position N (bit 0 = header, never freed as data).
    pub fn free_chunks(bitmap: &mut [u8; 32], offset: ChunkOffset, count: u8) {
        let bit_start = offset as usize;
        for i in bit_start..bit_start + count as usize {
            Self::set_bit(bitmap, i, false);
        }
    }

    /// Returns `true` when no 2 consecutive data chunks are free.
    ///
    /// Since every allocation needs at least 2 chunks (header + payload),
    /// a block with no 2-consecutive-free run is effectively full.
    pub fn is_block_full(bitmap: &[u8; 32]) -> bool {
        // Check bits 1..254 for any pair of consecutive zero (free) bits
        for pos in 1..CHUNKS_PER_BLOCK - 1 {
            if !Self::test_bit(bitmap, pos) && !Self::test_bit(bitmap, pos + 1) {
                return false; // Found 2 consecutive free chunks
            }
        }
        true
    }

    /// Returns `true` when no data chunk (positions 1..255) is allocated.
    pub fn is_block_empty(bitmap: &[u8; 32]) -> bool {
        for (byte_idx, &byte) in bitmap.iter().enumerate() {
            let expected = if byte_idx == 0 {
                // Only bit 0 (header) should be set
                0x01u8
            } else {
                0x00u8
            };
            if byte != expected {
                return false;
            }
        }
        true
    }

    /// Count how many 1-bits are set in the data region (bits 1..255).
    pub fn chunk_count(bitmap: &[u8; 32]) -> u8 {
        let mut count = 0u32;
        for (byte_idx, &byte) in bitmap.iter().enumerate() {
            let mask = if byte_idx == 0 {
                // bit 0 is header — don't count it
                0xFEu8
            } else {
                0xFFu8
            };
            count += (byte & mask).count_ones();
        }
        count as u8
    }

    /// Required byte-padding for a payload of `data_len` bytes so it fits
    /// exactly in whole chunks. Returns the padded length.
    pub fn padded_length(data_len: usize) -> usize {
        let chunk_size = 64usize;
        let chunks = data_len.div_ceil(chunk_size).min(255);
        chunks * chunk_size
    }

    /// Number of chunks required to store `data_len` bytes.
    pub fn chunks_needed(data_len: usize) -> u8 {
        let chunks = data_len.div_ceil(64);
        chunks.min(255) as u8 // max 255 data chunks (chunk 0 is header)
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    pub(crate) fn test_bit(bitmap: &[u8; 32], bit: usize) -> bool {
        let byte = bit / 8;
        let bit_in_byte = bit % 8;
        (bitmap[byte] & (1 << bit_in_byte)) != 0
    }

    fn set_bit(bitmap: &mut [u8; 32], bit: usize, value: bool) {
        let byte = bit / 8;
        let bit_in_byte = bit % 8;
        if value {
            bitmap[byte] |= 1 << bit_in_byte;
        } else {
            bitmap[byte] &= !(1 << bit_in_byte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_bitmap() -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0] = 0x01; // bit 0 = header
        b
    }

    #[test]
    fn test_alloc_one_chunk() {
        let mut bm = empty_bitmap();
        let mut offset = 0u8;
        // Bit 1 = first data chunk → data offset 1
        let off = BlockAllocator::alloc_chunks(&mut bm, &mut offset, 1);
        assert_eq!(off, Some(1));
        assert!(BlockAllocator::test_bit(&bm, 1));
    }

    #[test]
    fn test_alloc_frees_and_reallocates() {
        let mut bm = empty_bitmap();
        let mut offset = 0u8;
        let off1 = BlockAllocator::alloc_chunks(&mut bm, &mut offset, 2).unwrap();
        assert_eq!(off1, 1); // first free data chunk at offset 1
        BlockAllocator::free_chunks(&mut bm, off1, 2);
        // The allocation cursor advanced past the freed region; a fresh
        // allocation continues from the cursor (3) instead of reusing [1,2].
        let off2 = BlockAllocator::alloc_chunks(&mut bm, &mut offset, 2).unwrap();
        assert_eq!(off2, 3);
        // Freed chunks are still reusable via the fallback scan with a
        // reset cursor.
        let mut offset2 = 0u8;
        let off3 = BlockAllocator::alloc_chunks(&mut bm, &mut offset2, 2).unwrap();
        assert_eq!(off3, 1);
    }

    #[test]
    fn test_block_full() {
        let mut bm = empty_bitmap();
        let mut offset = 0u8;
        // Fill all 255 data chunks: offsets 1..=255
        for expected_off in 1u8..=255u8 {
            let off = BlockAllocator::alloc_chunks(&mut bm, &mut offset, 1).unwrap();
            assert_eq!(off, expected_off);
        }
        assert!(BlockAllocator::is_block_full(&bm));
        assert!(BlockAllocator::alloc_chunks(&mut bm, &mut offset, 1).is_none());
    }

    #[test]
    fn test_block_empty() {
        let bm = empty_bitmap();
        assert!(BlockAllocator::is_block_empty(&bm));
    }

    #[test]
    fn test_chunk_count() {
        let mut bm = empty_bitmap();
        let mut offset = 0u8;
        assert_eq!(BlockAllocator::chunk_count(&bm), 0);
        BlockAllocator::alloc_chunks(&mut bm, &mut offset, 3).unwrap();
        assert_eq!(BlockAllocator::chunk_count(&bm), 3);
    }

    #[test]
    fn test_padded_length() {
        assert_eq!(BlockAllocator::padded_length(64), 64);
        assert_eq!(BlockAllocator::padded_length(65), 128);
        assert_eq!(BlockAllocator::padded_length(1), 64);
    }

    #[test]
    fn test_chunks_needed() {
        assert_eq!(BlockAllocator::chunks_needed(1), 1);
        assert_eq!(BlockAllocator::chunks_needed(64), 1);
        assert_eq!(BlockAllocator::chunks_needed(65), 2);
        assert_eq!(BlockAllocator::chunks_needed(128), 2);
    }
}
