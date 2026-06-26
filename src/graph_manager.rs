use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::settings::NeuralConfig;
use crate::graph::Graph;

use crate::neuron::{ActivationConfig, LearningConfig, NeuralNetwork};
use crate::persistence;
use crate::storage::{DiskGraph, RedologWal};

/// A handle to a single graph instance within the manager.
#[derive(Clone)]
pub struct GraphHandle {
    pub name: String,
    /// In-memory Graph for Gremlin queries, populated from DiskGraph on startup.
    pub graph: Arc<Mutex<Graph>>,
    /// Disk-backed graph for incremental persistence (subgraphs, LRU cache).
    pub disk_graph: Arc<Mutex<DiskGraph>>,
    pub neural_network: Arc<Mutex<NeuralNetwork>>,
    pub redolog_wal: Arc<Mutex<RedologWal>>,
    pub data_dir: PathBuf,
}

impl GraphHandle {
    /// Get the full path to the data directory for this graph.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

/// Manages multiple named knowledge graphs, each persisted to
/// `data_root/graphs/{graph_name}/`.
pub struct GraphManager {
    graphs: HashMap<String, GraphHandle>,
    data_root: PathBuf,
    /// Stored neural config used when creating new neural networks.
    neural_config: NeuralConfig,
}

impl GraphManager {
    /// Open (or create) all graphs found under `data_root/graphs/`.
    ///
    /// Scans `data_root/graphs/` for subdirectories, opens each one as a graph.
    /// If no graphs exist, creates a `"graph0"` graph (with time-travel enabled).
    /// `neural_config` provides activation and learning parameters for new NeuralNetworks.
    pub fn open(data_root: impl Into<PathBuf>, neural_config: &NeuralConfig) -> Result<Self, String> {
        let data_root: PathBuf = data_root.into();
        let graphs_root = data_root.join("graphs");
        std::fs::create_dir_all(&graphs_root).map_err(|e| format!("Cannot create graphs dir: {}", e))?;

        let neural_config = neural_config.clone();
        let (act_cfg, learn_cfg) = Self::neural_to_configs(&neural_config);

        let mut graphs = HashMap::new();

        // Scan for existing graph directories under graphs/
        if let Ok(entries) = std::fs::read_dir(&graphs_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        // Check if there's any graph data in this directory
                        if path.join("graph.bin").exists() || path.join("neural.bin").exists() {
                            match Self::open_graph(name, &path, &act_cfg, &learn_cfg) {
                                Ok(handle) => {
                                    graphs.insert(name.to_string(), handle);
                                }
                                Err(e) => {
                                    log::warn!("Failed to open graph '{}': {}", name, e);
                                }
                            }
                        }
                    }
                }
            }
        }

        // If no graphs found, create default
        if graphs.is_empty() {
            let default_path = graphs_root.join("graph0");
            let handle = Self::create_graph_internal("graph0", &default_path, true, &act_cfg, &learn_cfg)?;
            graphs.insert("graph0".to_string(), handle);
            log::info!("Created default graph at {:?}", default_path);
        }

        log::info!(
            "GraphManager: {} graph(s) loaded from {:?}",
            graphs.len(),
            data_root
        );

        Ok(Self { graphs, data_root, neural_config })
    }

    /// Create a new named graph.
    pub fn create(&mut self, name: &str) -> Result<GraphHandle, String> {
        self.create_with_opts(name, false)
    }

    /// Create a new named graph with time-travel option.
    pub fn create_with_opts(&mut self, name: &str, time_travel: bool) -> Result<GraphHandle, String> {
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains('.') {
            return Err(format!("Invalid graph name: '{}'", name));
        }
        if self.graphs.contains_key(name) {
            return Err(format!("Graph '{}' already exists", name));
        }
        let graphs_root = self.data_root.join("graphs");
        let path = graphs_root.join(name);
        if path.exists() {
            return Err(format!("Directory already exists: {:?}", path));
        }
        let (act_cfg, learn_cfg) = Self::neural_to_configs(&self.neural_config);
        let handle = Self::create_graph_internal(name, &path, time_travel, &act_cfg, &learn_cfg)?;
        self.graphs.insert(name.to_string(), handle.clone());
        Ok(handle)
    }

    /// Get a graph handle by name. Returns `None` if not found.
    pub fn get(&self, name: &str) -> Option<&GraphHandle> {
        self.graphs.get(name)
    }

    /// Get a mutable reference to a graph handle.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut GraphHandle> {
        self.graphs.get_mut(name)
    }

    /// List all graph names.
    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.graphs.keys().cloned().collect();
        names.sort();
        names
    }

    /// Delete a graph (removes from memory and deletes data directory).
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        if name == "graph0" {
            return Err("Cannot delete the default graph".to_string());
        }
        let handle = self.graphs.remove(name).ok_or_else(|| format!("Graph '{}' not found", name))?;
        // Remove data directory
        if handle.data_dir.exists() {
            std::fs::remove_dir_all(&handle.data_dir)
                .map_err(|e| format!("Failed to delete graph data: {}", e))?;
        }
        log::info!("Deleted graph '{}'", name);
        Ok(())
    }

    /// Check if a graph exists.
    pub fn exists(&self, name: &str) -> bool {
        self.graphs.contains_key(name)
    }

    /// Create an empty manager (used internally for backward compat).
    pub fn empty(data_root: impl Into<PathBuf>) -> Self {
        Self {
            graphs: HashMap::new(),
            data_root: data_root.into(),
            neural_config: NeuralConfig::default(),
        }
    }

    /// Directly insert a handle (used internally for backward compat).
    pub fn insert(&mut self, name: String, handle: GraphHandle) {
        self.graphs.insert(name, handle);
    }

    /// Number of graphs.
    pub fn len(&self) -> usize {
        self.graphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.graphs.is_empty()
    }

    // ─── Config conversion ─────────────────────────────────────

    /// Convert a `NeuralConfig` (from settings) into `ActivationConfig` + `LearningConfig`.
    fn neural_to_configs(nc: &NeuralConfig) -> (ActivationConfig, LearningConfig) {
        let search_mode = if nc.search.default_search_mode.eq_ignore_ascii_case("exact") {
            crate::neuron::SearchMode::Exact
        } else {
            crate::neuron::SearchMode::Greedy
        };
        let act = ActivationConfig {
            max_ticks: nc.activate.max_ticks,
            hot_threshold: nc.activate.hot_threshold,
            search_mode,
            min_synapse_strength: nc.activate.min_synapse_strength,
            auto_stabilize: nc.activate.auto_stabilize,
            greedy_exact_score: nc.search.greedy_exact_score,
            greedy_partial_score: nc.search.greedy_partial_score,
            exact_min_score: nc.search.exact_min_score,
            fuzzy_match_enabled: nc.search.fuzzy_match_enabled,
            fuzzy_match_threshold: nc.search.fuzzy_match_threshold,
        };
        let learn = LearningConfig {
            enabled: nc.learn.enabled,
            co_fire_window: nc.learn.co_fire_window,
            min_plasticity: nc.learn.min_plasticity,
            synaptic_decay: nc.learn.synaptic_decay,
        };
        (act, learn)
    }

    // ─── Internal helpers ──────────────────────────────────────

    fn open_graph(name: &str, data_dir: &Path, act_cfg: &ActivationConfig, learn_cfg: &LearningConfig) -> Result<GraphHandle, String> {
        let neural_path = data_dir.join("neural.bin");

        // Open DiskGraph (subgraph-based, LRU-cached incremental persistence)
        let mut disk_graph = DiskGraph::open(data_dir)
            .map_err(|e| format!("Failed to open DiskGraph '{}': {}", name, e))?;

        // Build in-memory Graph from DiskGraph
        let mut graph = Graph::new();
        {
            let dg = &mut disk_graph;
            // Copy all vertices
            for vid in dg.vertex_ids() {
                if let Some(v) = dg.get_vertex(vid) {
                    let _ = graph.restore_vertex(v.id, v.labels.clone());
                    if let Some(gv) = graph.get_vertex_mut(v.id) {
                        gv.name = v.name.clone();
                        gv.keywords = v.keywords.clone();
                        gv.properties = v.properties.clone();
                        gv.document = v.document.clone();
                        gv._history = v._history.clone();
                        gv._version = v._version;
                        gv._updated_at = v._updated_at;
                        gv._is_deleted = v._is_deleted;
                    }
                }
            }
            // Copy all edges
            for e in dg.all_edges() {
                let _ = graph.restore_edge(e.id, e.label.clone(), e.source, e.target);
                if let Some(ge) = graph.get_edge_mut(e.id) {
                    ge.properties = e.properties.clone();
                }
            }
        }

        // Load neural network
        let mut neural_network = if neural_path.exists() {
            persistence::neuron_store::load_neural_network(&neural_path)
                .map_err(|e| format!("Failed to load neural network '{}': {}", name, e))?
        } else {
            NeuralNetwork::with_config(act_cfg.clone(), learn_cfg.clone())
        };

        // Replay neuron ops from RedologWal (graph ops are handled by DiskGraph's own RedoLog)
        if let Err(e) = RedologWal::replay_archived(&data_dir.to_path_buf(), &mut graph, &mut neural_network) {
            log::warn!("Redolog archived WAL recovery error for '{}': {}", name, e);
        }
        let redolog_path = data_dir.join("redolog.wal");
        let mut redolog_wal = RedologWal::open(&redolog_path)
            .map_err(|e| format!("Failed to open Redolog WAL for '{}': {}", name, e))?;
        if let Err(e) = redolog_wal.replay(&mut graph, &mut neural_network) {
            log::warn!("Redolog WAL recovery error for '{}': {}", name, e);
        }
        // Rebuild synapses from edge neurons — auto_synapse was never WAL-logged
        neural_network.rebuild_synapses();

        Ok(GraphHandle {
            name: name.to_string(),
            graph: Arc::new(Mutex::new(graph)),
            disk_graph: Arc::new(Mutex::new(disk_graph)),
            neural_network: Arc::new(Mutex::new(neural_network)),
            redolog_wal: Arc::new(Mutex::new(redolog_wal)),
            data_dir: data_dir.to_path_buf(),
        })
    }

    fn create_graph_internal(name: &str, data_dir: &Path, time_travel: bool, act_cfg: &ActivationConfig, learn_cfg: &LearningConfig) -> Result<GraphHandle, String> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| format!("Cannot create graph dir: {}", e))?;
        let handle = Self::open_graph(name, data_dir, act_cfg, learn_cfg)?;
        // Set time_travel on the underlying Graph
        if time_travel {
            if let Ok(mut g) = handle.graph.lock() {
                g.time_travel_enabled = true;
            }
        }
        Ok(handle)
    }

    /// Save all graphs.
    /// Add a vertex to a graph — transactional: all in-memory mutations first,
    /// then atomic WAL batch. On WAL failure, memory is rolled back.
    pub fn add_vertex_to_graph(
        &self,
        graph_name: &str,
        name: &str,
        keywords: &[String],
        labels: &[String],
        properties: &std::collections::HashMap<String, crate::graph::PropertyValue>,
    ) -> Result<u64, String> {
        let handle = self.get(graph_name).ok_or_else(|| format!("graph '{}' not found", graph_name))?;
        let mut g = handle.graph.lock().map_err(|e| e.to_string())?;
        let id = g.create_vertex(labels.to_vec());
        if let Some(v) = g.get_vertex_mut(id) {
            v.name = name.to_string();
            v.keywords = keywords.to_vec();
            let mut clean_props = properties.clone();
            clean_props.remove("name");
            clean_props.remove("keywords");
            v.properties = clean_props;
        }
        drop(g);
        let nn_label = labels.first().cloned().unwrap_or_else(|| "entity".to_string());
        let mut neuron_kw = labels.to_vec();
        neuron_kw.push(name.to_string());
        for kw in keywords { neuron_kw.push(kw.clone()); }
        let neuron: crate::neuron::Neuron;
        if let Ok(mut nn) = handle.neural_network.lock() {
            let nid = nn.neuron_count() as u64 + 1;
            let mut n = crate::neuron::Neuron::for_vertex(nid, &nn_label, id);
            n.keywords = neuron_kw;
            nn.add_neuron(n.clone());
            neuron = n;
        } else {
            if let Ok(mut g) = handle.graph.lock() { let _ = g.remove_vertex(id, true); }
            return Err("Failed to lock neural network".to_string());
        }
        let vertex_payload = bincode::serialize(
            &crate::storage::redolog_wal::AddVertexPayload { id, labels: labels.to_vec() }
        ).map_err(|e| format!("Serialization error: {}", e))?;
        let neuron_payload = bincode::serialize(&neuron)
            .map_err(|e| format!("Serialization error: {}", e))?;
        let entries = vec![
            (crate::storage::redolog_wal::OP_ADD_VERTEX, vertex_payload),
            (crate::storage::redolog_wal::OP_ADD_NEURON, neuron_payload),
        ];
        if let Ok(mut wal) = handle.redolog_wal.lock() {
            if let Err(e) = wal.write_batch(&entries) {
                if let Ok(mut nn) = handle.neural_network.lock() { nn.remove_neuron(neuron.id); }
                if let Ok(mut g) = handle.graph.lock() { let _ = g.remove_vertex(id, true); }
                return Err(format!("WAL write failed: {}", e));
            }
        }
        Ok(id)
    }

    /// Add an edge to a graph — transactional: all in-memory mutations first,
    /// then atomic WAL batch. On WAL failure, memory is rolled back.
    pub fn add_edge_to_graph(
        &self,
        graph_name: &str,
        label: &str,
        source: u64,
        target: u64,
        properties: &std::collections::HashMap<String, crate::graph::PropertyValue>,
    ) -> Result<u64, String> {
        let handle = self.get(graph_name).ok_or_else(|| format!("graph '{}' not found", graph_name))?;
        let mut g = handle.graph.lock().map_err(|e| e.to_string())?;
        let id = g.create_edge(label.to_string(), source, target).map_err(|e| e.to_string())?;
        if let Some(e) = g.get_edge_mut(id) {
            let mut clean_props = properties.clone();
            clean_props.remove("label");
            e.properties = clean_props;
        }
        drop(g);
        let neuron: crate::neuron::Neuron;
        if let Ok(mut nn) = handle.neural_network.lock() {
            let nid = nn.neuron_count() as u64 + 1;
            let mut n = crate::neuron::Neuron::for_edge(nid, label, id);
            n.vertex_refs = vec![source, target];
            n.keywords = vec![label.to_string()];
            nn.add_neuron(n.clone());
            nn.auto_synapse(source, target);
            neuron = n;
        } else {
            if let Ok(mut g) = handle.graph.lock() { let _ = g.remove_edge(id); }
            return Err("Failed to lock neural network".to_string());
        }
        let edge_payload = bincode::serialize(
            &crate::storage::redolog_wal::AddEdgePayload {
                id, label: label.to_string(), source, target,
            }
        ).map_err(|e| format!("Serialization error: {}", e))?;
        let neuron_payload = bincode::serialize(&neuron)
            .map_err(|e| format!("Serialization error: {}", e))?;
        let entries = vec![
            (crate::storage::redolog_wal::OP_ADD_EDGE, edge_payload),
            (crate::storage::redolog_wal::OP_ADD_NEURON, neuron_payload),
        ];
        if let Ok(mut wal) = handle.redolog_wal.lock() {
            if let Err(e) = wal.write_batch(&entries) {
                if let Ok(mut nn) = handle.neural_network.lock() { nn.remove_neuron(neuron.id); }
                if let Ok(mut g) = handle.graph.lock() { let _ = g.remove_edge(id); }
                return Err(format!("WAL write failed: {}", e));
            }
        }
        Ok(id)
    }

    /// WAL size threshold in bytes — when exceeded, a full snapshot is written.
    const SNAPSHOT_WAL_THRESHOLD: u64 = 64 * 1024 * 1024; // 64 MB

    /// Periodic incremental save — writes a full snapshot + checkpoints WAL
    /// ONLY when the WAL file exceeds `SNAPSHOT_WAL_THRESHOLD`.
    /// Under light load, this is a no-op — all data survives via WAL replay.
    pub fn save_all(&self) {
        for (_name, handle) in &self.graphs {
            // Check WAL size — skip snapshot if small
            let needs_snapshot = handle.redolog_wal.lock()
                .ok()
                .and_then(|wal| wal.file_size().ok())
                .map(|sz| sz >= Self::SNAPSHOT_WAL_THRESHOLD)
                .unwrap_or(false);
            if !needs_snapshot {
                continue;
            }
            self.save_graph_snapshot(handle);
        }
    }

    /// Write full snapshots (graph.bin + neural.bin) and checkpoint/truncate WAL.
    /// Called on shutdown and when WAL exceeds threshold.
    pub fn save_snapshot(&self) {
        for (_name, handle) in &self.graphs {
            self.save_graph_snapshot(handle);
        }
    }

    fn save_graph_snapshot(&self, handle: &GraphHandle) {
        // ── Step 1: DiskGraph checkpoint (flushes dirty subgraphs) ──
        if let Ok(mut dg) = handle.disk_graph.lock() {
            let _ = dg.checkpoint();
        }
        // ── Step 2: Neural network snapshot ──────────────────
        let neural_path = handle.data_dir.join("neural.bin");
        if let Ok(mut nn) = handle.neural_network.lock() {
            if nn.is_dirty() {
                let _ = persistence::neuron_store::save_neural_network(&nn, &neural_path);
                nn.mark_clean();
            }
        }
        // ── Step 3: Rotate WAL ───────────────────────────────
        if let Ok(mut wal) = handle.redolog_wal.lock() {
            let _ = wal.rotate();
            let _ = wal.clean_archived(2);
        }
    }
}
