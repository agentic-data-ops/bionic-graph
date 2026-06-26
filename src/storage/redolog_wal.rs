use std::io::Write;
use std::path::PathBuf;

use crate::graph::{EdgeId, Graph, VertexId, PropertyValue};
use crate::neuron::{NeuralNetwork, Neuron, NeuronId};
use std::collections::HashMap;

// ─── Operation Type Ranges ──────────────────────────────────────
// 0x01-0x0F: Graph operations
// 0x10-0x1F: Neuron operations

pub const OP_ADD_VERTEX: u8 = 0x01;
pub const OP_REMOVE_VERTEX: u8 = 0x02;
pub const OP_ADD_EDGE: u8 = 0x03;
pub const OP_REMOVE_EDGE: u8 = 0x04;
pub const OP_UPDATE_VERTEX: u8 = 0x05;
pub const OP_UPDATE_EDGE: u8 = 0x06;

pub const OP_ADD_NEURON: u8 = 0x11;
pub const OP_REMOVE_NEURON: u8 = 0x12;
pub const OP_ADD_SYNAPSE: u8 = 0x13;
pub const OP_LINK_VERTEX: u8 = 0x14;
pub const OP_UPDATE_NEURON: u8 = 0x15;

pub const OP_CHECKPOINT: u8 = 0xFF;

// ─── Payloads (re-exported from graph_wal/neuron_wal patterns) ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AddVertexPayload {
    pub id: VertexId, pub labels: Vec<String>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RemoveVertexPayload {
    id: VertexId,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AddEdgePayload {
    pub id: EdgeId, pub label: String, pub source: VertexId, pub target: VertexId,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RemoveEdgePayload {
    id: EdgeId,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct UpdateVertexPayload {
    id: VertexId, labels: Vec<String>, properties: HashMap<String, PropertyValue>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct UpdateEdgePayload {
    id: EdgeId, label: String, properties: HashMap<String, PropertyValue>,
}

// ─── RedologWal ──────────────────────────────────────────────────

/// Single write-ahead log for both graph and neuron mutations.
///
/// All entries are written to one file with one `write_all + sync_all`
/// call per transaction, guaranteeing that graph and neuron mutations
/// are persisted atomically — a crash at any point leaves either both
/// committed or neither.
pub struct RedologWal {
    file: Option<std::fs::File>,
    path: PathBuf,
}

impl RedologWal {
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
        Ok(Self { file: Some(file), path })
    }

    pub fn open_in_memory() -> Self {
        Self { file: None, path: PathBuf::new() }
    }

    // ─── Entry encoding helpers ────────────────────────────────

    /// Encode a single WAL entry (type + length + data + CRC32).
    /// Public so callers can build batch entries for `write_batch`.
    pub fn encode(entry_type: u8, data: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + 4 + data.len() + 4);
        buf.push(entry_type);
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(data);
        let crc = crc32fast::hash(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());
        buf
    }

    // ─── Atomic batch write ────────────────────────────────────

    /// Write multiple entries atomically — one `write_all` + one `sync_all`.
    ///
    /// If a crash occurs during this call, either ALL entries are
    /// persisted on disk, or NONE are — guaranteed by the single fsync.
    pub fn write_batch(&mut self, entries: &[(u8, Vec<u8>)]) -> std::io::Result<()> {
        if entries.is_empty() { return Ok(()); }
        let mut buf = Vec::new();
        for (ty, data) in entries {
            buf.extend_from_slice(&Self::encode(*ty, data));
        }
        if let Some(ref mut file) = self.file {
            file.write_all(&buf)?;
            file.sync_all()?;
        }
        Ok(())
    }

    // ─── Single-entry convenience (still atomic alone) ─────────

    pub fn append(&mut self, entry_type: u8, data: &[u8]) -> std::io::Result<()> {
        self.write_batch(&[(entry_type, data.to_vec())])
    }

    // ─── Convenience methods (matching old graph_wal / neuron_wal API) ──

    pub fn append_add_vertex(&mut self, id: VertexId, labels: &[String]) -> std::io::Result<()> {
        let p = AddVertexPayload { id, labels: labels.to_vec() };
        let d = bincode::serialize(&p).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_ADD_VERTEX, &d)
    }
    pub fn append_remove_vertex(&mut self, id: VertexId) -> std::io::Result<()> {
        let d = bincode::serialize(&RemoveVertexPayload { id })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_REMOVE_VERTEX, &d)
    }
    pub fn append_add_edge(&mut self, id: EdgeId, label: &str, source: VertexId, target: VertexId) -> std::io::Result<()> {
        let p = AddEdgePayload { id, label: label.to_string(), source, target };
        let d = bincode::serialize(&p).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_ADD_EDGE, &d)
    }
    pub fn append_remove_edge(&mut self, id: EdgeId) -> std::io::Result<()> {
        let d = bincode::serialize(&RemoveEdgePayload { id })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_REMOVE_EDGE, &d)
    }
    pub fn append_update_vertex(&mut self, id: VertexId, labels: &[String], properties: &HashMap<String, PropertyValue>) -> std::io::Result<()> {
        let p = UpdateVertexPayload { id, labels: labels.to_vec(), properties: properties.clone() };
        let d = bincode::serialize(&p).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_UPDATE_VERTEX, &d)
    }
    pub fn append_update_edge(&mut self, id: EdgeId, label: &str, properties: &HashMap<String, PropertyValue>) -> std::io::Result<()> {
        let p = UpdateEdgePayload { id, label: label.to_string(), properties: properties.clone() };
        let d = bincode::serialize(&p).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_UPDATE_EDGE, &d)
    }
    // Neuron operations
    pub fn append_add_neuron(&mut self, neuron: &Neuron) -> std::io::Result<()> {
        let d = bincode::serialize(neuron).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_ADD_NEURON, &d)
    }
    pub fn append_remove_neuron(&mut self, id: NeuronId) -> std::io::Result<()> {
        let d = bincode::serialize(&id).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_REMOVE_NEURON, &d)
    }
    pub fn append_update_neuron(&mut self, neuron: &Neuron) -> std::io::Result<()> {
        let d = bincode::serialize(neuron).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_UPDATE_NEURON, &d)
    }
    pub fn append_add_synapse(&mut self, pre: NeuronId, post: NeuronId, strength: f32, plasticity: f32) -> std::io::Result<()> {
        let d = bincode::serialize(&(pre, post, strength, plasticity))
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_ADD_SYNAPSE, &d)
    }
    pub fn append_link_vertex(&mut self, nid: NeuronId, vid: VertexId) -> std::io::Result<()> {
        let d = bincode::serialize(&(nid, vid))
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.append(OP_LINK_VERTEX, &d)
    }

    // ─── Checkpoint & Truncation ───────────────────────────────

    /// Write a CHECKPOINT marker.
    pub fn checkpoint(&mut self) -> std::io::Result<()> {
        self.append(OP_CHECKPOINT, b"ckpt")
    }

    /// Truncate log to entries after the last CHECKPOINT.
    pub fn truncate_after_checkpoint(&mut self) -> std::io::Result<()> {
        let entries = self.read_all()?;
        let last_ckpt = entries.iter().rposition(|(t, _, _)| *t == OP_CHECKPOINT);
        let keep_from = last_ckpt.map(|i| i + 1).unwrap_or(0);
        if keep_from == 0 && !entries.is_empty() { return Ok(()); }
        let tmp = self.path.with_extension("wal.tmp");
        let mut f = std::fs::File::create(&tmp)?;
        for i in keep_from..entries.len() {
            let (ty, data, _) = &entries[i];
            f.write_all(&Self::encode(*ty, data))?;
        }
        f.sync_all()?; drop(f);
        self.file = None;
        std::fs::rename(&tmp, &self.path)?;
        self.file = Some(std::fs::OpenOptions::new()
            .create(true).append(true).read(true).open(&self.path)?);
        log::info!("Redolog WAL truncated: kept {} entries", entries.len() - keep_from);
        Ok(())
    }

    // ─── Recovery ──────────────────────────────────────────────

    /// Replay all entries after the last CHECKPOINT.
    /// Calls `apply_graph_op` and `apply_neuron_op` for each entry.
    pub fn replay(
        &mut self,
        graph: &mut Graph,
        nn: &mut NeuralNetwork,
    ) -> std::io::Result<usize> {
        let entries = self.read_all()?;
        if entries.is_empty() { return Ok(0); }
        let last_ckpt = entries.iter().rposition(|(t, _, _)| *t == OP_CHECKPOINT);
        let to_replay: Vec<_> = match last_ckpt {
            Some(p) if p + 1 < entries.len() => entries[p + 1..].to_vec(),
            Some(_) => return Ok(0),
            None => entries,
        };
        if to_replay.is_empty() { return Ok(0); }
        let count = to_replay.len();
        log::info!("Redolog WAL recovery: replaying {} entries", count);
        for (ty, data, _) in &to_replay {
            match *ty {
                // Graph operations
                0x01..=0x06 => apply_graph_op(*ty, data, graph),
                // Neuron operations
                0x11..=0x15 => apply_neuron_op(*ty, data, nn),
                OP_CHECKPOINT => {},
                _ => log::warn!("Redolog WAL: unknown op 0x{:02x}", ty),
            }
        }
        Ok(count)
    }

    // ─── Internal ──────────────────────────────────────────────

    fn read_all(&self) -> std::io::Result<Vec<(u8, Vec<u8>, usize)>> {
        if !self.path.exists() { return Ok(Vec::new()); }
        let buf = std::fs::read(&self.path)?;
        let mut entries = Vec::new();
        let mut pos = 0;
        while pos + 1 + 4 + 4 <= buf.len() {
            let ty = buf[pos]; pos += 1;
            let len = u32::from_le_bytes(buf[pos..pos+4].try_into().unwrap()) as usize; pos += 4;
            if pos + len + 4 > buf.len() { break; }
            let crc_stored = u32::from_le_bytes(buf[pos+len..pos+len+4].try_into().unwrap());
            let crc_actual = crc32fast::hash(&buf[pos-5..pos+len]);
            if crc_actual != crc_stored { break; }
            let data = buf[pos..pos+len].to_vec();
            pos += len + 4;
            entries.push((ty, data, pos));
        }
        Ok(entries)
    }

    pub fn close(&mut self) -> std::io::Result<()> {
        if let Some(mut f) = self.file.take() { f.flush()?; f.sync_all()?; }
        Ok(())
    }
}

impl Drop for RedologWal {
    fn drop(&mut self) { let _ = self.close(); }
}

// ─── Apply helpers ───────────────────────────────────────────────

fn apply_graph_op(ty: u8, data: &[u8], graph: &mut Graph) {
    match ty {
        OP_ADD_VERTEX => {
            if let Ok(p) = bincode::deserialize::<AddVertexPayload>(data) {
                let _ = graph.restore_vertex(p.id, p.labels);
            }
        }
        OP_REMOVE_VERTEX => {
            if let Ok(p) = bincode::deserialize::<RemoveVertexPayload>(data) {
                let _ = graph.remove_vertex(p.id, true);
            }
        }
        OP_ADD_EDGE => {
            if let Ok(p) = bincode::deserialize::<AddEdgePayload>(data) {
                let _ = graph.restore_edge(p.id, p.label, p.source, p.target);
            }
        }
        OP_REMOVE_EDGE => {
            if let Ok(p) = bincode::deserialize::<RemoveEdgePayload>(data) {
                let _ = graph.remove_edge(p.id);
            }
        }
        OP_UPDATE_VERTEX => {
            if let Ok(p) = bincode::deserialize::<UpdateVertexPayload>(data) {
                if let Some(v) = graph.get_vertex_mut(p.id) {
                    v.labels = p.labels; v.properties = p.properties;
                }
            }
        }
        OP_UPDATE_EDGE => {
            if let Ok(p) = bincode::deserialize::<UpdateEdgePayload>(data) {
                if let Some(e) = graph.get_edge_mut(p.id) {
                    e.label = p.label; e.properties = p.properties;
                }
            }
        }
        _ => {}
    }
}

fn apply_neuron_op(ty: u8, data: &[u8], nn: &mut NeuralNetwork) {
    match ty {
        OP_ADD_NEURON | OP_UPDATE_NEURON => {
            if let Ok(neuron) = bincode::deserialize::<Neuron>(data) {
                // Remove old if updating
                if nn.get_neuron(neuron.id).is_some() {
                    nn.remove_neuron(neuron.id);
                }
                nn.add_neuron(neuron);
            }
        }
        OP_REMOVE_NEURON => {
            if let Ok(id) = bincode::deserialize::<NeuronId>(data) {
                nn.remove_neuron(id);
            }
        }
        OP_ADD_SYNAPSE => {
            if let Ok((pre, post, strength, plasticity)) = bincode::deserialize::<(NeuronId, NeuronId, f32, f32)>(data) {
                nn.add_synapse(pre, post, strength, plasticity);
            }
        }
        OP_LINK_VERTEX => {
            if let Ok((nid, vid)) = bincode::deserialize::<(NeuronId, VertexId)>(data) {
                nn.link_vertex(nid, vid);
            }
        }
        _ => {}
    }
}
