//! Builds the in-memory index (`MemoryIndex`) by scanning the data file at
//! graph startup.
//!
//! The scanner visits every occupied chunk in every data block via the
//! block header's bitmap, reads the `DataHeader` (first 64 bytes of each
//! record), deserializes the payload, and populates all index structures.

use crate::graph::serialize::{deserialize_edge, deserialize_token, deserialize_vertex};
use crate::storage::block_allocator::BlockAllocator;
use crate::storage::memory_index::{MetaPointer, MemoryIndex};
use crate::storage::data_file::DataFile;
use crate::storage::types::{BlockHeader, ChunkType, DataHeader, DataStatus, StorageResult, BLOCK_SIZE, DATA_HEADER_SIZE};

/// Scan the entire data file and build the in-memory index.
///
/// Called once during `Graph::open()`. For large graphs this may take
/// several seconds.
pub fn build_memory_index(data_file: &DataFile) -> StorageResult<MemoryIndex> {
    let mut mem = MemoryIndex::new();
    let block_count = data_file.block_count()?;

    for block_idx in 0..block_count {
        let block = data_file.read_block(block_idx as u32)?;
        let header = BlockHeader::decode(&block);

        let mut co: u16 = 1; // chunk offset starts at 1 (skip header chunk 0)
        while co < 256 {
            if !BlockAllocator::test_bit(&header.bitmap, co as usize) {
                co += 1;
                continue;
            }

            // Read the first byte to check if this is a record start.
            let chunk_start = (co as usize) * 64;
            let first_byte = block[chunk_start];
            let chunk_type = ChunkType::from(first_byte);

            match chunk_type {
                ChunkType::Vertex | ChunkType::Edge | ChunkType::Token => {
                    // Decode the DataHeader from the first 64 bytes.
                    let mut dh_buf = [0u8; 64];
                    dh_buf.copy_from_slice(&block[chunk_start..chunk_start + 64]);
                    let dh = DataHeader::decode(&dh_buf);

                    let total_len = DATA_HEADER_SIZE + dh.payload_len as usize;
                    let total_chunks = BlockAllocator::chunks_needed(total_len);
                    let ptr = MetaPointer::new(block_idx as u32, co as u8);

                    // Read the full record bytes from the block.
                    let record_end = (co as usize + total_chunks as usize) * 64;
                    let available = block.len().min(record_end);
                    let mut record_bytes = vec![0u8; total_len];
                    let copy_len = (available - chunk_start).min(total_len);
                    record_bytes[..copy_len].copy_from_slice(&block[chunk_start..chunk_start + copy_len]);

                    let payload_bytes = if total_len > DATA_HEADER_SIZE {
                        &record_bytes[DATA_HEADER_SIZE..]
                    } else {
                        &[]
                    };

                    match chunk_type {
                        ChunkType::Vertex => {
                            let payload = deserialize_vertex(payload_bytes)?;
                            mem.vertices.insert(dh.entity_id, ptr);
                            mem.vertex_names.insert(payload.name.clone(), dh.entity_id);
                            if dh.status != DataStatus::Deleted {
                                mem.ranks.insert(dh.rank, ptr);
                                mem.atime_index.insert(dh.atime, ptr);
                            }
                        }
                        ChunkType::Edge => {
                            let payload = deserialize_edge(payload_bytes)?;
                            mem.edges.insert(dh.entity_id, ptr);
                            mem.edge_names.insert(payload.name.clone(), dh.entity_id);
                            mem.adjacency.add_edge(dh.entity_id, payload.source, payload.target, ptr);
                            if dh.status != DataStatus::Deleted {
                                mem.ranks.insert(dh.rank, ptr);
                                mem.atime_index.insert(dh.atime, ptr);
                            }
                        }
                        ChunkType::Token => {
                            let payload = deserialize_token(payload_bytes)?;
                            let token_str = &payload.token;
                            if !token_str.is_empty() {
                                mem.tokens.insert(token_str.clone(), ptr);
                                // Build reverse index (entity_tokens) from token refs
                                for tref in &payload.refs {
                                    mem.add_entity_token(tref.ref_type, tref.ref_id, token_str);
                                }
                            }
                        }
                        _ => {} // unreachable
                    }

                    co += total_chunks as u16;
                }
                ChunkType::Empty => {
                    co += 1;
                }
            }
        }
    }

    Ok(mem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::serialize::serialize_vertex;
    use crate::storage::types::{DataHeader, VertexPayload, DATA_HEADER_SIZE};
    use tempfile::tempdir;

    fn write_vertex_data(df: &DataFile, vid: u32, name: &str) -> StorageResult<MetaPointer> {
        let payload = VertexPayload {
            id: vid,
            name: name.to_string(),
            labels: vec![],
            keywords: vec![],
            properties: std::collections::HashMap::new(),
            history: vec![],
        };
        let serialized = serialize_vertex(&payload)?;
        let total_len = DATA_HEADER_SIZE + serialized.len();
        let total_chunks = BlockAllocator::chunks_needed(total_len);
        let padded_total = BlockAllocator::padded_length(total_len);

        let dh = DataHeader::new_vertex(vid, serialized.len() as u16);
        let mut dh_buf = [0u8; 64];
        dh.encode(&mut dh_buf);

        let mut write_buf = vec![0u8; padded_total];
        write_buf[..64].copy_from_slice(&dh_buf);
        write_buf[64..64 + serialized.len()].copy_from_slice(&serialized);

        let block_count = df.block_count()? as u32;
        if block_count == 0 {
            df.allocate_blocks(1)?;
        }
        let mut block = df.read_block(0)?;
        let mut header = BlockHeader::decode(&block);

        // Manually find free chunks (simplified for test)
        let offset = BlockAllocator::alloc_chunks(&mut header.bitmap, total_chunks)
            .ok_or_else(|| crate::storage::types::StorageError::Other("no free chunks".into()))?;
        header.encode(&mut block);

        let start = (offset as usize) * 64;
        block[start..start + padded_total].copy_from_slice(&write_buf);
        df.write_block(0, &block)?;

        Ok(MetaPointer::new(0, offset))
    }

    #[test]
    fn test_build_empty_data_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("data");
        let df = DataFile::open(&path).unwrap();
        let mem = build_memory_index(&df).unwrap();
        assert_eq!(mem.vertices.len(), 0);
        assert_eq!(mem.edges.len(), 0);
        assert_eq!(mem.tokens.len(), 0);
    }

    #[test]
    fn test_build_with_vertices() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("data");
        let df = DataFile::open(&path).unwrap();

        for vid in 0..3 {
            write_vertex_data(&df, vid, &format!("vertex-{}", vid)).unwrap();
        }

        let mem = build_memory_index(&df).unwrap();
        assert_eq!(mem.vertices.len(), 3);
        for vid in 0..3 {
            assert!(mem.vertices.contains(vid));
            assert!(mem.vertex_names.contains_key(&format!("vertex-{}", vid)));
            assert_eq!(
                mem.vertex_names[&format!("vertex-{}", vid)],
                vid
            );
        }
    }
}
