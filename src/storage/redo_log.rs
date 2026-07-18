//! Write-Ahead Log (WAL) for crash-safe persistence.
//!
//! Every vertex/edge/token mutation is appended to a redo log file before
//! the in-memory state is updated. On restart, uncheckpointed entries are
//! replayed to restore the graph to its pre-crash state.
//!
//! # Write path (new)
//!
//! A background writer thread receives log entries through a FIFO channel,
//! accumulates them into batches (up to `BATCH_SIZE`), and writes each batch
//! to the WAL file in a single `write_all` + `fsync` call. Callers wait on a
//! condition variable until the batch is durably committed.
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

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::storage::types::{OpType, StorageError, StorageResult};

/// Default rotation threshold: 64 MB.
pub const ROTATION_THRESHOLD: u64 = 64 * 1024 * 1024;
/// Default batch size: accumulate up to 128 entries before writing.
pub const DEFAULT_BATCH_SIZE: usize = 128;
/// Maximum time the writer waits for more entries before flushing a partial batch.
const BATCH_FLUSH_INTERVAL: Duration = Duration::from_millis(10);
/// CRC32 of an entry covers: op_type (1) + op_id (8) + data_len (4) + data.
const CRC_HEADER_LEN: usize = 1 + 8 + 4;

// ── Data types ───────────────────────────────────────────────────────────────

/// A single redo log entry read from disk.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RedoLogEntry {
    pub op_type: OpType,
    pub op_id: u64,
    pub data: Vec<u8>,
}

/// Shared state between the background writer and callers.
struct WriteState {
    /// Incremented after each batch is durably committed to disk.
    committed_epoch: u64,
    /// If set, the writer encountered a fatal error.
    error: Option<StorageError>,
}

/// Messages sent from the API to the background writer.
enum WriterMessage {
    /// A data entry to be written (pre-encoded binary bytes).
    Entry(Vec<u8>),
    /// Flush any pending batch and ensure all prior entries are durable.
    Flush {
        done: Arc<(Mutex<bool>, Condvar)>,
    },
    /// Close the current file, remove all old files, create a new WAL file.
    Renew {
        done: Arc<(Mutex<bool>, Condvar)>,
    },
    /// Perform a full checkpoint: flush dirty blocks, sync WAL, remove all
    /// old files, create a new WAL file.
    Checkpoint {
        flush_fn: Box<dyn FnOnce() -> StorageResult<()> + Send>,
        done: Arc<(Mutex<bool>, Condvar)>,
    },
    /// Shut down the writer thread.
    Shutdown,
}

// ── RedoLog ─────────────────────────────────────────────────────────────────

/// WAL manager with FIFO queue, batched writer, rotation, and replay.
pub struct RedoLog {
    dir: PathBuf,
    /// Channel to send messages to the background writer.
    writer_tx: Sender<WriterMessage>,
    /// Shared state for epoch-based waiting.
    state: Arc<(Mutex<WriteState>, Condvar)>,
    /// Background writer thread handle.
    handle: Option<JoinHandle<()>>,
    /// File size threshold for rotation (bytes).
    rotation_threshold: u64,
}

impl RedoLog {
    /// Open/create redo logs in `dir` and start the background writer.
    ///
    /// If there is an existing redo log file, it is opened for appending.
    /// Otherwise a new file is created.
    pub fn open(dir: &Path) -> StorageResult<Self> {
        Self::open_with_config(dir, ROTATION_THRESHOLD, None)
    }

    /// Open with a custom rotation threshold and max age.
    pub fn open_with_config(
        dir: &Path,
        rotation_threshold: u64,
        rotation_max_age_secs: Option<u64>,
    ) -> StorageResult<Self> {
        fs::create_dir_all(dir)?;

        let (name, path, file, size) = find_latest_or_create(dir)?;
        let writer = RedoLogWriter {
            file,
            name,
            path: path.clone(),
            size,
            created_at: Instant::now(),
        };

        // Channel for FIFO queue.
        let (tx, rx) = mpsc::channel::<WriterMessage>();

        // Shared state for epoch-based waiting.
        let state = Arc::new((
            Mutex::new(WriteState {
                committed_epoch: 0,
                error: None,
            }),
            Condvar::new(),
        ));

        let dir_buf = dir.to_path_buf();
        let state_clone = state.clone();

        let handle = thread::Builder::new()
            .name("bgraph-wal-writer".into())
            .spawn(move || {
                writer_main_loop(rx, state_clone, dir_buf, rotation_threshold, rotation_max_age_secs, writer);
            })
            .map_err(|e| StorageError::Other(format!("failed to spawn WAL writer thread: {e}")))?;

        Ok(Self {
            dir: dir.to_path_buf(),
            writer_tx: tx,
            state,
            handle: Some(handle),
            rotation_threshold,
        })
    }

    /// Encode a single log entry into its binary representation.
    fn encode_entry(&self, op_type: OpType, op_id: u64, data: &[u8]) -> Vec<u8> {
        let entry_size = (1 + 8 + 4 + data.len() + 4) as usize; // crc32 at end
        let mut buf = Vec::with_capacity(entry_size);

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

        buf
    }

    /// Append an entry to the redo log.
    ///
    /// The entry is sent to the background writer via the FIFO queue.
    /// This call blocks until the writer commits the batch containing this
    /// entry to disk.
    pub fn append(&self, op_type: OpType, op_id: u64, data: &[u8]) -> StorageResult<()> {
        let bytes = self.encode_entry(op_type, op_id, data);

        // Read the current epoch so we can detect when our batch commits.
        let epoch = self.state.0.lock().unwrap().committed_epoch;

        // Send the entry to the writer.
        self.writer_tx
            .send(WriterMessage::Entry(bytes))
            .map_err(|_| StorageError::Other("WAL writer channel closed".into()))?;

        // Wait until the epoch advances (our entry is durable).
        let mut guard = self.state.0.lock().unwrap();
        while guard.committed_epoch == epoch && guard.error.is_none() {
            guard = self.state.1.wait(guard).unwrap();
        }

        if let Some(ref err) = guard.error {
            return Err(err.to_error());
        }

        Ok(())
    }

    /// Flush any pending batch and ensure all prior entries are durable
    /// without rotating or removing files.
    pub fn sync(&self) -> StorageResult<()> {
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let done_clone = done.clone();

        self.writer_tx
            .send(WriterMessage::Flush { done })
            .map_err(|_| StorageError::Other("WAL writer channel closed".into()))?;

        // Wait for the flush to complete.
        let mut guard = done_clone.0.lock().unwrap();
        while !*guard {
            guard = done_clone.1.wait(guard).unwrap();
        }

        // Check for writer errors.
        {
            let state = self.state.0.lock().unwrap();
            if let Some(ref err) = state.error {
                return Err(err.to_error());
            }
        }

        Ok(())
    }

    /// Close the current WAL file, remove all old redo log files from disk,
    /// and create a fresh file for a new WAL epoch.
    ///
    /// Called after startup replay to switch from the consumed WAL to a
    /// clean active file.
    pub fn renew(&self) -> StorageResult<()> {
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let done_clone = done.clone();

        self.writer_tx
            .send(WriterMessage::Renew { done })
            .map_err(|_| StorageError::Other("WAL writer channel closed".into()))?;

        let mut guard = done_clone.0.lock().unwrap();
        while !*guard {
            guard = done_clone.1.wait(guard).unwrap();
        }

        {
            let state = self.state.0.lock().unwrap();
            if let Some(ref err) = state.error {
                return Err(err.to_error());
        }
        }

        Ok(())
    }

    /// Perform a full checkpoint: flush all dirty blocks to their data files,
    /// sync the WAL, then remove all old redo logs and create a fresh file.
    ///
    /// The `flush_fn` is called from the writer thread to ensure data blocks
    /// are durable before the WAL is trimmed.
    pub fn checkpoint<F>(&self, flush_fn: F) -> StorageResult<()>
    where
        F: FnOnce() -> StorageResult<()> + Send + 'static,
    {
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let done_clone = done.clone();

        self.writer_tx
            .send(WriterMessage::Checkpoint {
                flush_fn: Box::new(flush_fn),
                done,
            })
            .map_err(|_| StorageError::Other("WAL writer channel closed".into()))?;

        let mut guard = done_clone.0.lock().unwrap();
        while !*guard {
            guard = done_clone.1.wait(guard).unwrap();
        }

        {
            let state = self.state.0.lock().unwrap();
            if let Some(ref err) = state.error {
                return Err(err.to_error());
            }
        }

        Ok(())
    }

    /// Stop the background writer thread and wait for it to finish.
    ///
    /// This is called during graph shutdown to ensure all pending entries
    /// are flushed before the process exits.
    pub fn stop(&mut self) {
        // Signal shutdown.
        let _ = self.writer_tx.send(WriterMessage::Shutdown);
        // Drop the sender so the writer sees channel disconnection.
        // (We keep a clone for potential sends above, then drain.)
        self.writer_tx = mpsc::channel::<WriterMessage>().0; // dummy sender

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    // ── Static methods (unchanged, no writer interaction) ────────────────

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

                let op_type = OpType::try_from(op_type_byte).map_err(|_| {
                    StorageError::RedoLogReplay {
                        seq,
                        message: format!("unknown op_type byte: {:#x}", op_type_byte),
                    }
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
    pub fn remove_all(dir: &Path) -> StorageResult<()> {
        let files = list_redo_files(dir);
        for fname in &files {
            let path = dir.join(fname);
            let _ = fs::remove_file(&path);
        }
        Ok(())
    }
}

impl Drop for RedoLog {
    fn drop(&mut self) {
        self.stop();
    }
}

// ── Background writer ───────────────────────────────────────────────────────

/// Internal writer for a single redo log file.
struct RedoLogWriter {
    file: File,
    /// Base name (e.g. "redo_20250101120000").
    name: String,
    /// Path of the current file.
    path: PathBuf,
    /// Current file size in bytes.
    size: u64,
    /// When this file was created (for time-based rotation).
    created_at: Instant,
}

/// Main loop of the background WAL writer thread.
///
/// Receives entries from the FIFO channel, accumulates batches, and writes
/// them to the WAL file in a single `write_all + fsync` call.
fn writer_main_loop(
    rx: Receiver<WriterMessage>,
    state: Arc<(Mutex<WriteState>, Condvar)>,
    dir: PathBuf,
    rotation_threshold: u64,
    rotation_max_age_secs: Option<u64>,
    mut writer: RedoLogWriter,
) {
    let mut batch: Vec<Vec<u8>> = Vec::with_capacity(DEFAULT_BATCH_SIZE);
    let mut checkpoint_seq: u64 = 0;

    // ── Main receive loop ────────────────────────────────────────────────
    loop {
        // Wait for the first message.
        let msg = match rx.recv() {
            Ok(m) => m,
            Err(_) => break, // Channel closed, shutdown.
        };

        match msg {
            WriterMessage::Entry(bytes) => {
                batch.push(bytes);

                // Try to collect more entries up to batch size.
                let deadline = Instant::now() + BATCH_FLUSH_INTERVAL;
                while batch.len() < DEFAULT_BATCH_SIZE && Instant::now() < deadline {
                    match rx.try_recv() {
                        Ok(WriterMessage::Entry(b)) => batch.push(b),
                        Ok(other) => {
                            // Control message arrived mid-batch.
                            // Flush the partial batch, then handle control.
                            writer = flush_entries(writer, &mut batch, &dir, rotation_threshold, rotation_max_age_secs, &mut checkpoint_seq, &state);
                            handle_control_msg(other, &mut writer, &dir, &mut checkpoint_seq, &state);
                            continue;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            flush_entries(writer, &mut batch, &dir, rotation_threshold, rotation_max_age_secs, &mut checkpoint_seq, &state);
                            return;
                        }
                    }
                }

                // Flush accumulated batch.
                writer = flush_entries(writer, &mut batch, &dir, rotation_threshold, rotation_max_age_secs, &mut checkpoint_seq, &state);
            }
            WriterMessage::Shutdown => {
                flush_entries(writer, &mut batch, &dir, rotation_threshold, rotation_max_age_secs, &mut checkpoint_seq, &state);
                return;
            }
            WriterMessage::Flush { done } => {
                writer = flush_entries(writer, &mut batch, &dir, rotation_threshold, rotation_max_age_secs, &mut checkpoint_seq, &state);
                let mut guard = done.0.lock().unwrap();
                *guard = true;
                done.1.notify_all();
            }
            WriterMessage::Renew { done } => {
                writer = flush_entries(writer, &mut batch, &dir, rotation_threshold, rotation_max_age_secs, &mut checkpoint_seq, &state);
                let _ = writer.file.sync_all();
                let files = list_redo_files(&dir);
                for fname in &files {
                    let _ = fs::remove_file(dir.join(fname));
                }
                match create_new_file(&dir, checkpoint_seq) {
                    Ok(new_writer) => {
                        checkpoint_seq += 1;
                        writer = new_writer;
                        advance_epoch(&state, None);
                    }
                    Err(e) => advance_epoch(&state, Some(e)),
                }
                let mut guard = done.0.lock().unwrap();
                *guard = true;
                done.1.notify_all();
            }
            WriterMessage::Checkpoint { flush_fn, done } => {
                writer = flush_entries(writer, &mut batch, &dir, rotation_threshold, rotation_max_age_secs, &mut checkpoint_seq, &state);
                if let Err(e) = flush_fn() {
                    advance_epoch(&state, Some(e));
                    let mut guard = done.0.lock().unwrap();
                    *guard = true;
                    done.1.notify_all();
                    return;
                }
                let _ = writer.file.sync_all();
                let files = list_redo_files(&dir);
                for fname in &files {
                    let _ = fs::remove_file(dir.join(fname));
                }
                match create_new_file(&dir, checkpoint_seq) {
                    Ok(new_writer) => {
                        checkpoint_seq += 1;
                        writer = new_writer;
                        advance_epoch(&state, None);
                    }
                    Err(e) => advance_epoch(&state, Some(e)),
                }
                let mut guard = done.0.lock().unwrap();
                *guard = true;
                done.1.notify_all();
            }
        }
    }
}

/// Flush accumulated entries to disk, returning (potentially new) writer.
fn flush_entries(
    mut writer: RedoLogWriter,
    batch: &mut Vec<Vec<u8>>,
    dir: &Path,
    rotation_threshold: u64,
    rotation_max_age_secs: Option<u64>,
    checkpoint_seq: &mut u64,
    state: &Arc<(Mutex<WriteState>, Condvar)>,
) -> RedoLogWriter {
    let result = try_flush_entries(&mut writer, batch, dir, rotation_threshold, rotation_max_age_secs, checkpoint_seq);
    match result {
        Ok(()) => {
            advance_epoch(state, None);
            batch.clear();
            writer
        }
        Err(e) => {
            advance_epoch(state, Some(e));
            writer
        }
    }
}

fn try_flush_entries(
    writer: &mut RedoLogWriter,
    batch: &[Vec<u8>],
    dir: &Path,
    rotation_threshold: u64,
    rotation_max_age_secs: Option<u64>,
    checkpoint_seq: &mut u64,
) -> Result<(), StorageError> {
    if batch.is_empty() {
        return Ok(());
    }

    // Check time-based rotation.
    if let Some(max_age) = rotation_max_age_secs {
        if writer.created_at.elapsed() > Duration::from_secs(max_age) {
            let old_path = writer.path.clone();
            let new_writer = create_new_file(dir, *checkpoint_seq)?;
            *checkpoint_seq += 1;
            writer.file.sync_all()?;
            let _ = fs::remove_file(&old_path);
            *writer = new_writer;
        }
    }

    // Check size-based rotation.
    let batch_size: u64 = batch.iter().map(|b| b.len() as u64).sum();
    if writer.size + batch_size > rotation_threshold {
        let old_path = writer.path.clone();
        let new_writer = create_new_file(dir, *checkpoint_seq)?;
        *checkpoint_seq += 1;
        writer.file.sync_all()?;
        let _ = fs::remove_file(&old_path);
        *writer = new_writer;
    }

    // Write all entries in one call.
    for entry_bytes in batch.iter() {
        writer.file.write_all(entry_bytes)?;
    }
    writer.file.sync_all()?;
    writer.size += batch_size;

    Ok(())
}

/// Handle control messages (Flush, Renew) that arrive mid-batch.
fn handle_control_msg(
    msg: WriterMessage,
    writer: &mut RedoLogWriter,
    dir: &Path,
    checkpoint_seq: &mut u64,
    state: &Arc<(Mutex<WriteState>, Condvar)>,
) {
    match msg {
        WriterMessage::Renew { done } => {
            let _ = writer.file.sync_all();
            let files = list_redo_files(dir);
            for fname in &files {
                let _ = fs::remove_file(dir.join(fname));
            }
            match create_new_file(dir, *checkpoint_seq) {
                Ok(new_writer) => {
                    *checkpoint_seq += 1;
                    *writer = new_writer;
                    advance_epoch(state, None);
                }
                Err(e) => advance_epoch(state, Some(e)),
            }
            let mut guard = done.0.lock().unwrap();
            *guard = true;
            done.1.notify_all();
        }
        _ => {
            // Other control messages: just advance epoch and signal done.
            advance_epoch(state, None);
            if let WriterMessage::Flush { done } = msg {
                let mut guard = done.0.lock().unwrap();
                *guard = true;
                done.1.notify_all();
            }
        }
    }
}

fn advance_epoch(state: &Arc<(Mutex<WriteState>, Condvar)>, err: Option<StorageError>) {
    let mut guard = state.0.lock().unwrap();
    guard.committed_epoch += 1;
    guard.error = err;
    state.1.notify_all();
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
        let w = create_new_file(dir, 0)?;
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
        created_at: Instant::now(),
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_append_and_replay() {
        let dir = tempdir().unwrap();
        let mut log = RedoLog::open(dir.path()).unwrap();

        log.append(OpType::VertexCreate, 1, b"hello").unwrap();
        log.append(OpType::EdgeCreate, 2, b"world").unwrap();

        // Stop the writer so the file is closed.
        log.stop();

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
        let mut log = RedoLog::open(dir.path()).unwrap();
        log.append(OpType::VertexCreate, 1, b"data").unwrap();
        log.stop();

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
        let mut log = RedoLog::open(dir.path()).unwrap();
        log.append(OpType::VertexCreate, 1, b"x").unwrap();
        log.stop();

        assert!(!list_redo_files(dir.path()).is_empty());
        RedoLog::remove_all(dir.path()).unwrap();
        assert!(list_redo_files(dir.path()).is_empty());
    }

    #[test]
    fn test_batch_ordering() {
        let dir = tempdir().unwrap();
        let mut log = RedoLog::open(dir.path()).unwrap();

        // Append multiple entries rapidly — they should land in one batch.
        for i in 0..10 {
            log.append(OpType::VertexCreate, i, b"batch-test").unwrap();
        }
        log.stop();

        let mut entries = Vec::new();
        RedoLog::replay(dir.path(), |entry| {
            entries.push(entry);
            Ok(())
        })
        .unwrap();

        assert_eq!(entries.len(), 10);
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry.op_id, i as u64);
            assert_eq!(entry.op_type as u8, OpType::VertexCreate as u8);
        }
    }

    #[test]
    fn test_sync_flushes_batch() {
        let dir = tempdir().unwrap();
        let mut log = RedoLog::open(dir.path()).unwrap();

        log.append(OpType::VertexCreate, 1, b"data").unwrap();

        // sync should ensure the entry is durable.
        log.sync().unwrap();

        // Now we can stop and replay.
        log.stop();

        let mut entries = Vec::new();
        RedoLog::replay(dir.path(), |entry| {
            entries.push(entry);
            Ok(())
        })
        .unwrap();

        assert_eq!(entries.len(), 1);
    }
}
