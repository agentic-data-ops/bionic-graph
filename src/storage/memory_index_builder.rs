//! Builds the in-memory index (`MemoryIndex`) by scanning the `IndexFile` at
//! graph startup.
//!
//! The scanner visits every non-empty chunk in every index block and
//! populates the four index structures:
//!
//! - `VertexBTree`: vertex_id → (block_idx, chunk_offset)
//! - `EdgeBTree`:   edge_id → (block_idx, chunk_offset)
//! - `TokenMap`:    token string → Vec of (block_idx, chunk_offset)
//! - `RankIndex`:   rank → Vec of (block_idx, chunk_offset)
//! - `AdjacencyIndex`: vertex → outgoing/incoming edges

use crate::storage::index_file::{EdgeIndexRecord, IndexFile, TokenIndexRecord, VertexIndexRecord};
use crate::storage::memory_index::{IndexPointer, MemoryIndex};
use crate::storage::types::{ChunkType, DataStatus, StorageResult};

/// Scan the entire index file and build the in-memory index.
///
/// This is called once during `Graph::open()`. For large graphs it may take
/// several seconds — consider adding progress logging for graphs with
/// >100,000 records.
pub fn build_memory_index(idx_file: &IndexFile) -> StorageResult<MemoryIndex> {
    let mut mem = MemoryIndex::new();

    idx_file.scan(|block_idx, chunk_offset, data| {
        let chunk_type = ChunkType::from(data[0]);
        let ptr = IndexPointer::new(block_idx, chunk_offset);

        match chunk_type {
            ChunkType::Vertex => {
                let buf: &[u8; 128] = data.try_into().map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "vertex record must be 128 bytes"))?;
                let rec = VertexIndexRecord::decode(buf);
                // Keep deleted vertices for time-travel traversal
                mem.vertices.insert(rec.vertex_id, ptr);
                mem.vertex_names.insert(rec.get_name().to_string(), ptr);
                if rec.status != DataStatus::Deleted {
                    mem.ranks.insert(rec.rank, ptr);
                    mem.atime_index.insert(rec.atime, ptr);
                }
            }
            ChunkType::Edge => {
                let buf: &[u8; 128] = data.try_into().map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "edge record must be 128 bytes"))?;
                let rec = EdgeIndexRecord::decode(buf);
                // Always add to adjacency and edges index (even deleted) for time-travel traversal
                mem.adjacency.add_edge(rec.edge_id, rec.source, rec.target, ptr);
                mem.edges.insert(rec.edge_id, ptr);
                mem.edge_names.insert(rec.get_name().to_string(), ptr);
                if rec.status != DataStatus::Deleted {
                    mem.ranks.insert(rec.rank, ptr);
                    mem.atime_index.insert(rec.atime, ptr);
                }
            }
            ChunkType::Token => {
                let buf: &[u8; 64] = data.try_into().map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "token record must be 64 bytes"))?;
                let rec = TokenIndexRecord::decode(buf);
                if rec.status != DataStatus::Deleted {
                    let token_str = rec.token_str().to_string();
                    if !token_str.is_empty() {
                        mem.tokens.insert(token_str, ptr);
                    }
                }
            }
            ChunkType::Empty => {
                // Skip empty chunks.
            }
        }

        Ok(())
    })?;

    Ok(mem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::index_file::VertexIndexRecord;
    use tempfile::tempdir;

    #[test]
    fn test_build_empty_index() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("index.idx");
        let idx = IndexFile::open(&path).unwrap();
        let mem = build_memory_index(&idx).unwrap();
        assert_eq!(mem.vertices.len(), 0);
        assert_eq!(mem.edges.len(), 0);
        assert_eq!(mem.tokens.len(), 0);
    }

    #[test]
    fn test_build_with_vertices() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("index.idx");
        let idx = IndexFile::open(&path).unwrap();

        // Insert some vertex records.
        for vid in 0..5 {
            let rec = VertexIndexRecord::new(vid, 0, 1, 64);
            let mut buf = [0u8; 128];
            rec.encode(&mut buf);
            idx.alloc_record_128(&buf).unwrap();
        }

        let mem = build_memory_index(&idx).unwrap();
        assert_eq!(mem.vertices.len(), 5);
        for vid in 0..5 {
            assert!(mem.vertices.contains(vid));
        }
    }

    #[test]
    fn test_build_skips_deleted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("index.idx");
        let idx = IndexFile::open(&path).unwrap();

        // Insert one record.
        let rec = VertexIndexRecord::new(1, 0, 1, 64);
        let mut buf = [0u8; 128];
        rec.encode(&mut buf);
        let (block, chunk) = idx.alloc_record_128(&buf).unwrap();

        // Mark as deleted in the index record.
        let mut deleted_rec = rec.clone();
        deleted_rec.mark_deleted();
        idx.update_vertex_record(block, chunk, &deleted_rec).unwrap();

        let mem = build_memory_index(&idx).unwrap();
        // Deleted vertices are kept in the vertices index for time-travel support,
        // but excluded from the rank and atime indexes.
        assert_eq!(mem.vertices.len(), 1, "deleted vertex kept for time-travel");
        assert_eq!(mem.ranks.len(), 0, "deleted vertex excluded from rank index");
    }
}
