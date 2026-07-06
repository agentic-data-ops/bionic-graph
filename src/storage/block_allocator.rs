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
    /// Bit positions 0..=255 map to chunks as follows:
    /// - Bit 0 = always-set header bit (chunk 1, never allocated as data)
    /// - Bits 1..=255 = data chunks, where bit N → data offset N
    ///
    /// The search starts from bit 1 (skipping the header bit at position 0).
    pub fn alloc_chunks(bitmap: &mut [u8; 32], count: u8) -> Option<ChunkOffset> {
        if count == 0 || count as usize >= CHUNKS_PER_BLOCK {
            return None;
        }
        let run = count as usize;
        // Start scanning from bit 1 (skip header bit 0).
        let start = 1usize;
        let mut pos = start;
        loop {
            // Find first free bit starting from `pos`
            let free_start = match Self::find_next_free_from(bitmap, pos) {
                Some(s) => s,
                None => return None,
            };
            // Check if we have `run` consecutive free bits
            if Self::has_consecutive_free(bitmap, free_start, run) {
                // Mark them allocated
                for i in free_start..free_start + run {
                    Self::set_bit(bitmap, i, true);
                }
                // Bit position N ↔ data offset N (1-indexed, ≥1)
                return Some(free_start as u8);
            }
            pos = free_start + 1;
            if pos >= CHUNKS_PER_BLOCK {
                return None;
            }
        }
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

    /// Returns `true` when every data chunk (positions 1..255) is allocated.
    pub fn is_block_full(bitmap: &[u8; 32]) -> bool {
        // Check bits 1..255 — we can check bytes 0..31 except bit 0 of byte 0
        for (byte_idx, &byte) in bitmap.iter().enumerate() {
            let expected = if byte_idx == 0 {
                // bit 0 is the header (always set); bits 1-7 should be set
                0xFFu8
            } else {
                0xFFu8
            };
            if byte != expected {
                return false;
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
        if data_len % chunk_size == 0 {
            data_len
        } else {
            ((data_len / chunk_size) + 1) * chunk_size
        }
    }

    /// Number of chunks required to store `data_len` bytes.
    pub fn chunks_needed(data_len: usize) -> u8 {
        let chunks = data_len.div_ceil(64);
        chunks.min(255) as u8 // max 255 data chunks (chunk 0 is header)
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Find the next 0-bit starting from `start_bit` (0-indexed).
    fn find_next_free_from(bitmap: &[u8; 32], start_bit: usize) -> Option<usize> {
        for i in start_bit..CHUNKS_PER_BLOCK {
            if !Self::test_bit(bitmap, i) {
                return Some(i);
            }
        }
        None
    }

    /// Check if `count` consecutive bits starting at `start_bit` are all free.
    fn has_consecutive_free(bitmap: &[u8; 32], start_bit: usize, count: usize) -> bool {
        for i in start_bit..start_bit + count {
            if i >= CHUNKS_PER_BLOCK || Self::test_bit(bitmap, i) {
                return false;
            }
        }
        true
    }

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
        // Bit 1 = first data chunk → data offset 1
        let off = BlockAllocator::alloc_chunks(&mut bm, 1);
        assert_eq!(off, Some(1));
        assert!(BlockAllocator::test_bit(&bm, 1));
    }

    #[test]
    fn test_alloc_frees_and_reallocates() {
        let mut bm = empty_bitmap();
        let off1 = BlockAllocator::alloc_chunks(&mut bm, 2).unwrap();
        assert_eq!(off1, 1); // first free data chunk at offset 1
        BlockAllocator::free_chunks(&mut bm, off1, 2);
        let off2 = BlockAllocator::alloc_chunks(&mut bm, 2).unwrap();
        assert_eq!(off2, 1); // same spot
    }

    #[test]
    fn test_block_full() {
        let mut bm = empty_bitmap();
        // Fill all 255 data chunks: offsets 1..=255
        for expected_off in 1u8..=255u8 {
            let off = BlockAllocator::alloc_chunks(&mut bm, 1).unwrap();
            assert_eq!(off, expected_off);
        }
        assert!(BlockAllocator::is_block_full(&bm));
        assert!(BlockAllocator::alloc_chunks(&mut bm, 1).is_none());
    }

    #[test]
    fn test_block_empty() {
        let bm = empty_bitmap();
        assert!(BlockAllocator::is_block_empty(&bm));
    }

    #[test]
    fn test_chunk_count() {
        let mut bm = empty_bitmap();
        assert_eq!(BlockAllocator::chunk_count(&bm), 0);
        BlockAllocator::alloc_chunks(&mut bm, 3).unwrap();
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
