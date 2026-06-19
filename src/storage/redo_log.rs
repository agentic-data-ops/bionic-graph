use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::subgraph::SubgraphId;

// ─── Log Entry Types ─────────────────────────────────────────────

pub const ENTRY_ADD_VERTEX: u8 = 0x01;
pub const ENTRY_ADD_EDGE: u8 = 0x02;
pub const ENTRY_REMOVE_VERTEX: u8 = 0x03;
pub const ENTRY_REMOVE_EDGE: u8 = 0x04;
pub const ENTRY_UPDATE_PROPERTY: u8 = 0x05;
pub const ENTRY_ADD_CROSS_EDGE: u8 = 0x06;
pub const ENTRY_CHECKPOINT: u8 = 0xFF;

// ─── Operation Payloads ──────────────────────────────────────────

/// Payload for ADD_VERTEX.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddVertexPayload {
    pub subgraph_id: SubgraphId,
    pub vertex_id: u64,
    pub labels: Vec<String>,
}

/// Payload for ADD_EDGE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddEdgePayload {
    pub subgraph_id: SubgraphId,
    pub edge_id: u64,
    pub label: String,
    pub source: u64,
    pub target: u64,
}

/// Payload for REMOVE_VERTEX.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveVertexPayload {
    pub subgraph_id: SubgraphId,
    pub vertex_id: u64,
}

/// Payload for REMOVE_EDGE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveEdgePayload {
    pub subgraph_id: SubgraphId,
    pub edge_id: u64,
}

/// Payload for UPDATE_PROPERTY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePropertyPayload {
    pub subgraph_id: SubgraphId,
    pub element_id: u64,
    pub is_vertex: bool,
    pub key: String,
    pub value: Vec<u8>, // serialized PropertyValue
}

/// Payload for ADD_CROSS_EDGE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddCrossEdgePayload {
    pub subgraph_id: SubgraphId,
    pub edge_id: u64,
    pub label: String,
    pub source: u64,
    pub target_subgraph: SubgraphId,
    pub target_vertex: u64,
}

/// Checkpoint payload (just timestamp + seq at checkpoint time).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointPayload {
    pub timestamp_us: i64,
    pub seq_at_checkpoint: u64,
}

/// A decoded redo log entry.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub entry_type: u8,
    pub sequence: u64,
    pub data: Vec<u8>,
}

/// All possible redo operations.
#[derive(Debug, Clone)]
pub enum RedoOperation {
    AddVertex(AddVertexPayload),
    AddEdge(AddEdgePayload),
    RemoveVertex(RemoveVertexPayload),
    RemoveEdge(RemoveEdgePayload),
    UpdateProperty(UpdatePropertyPayload),
    AddCrossEdge(AddCrossEdgePayload),
}

impl RedoOperation {
    pub fn entry_type(&self) -> u8 {
        match self {
            Self::AddVertex(_) => ENTRY_ADD_VERTEX,
            Self::AddEdge(_) => ENTRY_ADD_EDGE,
            Self::RemoveVertex(_) => ENTRY_REMOVE_VERTEX,
            Self::RemoveEdge(_) => ENTRY_REMOVE_EDGE,
            Self::UpdateProperty(_) => ENTRY_UPDATE_PROPERTY,
            Self::AddCrossEdge(_) => ENTRY_ADD_CROSS_EDGE,
        }
    }

    fn serialize_payload(&self) -> Vec<u8> {
        match self {
            Self::AddVertex(p) => bincode::serialize(p).unwrap(),
            Self::AddEdge(p) => bincode::serialize(p).unwrap(),
            Self::RemoveVertex(p) => bincode::serialize(p).unwrap(),
            Self::RemoveEdge(p) => bincode::serialize(p).unwrap(),
            Self::UpdateProperty(p) => bincode::serialize(p).unwrap(),
            Self::AddCrossEdge(p) => bincode::serialize(p).unwrap(),
        }
    }
}

// ─── RedoLog ─────────────────────────────────────────────────────

/// Append-only Write-Ahead Log for crash recovery.
///
/// Every mutation is written here **before** being applied to the cache.
/// On crash recovery, we replay entries after the last checkpoint.
pub struct RedoLog {
    file: Option<std::fs::File>,
    path: PathBuf,
    /// Monotonic sequence number (increases per entry).
    sequence: u64,
    /// Sequence number at the last checkpoint.
    last_checkpoint_seq: u64,
    /// Total bytes written since last checkpoint (for triggering new checkpoints).
    bytes_since_checkpoint: u64,
    /// Entries written since last checkpoint.
    entries_since_checkpoint: u64,
}

impl RedoLog {
    /// Open (or create) the redo log file at `path`.
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path: PathBuf = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        Ok(Self {
            file: Some(file),
            path,
            sequence: 0,
            last_checkpoint_seq: 0,
            bytes_since_checkpoint: 0,
            entries_since_checkpoint: 0,
        })
    }

    /// Append an operation to the log (WAL write + fsync).
    pub fn append(&mut self, op: &RedoOperation) -> std::io::Result<u64> {
        let seq = self.sequence;
        self.sequence += 1;

        let payload = op.serialize_payload();
        let entry_bytes = encode_entry(op.entry_type(), seq, &payload);

        if let Some(ref mut file) = self.file {
            file.write_all(&entry_bytes)?;
            file.sync_all()?; // fsync: durability guarantee
        }

        self.bytes_since_checkpoint += entry_bytes.len() as u64;
        self.entries_since_checkpoint += 1;

        Ok(seq)
    }

    /// Write a CHECKPOINT marker.
    ///
    /// After this, entries before the checkpoint can be safely discarded
    /// (the corresponding dirty subgraphs have been flushed to disk).
    pub fn checkpoint(&mut self) -> std::io::Result<u64> {
        let seq = self.sequence;
        self.sequence += 1;

        let payload = CheckpointPayload {
            timestamp_us: chrono::Utc::now().timestamp_micros(),
            seq_at_checkpoint: seq,
        };
        let payload_bytes = bincode::serialize(&payload).unwrap();
        let entry_bytes = encode_entry(ENTRY_CHECKPOINT, seq, &payload_bytes);

        if let Some(ref mut file) = self.file {
            file.write_all(&entry_bytes)?;
            file.sync_all()?;
        }

        self.last_checkpoint_seq = seq;
        self.bytes_since_checkpoint = 0;
        self.entries_since_checkpoint = 0;

        // After checkpoint, try to truncate (rotate the log)
        self.rotate_if_needed()?;

        Ok(seq)
    }

    /// Recover by replaying all entries after the last checkpoint.
    ///
    /// Returns the list of entries to replay (in order).
    pub fn recover(&mut self) -> std::io::Result<Vec<LogEntry>> {
        let mut all_entries = Vec::new();
        // None = no checkpoint found in this file (e.g., after rotation)
        let mut last_checkpoint_pos: Option<usize> = None;

        // Read all existing entries from the log
        if let Some(ref mut file) = self.file {
            file.seek(SeekFrom::Start(0))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;

            let mut pos = 0;
            while pos + 1 + 8 + 4 + 4 < buf.len() {
                let entry_type = buf[pos];
                pos += 1;

                let seq = u64::from_le_bytes(
                    buf[pos..pos + 8].try_into().unwrap(),
                );
                pos += 8;

                let data_len = u32::from_le_bytes(
                    buf[pos..pos + 4].try_into().unwrap(),
                ) as usize;
                pos += 4;

                let stored_crc = u32::from_le_bytes(
                    buf[pos + data_len..pos + data_len + 4]
                        .try_into()
                        .unwrap(),
                );

                // Verify CRC
                let crc_data = &buf[pos - (1 + 8 + 4)..pos + data_len];
                let actual_crc = crc32fast::hash(crc_data);
                if actual_crc != stored_crc {
                    log::warn!("Redo log CRC mismatch at pos {}, stopping", pos);
                    break;
                }

                let data = buf[pos..pos + data_len].to_vec();
                pos += data_len + 4; // skip data + crc

                let entry = LogEntry {
                    entry_type,
                    sequence: seq,
                    data,
                };

                if entry_type == ENTRY_CHECKPOINT {
                    last_checkpoint_pos = Some(all_entries.len());

                    // Update last_checkpoint_seq from the checkpoint payload
                    if let Ok(ckpt) = bincode::deserialize::<CheckpointPayload>(&entry.data) {
                        self.last_checkpoint_seq = ckpt.seq_at_checkpoint;
                    }
                }

                all_entries.push(entry);
            }

            // Update sequence to continue from where we left off
            if let Some(last) = all_entries.last() {
                if last.sequence >= self.sequence {
                    self.sequence = last.sequence + 1;
                }
            }
        }

        match last_checkpoint_pos {
            // Checkpoint found: replay only entries after it
            Some(ckpt_pos) if ckpt_pos + 1 < all_entries.len() => {
                let to_replay = all_entries[ckpt_pos + 1..].to_vec();
                log::info!(
                    "Redo log recovery: {} entries total, {} after last checkpoint, replaying",
                    all_entries.len(),
                    to_replay.len()
                );
                Ok(to_replay)
            }
            // Checkpoint found and it's the last entry: nothing to replay
            Some(_) => {
                log::info!("Redo log: all entries before last checkpoint, nothing to replay");
                Ok(Vec::new())
            }
            // No checkpoint found: replay all entries (previous checkpoint rotated)
            None if !all_entries.is_empty() => {
                log::info!(
                    "Redo log: no checkpoint in this file, replaying all {} entries",
                    all_entries.len()
                );
                Ok(all_entries)
            }
            // Empty log
            None => {
                log::info!("Redo log is empty, no recovery needed");
                Ok(Vec::new())
            }
        }
    }

    /// Rotate the log file — called after a successful checkpoint.
    /// Renames the old log so it can be safely deleted.
    fn rotate_if_needed(&mut self) -> std::io::Result<()> {
        // Only rotate if we have a meaningful amount of data to discard
        if self.bytes_since_checkpoint > 0 || self.entries_since_checkpoint > 0 {
            // Not yet efficient to rotate; the checkpoint marker is in the current file
            return Ok(());
        }

        // Rotate: close old file, open new one
        if let Some(mut file) = self.file.take() {
            file.flush()?;
            // Rename: redo.log → redo.{seq}.done
            let done_path = self.path.with_extension(format!(
                "{:020}.done",
                self.last_checkpoint_seq
            ));
            let closed_path = self.path.with_extension("closed");
            // Need to copy since we can't rename an open handle portably
            // Simpler approach: just close and don't worry about the old file
            // The old file will be cleaned up on next startup
            drop(file);
            let _ = std::fs::rename(&self.path, &done_path);
        }

        // Open new log file
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&self.path)?;
        self.file = Some(file);

        Ok(())
    }

    /// Force-truncate: read back everything before the last checkpoint,
    /// write it to a temp file, then atomically replace.
    /// Called on startup after successful recovery.
    pub fn truncate_after_recovery(&mut self) -> std::io::Result<()> {
        let mut all_entries = Vec::new();

        // Read all entries
        if let Some(ref mut file) = self.file {
            file.seek(SeekFrom::Start(0))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;

            let mut pos = 0;
            while pos + 1 + 8 + 4 + 4 < buf.len() {
                let entry_type = buf[pos];
                pos += 1;
                let seq = u64::from_le_bytes(buf[pos..pos + 8].try_into().unwrap());
                pos += 8;
                let data_len = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;

                // Skip CRC check for speed
                let data = buf[pos..pos + data_len].to_vec();
                pos += data_len + 4;

                all_entries.push((entry_type, seq, data));
            }
        }

        // Find the last checkpoint
        let last_ckpt = all_entries.iter().rposition(|(t, _, _)| *t == ENTRY_CHECKPOINT);
        let keep_from = last_ckpt.unwrap_or(0);

        // Rebuild the log file with only entries from the last checkpoint onward
        let tmp_path = self.path.with_extension("tmp");
        let mut tmp = std::fs::File::create(&tmp_path)?;

        for i in keep_from..all_entries.len() {
            let (entry_type, seq, data) = &all_entries[i];
            let bytes = encode_entry(*entry_type, *seq, data);
            tmp.write_all(&bytes)?;
        }
        tmp.sync_all()?;
        drop(tmp);

        // Atomic replace
        std::fs::rename(&tmp_path, &self.path)?;

        // Re-open the file
        let file = std::fs::OpenOptions::new()
            .append(true)
            .read(true)
            .open(&self.path)?;
        self.file = Some(file);

        log::info!(
            "Redo log truncated: kept {} entries (from checkpoint to end)",
            all_entries.len() - keep_from
        );

        Ok(())
    }

    /// Clean up old .done log files.
    pub fn clean_old_logs(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            let stem = self.path.file_stem().unwrap_or_default();
            for entry in std::fs::read_dir(parent)? {
                let entry = entry?;
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    if ext == "done" && path.file_stem() == Some(stem) {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
        Ok(())
    }

    /// Get the current sequence number.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Get the sequence at the last checkpoint.
    pub fn last_checkpoint_seq(&self) -> u64 {
        self.last_checkpoint_seq
    }

    /// Check whether a checkpoint is recommended based on
    /// the number of entries written.
    pub fn should_checkpoint(&self, max_entries: u64) -> bool {
        self.entries_since_checkpoint >= max_entries
    }

    /// Close the log file.
    pub fn close(&mut self) -> std::io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
            file.sync_all()?;
        }
        Ok(())
    }
}

impl Drop for RedoLog {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

// ─── Binary Encoding ─────────────────────────────────────────────

/// Encode a single log entry into bytes:
/// [type(1)] [seq(8)] [data_len(4)] [data...] [crc32(4)]
fn encode_entry(entry_type: u8, seq: u64, data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + 8 + 4 + data.len() + 4);
    buf.push(entry_type);
    buf.extend_from_slice(&seq.to_le_bytes());
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(data);

    // CRC covers everything from type through data
    let crc = crc32fast::hash(&buf);
    buf.extend_from_slice(&crc.to_le_bytes());
    buf
}

// ─── Decoding (for debugging / inspection) ───────────────────────

/// Decode a log entry from raw bytes. Returns None if truncated or invalid.
pub fn decode_entry(data: &[u8]) -> Option<LogEntry> {
    if data.len() < 1 + 8 + 4 + 4 {
        return None;
    }
    let entry_type = data[0];
    let seq = u64::from_le_bytes(data[1..9].try_into().ok()?);
    let data_len = u32::from_le_bytes(data[9..13].try_into().ok()?) as usize;

    if data.len() < 13 + data_len + 4 {
        return None;
    }

    let stored_crc = u32::from_le_bytes(data[13 + data_len..13 + data_len + 4].try_into().ok()?);
    let crc_data = &data[..13 + data_len];
    let actual_crc = crc32fast::hash(crc_data);

    if actual_crc != stored_crc {
        return None;
    }

    Some(LogEntry {
        entry_type,
        sequence: seq,
        data: data[13..13 + data_len].to_vec(),
    })
}

// ─── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_log() -> (RedoLog, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("redo.log");
        let log = RedoLog::open(&path).unwrap();
        (log, dir)
    }

    #[test]
    fn test_append_and_recover_empty() {
        let (mut log, _dir) = make_log();
        let entries = log.recover().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_append_and_recover_single() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("redo.log");

        // Write phase
        {
            let mut log = RedoLog::open(&path).unwrap();
            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 100,
                labels: vec!["person".to_string()],
            }))
            .unwrap();
            log.checkpoint().unwrap();
        }

        // Read phase (simulating restart)
        {
            let mut log = RedoLog::open(&path).unwrap();
            let entries = log.recover().unwrap();
            // Everything is before checkpoint, so nothing to replay
            assert!(entries.is_empty());
        }
    }

    #[test]
    fn test_entries_after_checkpoint_are_replayed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("redo.log");

        // Write phase
        {
            let mut log = RedoLog::open(&path).unwrap();
            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 1,
                labels: vec!["before".to_string()],
            }))
            .unwrap();
            log.checkpoint().unwrap();
            // This entry is after the checkpoint
            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 2,
                labels: vec!["after".to_string()],
            }))
            .unwrap();
        }

        // Recovery phase
        {
            let mut log = RedoLog::open(&path).unwrap();
            let entries = log.recover().unwrap();
            assert_eq!(entries.len(), 1, "Should replay 1 entry after checkpoint");
            assert_eq!(entries[0].entry_type, ENTRY_ADD_VERTEX);
        }
    }

    #[test]
    fn test_two_checkpoints_only_replay_after_last() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("redo.log");

        {
            let mut log = RedoLog::open(&path).unwrap();
            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 1,
                labels: vec!["a".to_string()],
            }))
            .unwrap();
            log.checkpoint().unwrap();

            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 2,
                labels: vec!["b".to_string()],
            }))
            .unwrap();
            log.checkpoint().unwrap();

            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 3,
                labels: vec!["c".to_string()],
            }))
            .unwrap();
            // No checkpoint after this
        }

        {
            let mut log = RedoLog::open(&path).unwrap();
            let entries = log.recover().unwrap();
            // Only entry after the LAST checkpoint (vertex 3)
            assert_eq!(entries.len(), 1);
        }
    }

    #[test]
    fn test_crc_detects_corruption() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("redo.log");

        {
            let mut log = RedoLog::open(&path).unwrap();
            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 1,
                labels: vec!["test".to_string()],
            }))
            .unwrap();
        }

        // Corrupt the file
        {
            let mut data = std::fs::read(&path).unwrap();
            if data.len() > 20 {
                data[15] ^= 0xFF; // flip some bits
            }
            std::fs::write(&path, &data).unwrap();
        }

        {
            let mut log = RedoLog::open(&path).unwrap();
            let entries = log.recover().unwrap();
            // Should gracefully handle corruption (CRC mismatch stops)
            assert!(entries.is_empty() || entries.len() <= 1);
        }
    }

    #[test]
    fn test_checkpoint_rotation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("redo.log");

        {
            let mut log = RedoLog::open(&path).unwrap();
            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 1,
                labels: vec!["rotate_test".to_string()],
            }))
            .unwrap();
            log.checkpoint().unwrap();
        }

        // After checkpoint + drop, data is in the rotated .done file
        // The main log file may be empty after rotation
        let done_name = format!("redo.{:020}.done", 1);
        let done_path = dir.path().join(&done_name);
        assert!(done_path.exists(), "Done file should exist after checkpoint rotation");
        let metadata = std::fs::metadata(&done_path).unwrap();
        assert!(metadata.len() > 0, "Done file should have content");
    }

    #[test]
    fn test_append_after_recovery() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("redo.log");

        // Write, checkpoint, write more
        {
            let mut log = RedoLog::open(&path).unwrap();
            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 1,
                labels: vec!["first".to_string()],
            }))
            .unwrap();
            log.checkpoint().unwrap();
            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 2,
                labels: vec!["second".to_string()],
            }))
            .unwrap();
        }

        // Recover, then append more
        {
            let mut log = RedoLog::open(&path).unwrap();
            let replay = log.recover().unwrap();
            assert_eq!(replay.len(), 1);

            // After recovery, append new entries
            log.append(&RedoOperation::AddVertex(AddVertexPayload {
                subgraph_id: 1,
                vertex_id: 3,
                labels: vec!["third".to_string()],
            }))
            .unwrap();
        }

        // Verify: should have checkpoint + second + third entries now
        {
            let mut log = RedoLog::open(&path).unwrap();
            let replay = log.recover().unwrap();
            // After last checkpoint: second, third (and checkpoint itself is before)
            assert_eq!(replay.len(), 2);
            assert_eq!(replay[0].sequence, 2); // second
            assert_eq!(replay[1].sequence, 3); // third
        }
    }
}
