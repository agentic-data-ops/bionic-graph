use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::graph::vertex::{VersionRecord, VertexId, now_micros};

/// Magic bytes for version log files: "BGVL"
pub const VLOG_MAGIC: [u8; 4] = [0x42, 0x47, 0x56, 0x4C];
pub const VLOG_VERSION: u32 = 2;

/// Default index interval — record an index entry every N data entries.
pub const DEFAULT_INDEX_INTERVAL: u32 = 64;

/// A single entry in the version log.
#[derive(Debug, Clone)]
pub struct VlogEntry {
    pub vertex_id: VertexId,
    pub version: u64,
    pub record: VersionRecord,
}

/// A sparse index entry pointing to a location in the vlog file.
#[derive(Debug, Clone, Copy)]
pub struct VlogIndexEntry {
    /// 0-based entry index in the data section.
    pub entry_idx: u32,
    /// Byte offset from the start of the file to this entry.
    pub file_offset: u64,
    /// The vertex_id of the first entry at this location.
    pub first_vertex_id: VertexId,
}

/// Statistics from a version log write operation.
#[derive(Debug, Default, Clone)]
pub struct VlogStats {
    pub entries_written: usize,
    pub file_size_bytes: u64,
    pub files_created: usize,
    pub index_entries: usize,
}

// ─── Write ───────────────────────────────────────────────────────

/// Write a batch of version records into a new version log file.
///
/// File naming: `{data_dir}/version_log/{timestamp_us}_{sequence:04}.vlog`
///
/// Format:
///   HEADER: Magic(4) + Version(4) + Count(4) + IndexInterval(4) + IndexCount(4)
///   INDEX:  [IndexEntry × IndexCount]  — each: entry_idx(4) + offset(8) + vertex_id(8)
///   ENTRIES: [Entry × Count] — each: vertex_id(8) + version(8) + payload_len(4) + payload
pub fn write_vlog(
    data_dir: &Path,
    entries: &[VlogEntry],
    sequence: u32,
) -> std::io::Result<VlogStats> {
    if entries.is_empty() {
        return Ok(VlogStats::default());
    }

    let log_dir = data_dir.join("version_log");
    std::fs::create_dir_all(&log_dir)?;

    let timestamp = now_micros();
    let filename = format!("{:020}_{:04}.vlog", timestamp, sequence);
    let path = log_dir.join(&filename);

    // Phase 1: build entries buffer + sparse index
    let interval = DEFAULT_INDEX_INTERVAL;
    let mut payload_buf = Vec::new();
    let mut sparse_index: Vec<VlogIndexEntry> = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        // Record index entry at every `interval` boundary
        if i % interval as usize == 0 {
            sparse_index.push(VlogIndexEntry {
                entry_idx: i as u32,
                file_offset: 0, // will be filled after we know header size
                first_vertex_id: entry.vertex_id,
            });
        }

        let record_bytes = bincode::serialize(&entry.record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        payload_buf.extend_from_slice(&entry.vertex_id.to_le_bytes());
        payload_buf.extend_from_slice(&entry.version.to_le_bytes());
        payload_buf.extend_from_slice(&(record_bytes.len() as u32).to_le_bytes());
        payload_buf.extend_from_slice(&record_bytes);
    }

    // Calculate header size to fix up file offsets in the index
    // Header: 4 + 4 + 4 + 4 + 4 = 20 bytes
    // Index section: sparse_index.len() * (4 + 8 + 8) bytes
    let header_size = 4 + 4 + 4 + 4 + 4; // magic, version, count, interval, index_count
    let index_size = sparse_index.len() * (4 + 8 + 8);
    let data_offset = (header_size + index_size) as u64;

    // Fix up file offsets in the index
    // Pre-collect end boundaries for each index entry
    let boundaries: Vec<usize> = sparse_index
        .iter()
        .enumerate()
        .map(|(i, _e)| {
            if i + 1 < sparse_index.len() {
                sparse_index[i + 1].entry_idx as usize
            } else {
                entries.len()
            }
        })
        .collect();

    let mut running_offset = data_offset;
    for (i, idx_entry) in sparse_index.iter_mut().enumerate() {
        idx_entry.file_offset = running_offset;
        let end_entry = boundaries[i];
        // Advance running_offset by the serialized size of entries [start_entry..end_entry)
        let start_entry = idx_entry.entry_idx as usize;
        for e_idx in start_entry..end_entry {
            let est_entry_size = 8 + 8 + 4 + estimate_record_size(&entries[e_idx].record);
            running_offset += est_entry_size as u64;
        }
    }

    let entry_count = entries.len() as u32;
    let index_count = sparse_index.len() as u32;

    // Phase 2: write file
    let mut file = std::fs::File::create(&path)?;

    // Header
    file.write_all(&VLOG_MAGIC)?;
    file.write_all(&VLOG_VERSION.to_le_bytes())?;
    file.write_all(&entry_count.to_le_bytes())?;
    file.write_all(&interval.to_le_bytes())?;
    file.write_all(&index_count.to_le_bytes())?;

    // Index section
    for idx_entry in &sparse_index {
        file.write_all(&idx_entry.entry_idx.to_le_bytes())?;
        file.write_all(&idx_entry.file_offset.to_le_bytes())?;
        file.write_all(&idx_entry.first_vertex_id.to_le_bytes())?;
    }

    // Data entries
    file.write_all(&payload_buf)?;
    file.sync_all()?;

    let file_size = std::fs::metadata(&path)?.len();

    Ok(VlogStats {
        entries_written: entries.len(),
        file_size_bytes: file_size,
        files_created: 1,
        index_entries: sparse_index.len(),
    })
}

fn estimate_record_size(record: &VersionRecord) -> usize {
    // Rough estimate: version(8) + updated_at(8) + labels len(8) + labels content
    // + properties map overhead
    8 + 8 + 8 + record.labels.iter().map(|l| l.len() + 8).sum::<usize>()
        + 32 + record.properties.len() * 64
}

// ─── Read ────────────────────────────────────────────────────────

/// Read all entries from a version log file.
pub fn read_vlog(path: &Path) -> std::io::Result<Vec<VlogEntry>> {
    let (entries, _index) = read_vlog_with_index(path)?;
    Ok(entries)
}

/// Read entries + sparse index from a version log file.
pub fn read_vlog_with_index(path: &Path) -> std::io::Result<(Vec<VlogEntry>, Vec<VlogIndexEntry>)> {
    let mut data = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut data)?;

    let total_len = data.len();

    let mut off = 0;
    if total_len < 20 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
            "File too small for v2 header"));
    }

    // Magic
    if &data[off..off + 4] != VLOG_MAGIC {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Bad magic"));
    }
    off += 4;

    // Version
    let version = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
    off += 4;

    // For v1 (no index), handle differently
    if version == 1 {
        return read_vlog_v1(&data[off..]);
    }
    if version != 2 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
            format!("Unsupported vlog version: {}", version)));
    }

    // v2 header: count(4) + interval(4) + index_count(4)
    let entry_count = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
    off += 4;
    let _interval = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
    off += 4;
    let index_count = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
    off += 4;

    // Read sparse index
    let mut sparse_index = Vec::with_capacity(index_count as usize);
    for _ in 0..index_count {
        if off + 20 > total_len {
            break;
        }
        let entry_idx = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
        off += 4;
        let file_offset = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        let first_vertex_id = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        sparse_index.push(VlogIndexEntry { entry_idx, file_offset, first_vertex_id });
    }

    // Read data entries
    let mut entries = Vec::with_capacity(entry_count as usize);
    while off + 20 <= total_len {
        let vertex_id = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        let version_val = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        let payload_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;

        if off + payload_len > total_len {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Truncated payload"));
        }

        let record: VersionRecord = bincode::deserialize(&data[off..off + payload_len])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        off += payload_len;

        entries.push(VlogEntry { vertex_id, version: version_val, record });
    }

    Ok((entries, sparse_index))
}

/// Fallback reader for v1 format (no index).
fn read_vlog_v1(data: &[u8]) -> std::io::Result<(Vec<VlogEntry>, Vec<VlogIndexEntry>)> {
    let total_len = data.len();
    let mut off = 0;

    if total_len < 4 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "v1 file too small"));
    }
    let entry_count = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
    off += 4;

    let mut entries = Vec::with_capacity(entry_count as usize);
    while off + 20 <= total_len {
        let vertex_id = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        let version_val = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        let payload_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;

        if off + payload_len > total_len {
            break;
        }
        let record: VersionRecord = bincode::deserialize(&data[off..off + payload_len])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        off += payload_len;

        entries.push(VlogEntry { vertex_id, version: version_val, record });
    }

    Ok((entries, Vec::new()))
}

// ─── Sparse Index Lookup ─────────────────────────────────────────

/// Look up entries for a specific vertex_id using the sparse index.
/// Returns entries whose vertex_id matches, or empty vec if not found.
///
/// Algorithm:
/// 1. Binary search the sparse index for the closest entry ≤ vertex_id
/// 2. Linear scan from that offset, up to `interval` entries
pub fn lookup_vertex_in_vlog(
    path: &Path,
    target_vertex_id: VertexId,
) -> std::io::Result<Vec<VlogEntry>> {
    let (_, index) = read_vlog_with_index(path)?;
    if index.is_empty() {
        // No index — full scan
        return Ok(read_vlog(path)?.into_iter()
            .filter(|e| e.vertex_id == target_vertex_id)
            .collect());
    }

    // Binary search: find the rightmost index entry with first_vertex_id ≤ target
    let idx_pos = match index.binary_search_by_key(&target_vertex_id, |e| e.first_vertex_id) {
        Ok(pos) => pos,
        Err(pos) => {
            if pos == 0 {
                return Ok(Vec::new()); // target is before all indexed vertices
            }
            pos - 1 // closest index entry before target
        }
    };

    let idx_entry = &index[idx_pos];
    let next_offset = if idx_pos + 1 < index.len() {
        index[idx_pos + 1].file_offset
    } else {
        // Read to end of file
        let metadata = std::fs::metadata(path)?;
        metadata.len()
    };

    // Read only the byte range containing the candidate entries
    let range_start = idx_entry.file_offset as usize;
    let range_len = (next_offset - idx_entry.file_offset) as usize;

    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; range_len];
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(range_start as u64))?;
    file.read_exact(&mut buf)?;

    // Parse entries in this range and filter by vertex_id
    let mut results = Vec::new();
    let mut off = 0;
    while off + 20 <= buf.len() {
        let vertex_id = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap());
        off += 8;
        let version_val = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap());
        off += 8;
        let payload_len = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
        off += 4;

        if off + payload_len > buf.len() {
            break;
        }

        if vertex_id == target_vertex_id {
            let record: VersionRecord = bincode::deserialize(&buf[off..off + payload_len])
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            results.push(VlogEntry { vertex_id, version: version_val, record });
        }
        off += payload_len;
    }

    Ok(results)
}

// ─── File Management ─────────────────────────────────────────────

/// List all version log files in the data directory (sorted by name).
pub fn list_vlog_files(data_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let log_dir = data_dir.join("version_log");
    if !log_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&log_dir)?
        .flatten()
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "vlog"))
        .map(|e| e.path())
        .collect();
    files.sort();
    Ok(files)
}

/// Delete old version log files, keeping the latest `keep_latest`.
pub fn delete_vlog_files(data_dir: &Path, keep_latest: usize) -> std::io::Result<usize> {
    let files = list_vlog_files(data_dir)?;
    if files.len() <= keep_latest {
        return Ok(0);
    }
    let mut deleted = 0;
    for f in &files[..files.len() - keep_latest] {
        if std::fs::remove_file(f).is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn make_record(version: u64) -> VersionRecord {
        VersionRecord {
            version,
            updated_at: (version as i64) * 1000,
            name: "test".to_string(),
            keywords: vec![],
            document: "".to_string(),
            labels: vec!["test".to_string()],
            properties: HashMap::new(),
        }
    }

    #[test]
    fn test_write_and_read_vlog_v2() {
        let dir = tempdir().unwrap();
        let entries: Vec<VlogEntry> = (0..200).map(|i| {
            VlogEntry {
                vertex_id: (i % 10) as u64 + 1,
                version: (i / 10) as u64 + 1,
                record: make_record((i / 10) as u64 + 1),
            }
        }).collect();

        let stats = write_vlog(dir.path(), &entries, 1).unwrap();
        assert_eq!(stats.entries_written, 200);
        assert!(stats.index_entries > 0);
        assert!(stats.file_size_bytes > 0);

        // Read back
        let (loaded, index) = read_vlog_with_index(&dir.path().join("version_log").read_dir().unwrap()
            .next().unwrap().unwrap().path()).unwrap();
        assert_eq!(loaded.len(), 200);
        assert!(!index.is_empty(), "Should have sparse index");
    }

    #[test]
    fn test_lookup_by_vertex_id() {
        let dir = tempdir().unwrap();
        let mut entries = Vec::new();
        // vertex 1 has 5 versions, vertex 2 has 3, vertex 3 has 2
        for v in 1..=3u64 {
            for ver in 1..=(6 - v) {
                entries.push(VlogEntry {
                    vertex_id: v,
                    version: ver,
                    record: make_record(ver),
                });
            }
        }
        let _stats = write_vlog(dir.path(), &entries, 1).unwrap();
        let files = list_vlog_files(dir.path()).unwrap();
        assert_eq!(files.len(), 1);

        // Lookup vertex 2
        let results = lookup_vertex_in_vlog(&files[0], 2).unwrap();
        assert_eq!(results.len(), 4, "vertex 2 should have 4 versions");

        // All returned entries should be vertex 2
        assert!(results.iter().all(|e| e.vertex_id == 2));
    }

    #[test]
    fn test_v1_backward_compat() {
        // Write a v1-format file (no index) and verify we can still read it
        let dir = tempdir().unwrap();
        let log_dir = dir.path().join("version_log");
        std::fs::create_dir_all(&log_dir).unwrap();

        let path = log_dir.join("v1_test.vlog");
        let entry = VlogEntry {
            vertex_id: 99,
            version: 1,
            record: make_record(1),
        };
        let record_bytes = bincode::serialize(&entry.record).unwrap();

        // Manually write v1 format: magic + version(1) + count + entries
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&VLOG_MAGIC).unwrap();
        f.write_all(&1u32.to_le_bytes()).unwrap(); // version = 1
        f.write_all(&1u32.to_le_bytes()).unwrap(); // count = 1
        f.write_all(&entry.vertex_id.to_le_bytes()).unwrap();
        f.write_all(&entry.version.to_le_bytes()).unwrap();
        f.write_all(&(record_bytes.len() as u32).to_le_bytes()).unwrap();
        f.write_all(&record_bytes).unwrap();
        drop(f);

        let (loaded, index) = read_vlog_with_index(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].vertex_id, 99);
        assert!(index.is_empty(), "v1 should have no index");
    }

    #[test]
    fn test_lookup_nonexistent_vertex() {
        let dir = tempdir().unwrap();
        let entries = vec![
            VlogEntry { vertex_id: 1, version: 1, record: make_record(1) },
        ];
        write_vlog(dir.path(), &entries, 1).unwrap();
        let files = list_vlog_files(dir.path()).unwrap();

        let results = lookup_vertex_in_vlog(&files[0], 999).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_multiple_vlog_files() {
        let dir = tempdir().unwrap();
        write_vlog(dir.path(), &[
            VlogEntry { vertex_id: 1, version: 1, record: make_record(1) },
        ], 1).unwrap();
        write_vlog(dir.path(), &[
            VlogEntry { vertex_id: 2, version: 1, record: make_record(1) },
        ], 2).unwrap();
        assert_eq!(list_vlog_files(dir.path()).unwrap().len(), 2);
    }

    #[test]
    fn test_delete_old_vlogs() {
        let dir = tempdir().unwrap();
        for i in 0..5 {
            write_vlog(dir.path(), &[
                VlogEntry { vertex_id: i, version: 1, record: make_record(1) },
            ], i as u32).unwrap();
        }
        assert_eq!(list_vlog_files(dir.path()).unwrap().len(), 5);
        let deleted = delete_vlog_files(dir.path(), 2).unwrap();
        assert_eq!(deleted, 3);
        assert_eq!(list_vlog_files(dir.path()).unwrap().len(), 2);
    }
}
