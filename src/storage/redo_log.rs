//! Write-Ahead Log (WAL) for crash-safe persistence.
//!
//! Every vertex/edge/token mutation is appended to a redo log file before
//! the in-memory state is updated. On restart, uncheckpointed entries are
//! replayed to restore the graph to its pre-crash state.
//!
//! # File format
//!
//! Files are named `redo_<yyyymmddHHMMss>` and are stored in the graph data
//! directory. Each file contains a sequence of entries:
//!
//! | Field | Type | Size |
//! |-------|------|------|
//! | op_type | u8 | 1 |
//! | op_id | u64 | 8 |
//! | data_len | u32 | 4 |
//! | data | bytes | data_len |
//! | crc32 | u32 | 4 |
//!
//! Total per entry: 17 + data_len bytes.
//!
//! # Rotation
//!
//! When a file exceeds `ROTATION_THRESHOLD` (64 MB), it is closed and a new
//! file is created with the current timestamp.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};

use crate::storage::types::{OpType, StorageError, StorageResult};

/// Default rotation threshold: 64 MB.
pub const ROTATION_THRESHOLD: u64 = 64 * 1024 * 1024;
/// CRC32 of an entry covers: op_type (1) + op_id (8) + data_len (4) + data.
const CRC_HEADER_LEN: usize = 1 + 8 + 4;

/// A single redo log entry read from disk.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RedoLogEntry {
    pub op_type: OpType,
    pub op_id: u64,
    pub data: Vec<u8>,
}

/// WAL manager with append, rotate, and replay.
pub struct RedoLog {
    dir: PathBuf,
    /// The current write-ahead log file (wrapped in Mutex for interior
    /// mutability — the Graph holds `&self` references).
    current: Mutex<RedoLogWriter>,
    /// Monotonically increasing sequence number for checkpoint tracking.
    checkpoint_seq: std::sync::atomic::AtomicU64,
    rotation_threshold: u64,
}

/// Internal writer for a single redo log file.
struct RedoLogWriter {
    file: File,
    /// Base name (e.g. "redo_20250101120000").
    name: String,
    /// Path of the current file.
    path: PathBuf,
    /// Current file size in bytes.
    size: u64,
}

impl RedoLog {
    /// Open/create redo logs in `dir`.
    ///
    /// If there is an existing redo log file, it is opened for appending.
    /// Otherwise a new file is created.
    pub fn open(dir: &Path) -> StorageResult<Self> {
        fs::create_dir_all(dir)?;

        let (name, path, file, size) = find_latest_or_create(dir)?;

        let writer = RedoLogWriter {
            file,
            name,
            path,
            size,
        };

        Ok(Self {
            dir: dir.to_path_buf(),
            current: Mutex::new(writer),
            checkpoint_seq: std::sync::atomic::AtomicU64::new(0),
            rotation_threshold: ROTATION_THRESHOLD,
        })
    }

    /// Append an entry to the current redo log file.
    ///
    /// If the file exceeds the rotation threshold, it is rotated first.
    pub fn append(&self, op_type: OpType, op_id: u64, data: &[u8]) -> StorageResult<()> {
        let mut writer = self.current.lock().unwrap();

        // Rotate if needed.
        let entry_size = (1 + 8 + 4 + data.len() + 4) as u64; // crc32 at end
        if writer.size + entry_size > self.rotation_threshold {
            let seq = self.checkpoint_seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let new_writer = create_new_file(&self.dir, seq)?;
            // Sync and close old file.
            writer.file.sync_all()?;
            *writer = new_writer;
        }

        // Write entry.
        let mut buf = Vec::with_capacity(entry_size as usize);

        // 1. op_type
        buf.push(op_type as u8);
        // 2. op_id (little-endian)
        buf.extend_from_slice(&op_id.to_le_bytes());
        // 3. data_len (little-endian)
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        // 4. data
        buf.extend_from_slice(data);
        // 5. CRC32 of (1+2+3+4)
        let crc = crc32fast::hash(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());

        writer.file.write_all(&buf)?;
        writer.file.sync_all()?;
        writer.size += buf.len() as u64;

        Ok(())
    }

    /// Iterate over all redo log files in order (oldest first) and call
    /// `callback` for each entry.
    ///
    /// This is used at startup to recover state.
    pub fn replay<F>(dir: &Path, mut callback: F) -> StorageResult<()>
    where
        F: FnMut(RedoLogEntry) -> StorageResult<()>,
    {
        let mut files = list_redo_files(dir);
        files.sort();

        for fname in &files {
            let path = dir.join(fname);
            let mut file = File::open(&path)?;
            let mut seq: u64 = 0;

            loop {
                // Read header: op_type (1) + op_id (8) + data_len (4) = 13 bytes.
                let mut header = [0u8; 13];
                match file.read_exact(&mut header) {
                    Ok(_) => {}
                    Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(e) => return Err(e.into()),
                }

                let op_type_byte = header[0];
                let op_id = u64::from_le_bytes(header[1..9].try_into().unwrap());
                let data_len = u32::from_le_bytes(header[9..13].try_into().unwrap()) as usize;

                // Read data.
                let mut data = vec![0u8; data_len];
                file.read_exact(&mut data)?;

                // Read CRC32.
                let mut crc_bytes = [0u8; 4];
                file.read_exact(&mut crc_bytes)?;
                let stored_crc = u32::from_le_bytes(crc_bytes);

                // Verify CRC32.
                let mut crc_buf = Vec::with_capacity(CRC_HEADER_LEN + data_len);
                crc_buf.push(op_type_byte);
                crc_buf.extend_from_slice(&op_id.to_le_bytes());
                crc_buf.extend_from_slice(&(data_len as u32).to_le_bytes());
                crc_buf.extend_from_slice(&data);
                let computed_crc = crc32fast::hash(&crc_buf);

                if stored_crc != computed_crc {
                    return Err(StorageError::RedoLogReplay {
                        seq,
                        message: format!(
                            "CRC mismatch: stored={:#x}, computed={:#x}",
                            stored_crc, computed_crc
                        ),
                    });
                }

                let op_type = OpType::try_from(op_type_byte).map_err(|_| StorageError::RedoLogReplay {
                    seq,
                    message: format!("unknown op_type byte: {:#x}", op_type_byte),
                })?;

                callback(RedoLogEntry {
                    op_type,
                    op_id,
                    data,
                })?;

                seq += 1;
            }
        }

        Ok(())
    }

    /// Remove all redo log files from disk (after a successful checkpoint).
    ///
    /// This is called when all pending mutations have been flushed to the
    /// data files and the in-memory index is consistent.
    pub fn remove_all(dir: &Path) -> StorageResult<()> {
        let files = list_redo_files(dir);
        for fname in &files {
            let path = dir.join(fname);
            let _ = fs::remove_file(&path);
        }
        Ok(())
    }

    /// Perform a full checkpoint: flush all dirty blocks to their data files,
    /// then remove the redo logs.
    ///
    /// The `flush_fn` receives a list of redo log entries that have been
    /// applied and should flush the corresponding dirty blocks to disk.
    /// After flushing, the redo log files are deleted.
    pub fn checkpoint<F>(&self, flush_fn: F) -> StorageResult<()>
    where
        F: FnOnce() -> StorageResult<()>,
    {
        // First, flush all dirty blocks.
        flush_fn()?;

        // Sync the current WAL file (all entries up to now are durable).
        {
            let writer = self.current.lock().unwrap();
            writer.file.sync_all()?;
        }

        // Remove all existing redo log files.
        let dir = self.dir.clone();
        let files = list_redo_files(&dir);
        for fname in &files {
            let path = dir.join(fname);
            let _ = fs::remove_file(&path);
        }

        Ok(())
    }

    /// Flush and sync the current redo log file (make entries durable
    /// without rotating or removing).
    pub fn sync(&self) -> StorageResult<()> {
        let writer = self.current.lock().unwrap();
        writer.file.sync_all()?;
        Ok(())
    }
}

// ── Private helpers ──────────────────────────────────────────────────────────

/// Find the most recent redo log file in `dir`, or create a new one.
fn find_latest_or_create(dir: &Path) -> StorageResult<(String, PathBuf, File, u64)> {
    let mut files = list_redo_files(dir);
    files.sort();

    if let Some(latest) = files.last() {
        let path = dir.join(latest);
        let file = OpenOptions::new().append(true).read(true).open(&path)?;
        let size = file.metadata()?.len();
        Ok((latest.clone(), path, file, size))
    } else {
        let seq = 0;
        let w = create_new_file(dir, seq)?;
        Ok((w.name, w.path, w.file, w.size))
    }
}

/// Create a brand new redo log file.
fn create_new_file(dir: &Path, seq: u64) -> StorageResult<RedoLogWriter> {
    let now = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let name = format!("redo_{}_{:06}", now, seq);
    let path = dir.join(&name);
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(&path)?;
    Ok(RedoLogWriter {
        file,
        name,
        path,
        size: 0,
    })
}

/// List all files in `dir` whose name starts with "redo_".
fn list_redo_files(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("redo_") {
                files.push(name);
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_append_and_replay() {
        let dir = tempdir().unwrap();
        let log = RedoLog::open(dir.path()).unwrap();

        log.append(OpType::VertexCreate, 1, b"hello").unwrap();
        log.append(OpType::EdgeCreate, 2, b"world").unwrap();

        // Drop current log handles so they're closed.
        drop(log);

        let mut entries = Vec::new();
        RedoLog::replay(dir.path(), |entry| {
            entries.push(entry);
            Ok(())
        })
        .unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].op_type as u8, OpType::VertexCreate as u8);
        assert_eq!(entries[0].op_id, 1);
        assert_eq!(&entries[0].data, b"hello");
        assert_eq!(entries[1].op_type as u8, OpType::EdgeCreate as u8);
        assert_eq!(entries[1].op_id, 2);
        assert_eq!(&entries[1].data, b"world");
    }

    #[test]
    fn test_crc_mismatch_detected() {
        let dir = tempdir().unwrap();
        let log = RedoLog::open(dir.path()).unwrap();
        log.append(OpType::VertexCreate, 1, b"data").unwrap();
        drop(log);

        // Corrupt the file.
        let mut files = list_redo_files(dir.path());
        files.sort();
        let path = dir.path().join(&files[0]);
        let mut f = OpenOptions::new().write(true).open(&path).unwrap();
        // Corrupt byte 0 (op_type)
        f.write_all(&[0xFF]).unwrap();
        drop(f);

        let result = RedoLog::replay(dir.path(), |_| Ok(()));
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_all() {
        let dir = tempdir().unwrap();
        let log = RedoLog::open(dir.path()).unwrap();
        log.append(OpType::VertexCreate, 1, b"x").unwrap();
        drop(log);

        assert!(!list_redo_files(dir.path()).is_empty());
        RedoLog::remove_all(dir.path()).unwrap();
        assert!(list_redo_files(dir.path()).is_empty());
    }
}
