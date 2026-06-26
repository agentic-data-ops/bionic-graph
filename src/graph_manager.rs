use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::settings::NeuralConfig;
use crate::graph::Graph;
use crate::neuron::{ActivationConfig, LearningConfig, NeuralNetwork};
use crate::persistence::{self, AutoSaveConfig};
use crate::storage::RedologWal;

/// A handle to a single graph instance within the manager.
#[derive(Clone)]
pub struct GraphHandle {
    pub name: String,
    pub graph: Arc<Mutex<Graph>>,
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
        let config = AutoSaveConfig {
            graph_path: data_dir.join("graph.bin"),
            neural_path: data_dir.join("neural.bin"),
            disk_data_dir: data_dir.to_path_buf(),
            ..Default::default()
        };

        let (mut graph, mut neural_network) = persistence::load_or_create(&config, act_cfg, learn_cfg)
            .map_err(|e| format!("Failed to load graph '{}': {}", name, e))?;

        // Open Redolog WAL and replay un-persisted mutations atomically
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
        // ── Step 1: In-memory vertex ────────────────────────────
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
        // ── Step 2: In-memory neuron ────────────────────────────
        let nn_label = labels.first().cloned().unwrap_or_else(|| "entity".to_string());
        let mut neuron_kw = labels.to_vec();
        neuron_kw.push(name.to_string());
        for kw in keywords {
            neuron_kw.push(kw.clone());
        }
        let neuron: crate::neuron::Neuron;
        if let Ok(mut nn) = handle.neural_network.lock() {
            let nid = nn.neuron_count() as u64 + 1;
            let mut n = crate::neuron::Neuron::for_vertex(nid, &nn_label, id);
            n.keywords = neuron_kw;
            nn.add_neuron(n.clone());
            neuron = n;
        } else {
            // NN lock failed — rollback vertex
            if let Ok(mut g) = handle.graph.lock() {
                let _ = g.remove_vertex(id, true);
            }
            return Err("Failed to lock neural network".to_string());
        }
        // ── Step 3: Atomic WAL batch ────────────────────────────
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
                // WAL failed — rollback both memory changes
                if let Ok(mut nn) = handle.neural_network.lock() {
                    nn.remove_neuron(neuron.id);
                }
                if let Ok(mut g) = handle.graph.lock() {
                    let _ = g.remove_vertex(id, true);
                }
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
        // ── Step 1: In-memory edge ──────────────────────────────
        let mut g = handle.graph.lock().map_err(|e| e.to_string())?;
        let id = g.create_edge(label.to_string(), source, target).map_err(|e| e.to_string())?;
        if let Some(e) = g.get_edge_mut(id) {
            let mut clean_props = properties.clone();
            clean_props.remove("label");
            e.properties = clean_props;
        }
        drop(g);
        // ── Step 2: In-memory neuron + auto_synapse ─────────────
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
            // NN lock failed — rollback edge
            if let Ok(mut g) = handle.graph.lock() {
                let _ = g.remove_edge(id);
            }
            return Err("Failed to lock neural network".to_string());
        }
        // ── Step 3: Atomic WAL batch ────────────────────────────
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
                // WAL failed — rollback both memory changes
                if let Ok(mut nn) = handle.neural_network.lock() {
                    nn.remove_neuron(neuron.id);
                }
                if let Ok(mut g) = handle.graph.lock() {
                    let _ = g.remove_edge(id);
                }
                return Err(format!("WAL write failed: {}", e));
            }
        }
        Ok(id)
    }

    pub fn save_all(&self) {
        for (_name, handle) in &self.graphs {
            let config = AutoSaveConfig {
                graph_path: handle.data_dir.join("graph.bin"),
                neural_path: handle.data_dir.join("neural.bin"),
                disk_data_dir: handle.data_dir.clone(),
                ..Default::default()
            };
            if let Ok(g) = handle.graph.lock() {
                let _ = persistence::graph_store::save_graph(&g, &config.graph_path);
            }
            if let Ok(mut nn) = handle.neural_network.lock() {
                if nn.is_dirty() {
                    let _ = persistence::neuron_store::save_neural_network(&nn, &config.neural_path);
                    nn.mark_clean();
                }
            }
            // Redolog WAL checkpoint after saving both snapshots
            if let Ok(mut wal) = handle.redolog_wal.lock() {
                let _ = wal.checkpoint();
                let _ = wal.truncate_after_checkpoint();
            }
        }
    }
}
