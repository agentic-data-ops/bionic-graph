use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::graph::Graph;
use crate::neuron::NeuralNetwork;
use crate::persistence::{self, AutoSaveConfig};

/// A handle to a single graph instance within the manager.
#[derive(Clone)]
pub struct GraphHandle {
    pub name: String,
    pub graph: Arc<Mutex<Graph>>,
    pub neural_network: Arc<Mutex<NeuralNetwork>>,
    pub data_dir: PathBuf,
}

impl GraphHandle {
    /// Get the full path to the data directory for this graph.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

/// Manages multiple named knowledge graphs, each persisted to
/// `data_root/{graph_name}/`.
pub struct GraphManager {
    graphs: HashMap<String, GraphHandle>,
    data_root: PathBuf,
}

impl GraphManager {
    /// Open (or create) all graphs found under `data_root/`.
    ///
    /// Scans `data_root/` for subdirectories, opens each one as a graph.
    /// If no graphs exist, creates a `"default"` graph.
    pub fn open(data_root: impl Into<PathBuf>) -> Result<Self, String> {
        let data_root: PathBuf = data_root.into();
        std::fs::create_dir_all(&data_root).map_err(|e| format!("Cannot create data dir: {}", e))?;

        let mut graphs = HashMap::new();

        // Scan for existing graph directories
        if let Ok(entries) = std::fs::read_dir(&data_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        // Check if there's any graph data in this directory
                        if path.join("graph.bin").exists() || path.join("neural.bin").exists() {
                            match Self::open_graph(name, &path) {
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
            let default_path = data_root.join("default");
            let handle = Self::create_graph_internal("default", &default_path, false)?;
            graphs.insert("default".to_string(), handle);
            log::info!("Created default graph at {:?}", default_path);
        }

        log::info!(
            "GraphManager: {} graph(s) loaded from {:?}",
            graphs.len(),
            data_root
        );

        Ok(Self { graphs, data_root })
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
        let path = self.data_root.join(name);
        if path.exists() {
            return Err(format!("Directory already exists: {:?}", path));
        }
        let handle = Self::create_graph_internal(name, &path, time_travel)?;
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
        if name == "default" {
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

    // ─── Internal helpers ──────────────────────────────────────

    fn open_graph(name: &str, data_dir: &Path) -> Result<GraphHandle, String> {
        let config = AutoSaveConfig {
            graph_path: data_dir.join("graph.bin"),
            neural_path: data_dir.join("neural.bin"),
            disk_data_dir: data_dir.to_path_buf(),
            ..Default::default()
        };

        let (graph, neural_network) = persistence::load_or_create(&config)
            .map_err(|e| format!("Failed to load graph '{}': {}", name, e))?;

        Ok(GraphHandle {
            name: name.to_string(),
            graph: Arc::new(Mutex::new(graph)),
            neural_network: Arc::new(Mutex::new(neural_network)),
            data_dir: data_dir.to_path_buf(),
        })
    }

    fn create_graph_internal(name: &str, data_dir: &Path, time_travel: bool) -> Result<GraphHandle, String> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| format!("Cannot create graph dir: {}", e))?;
        let mut handle = Self::open_graph(name, data_dir)?;
        // Set time_travel on the underlying Graph
        if time_travel {
            if let Ok(mut g) = handle.graph.lock() {
                g.time_travel_enabled = true;
            }
        }
        Ok(handle)
    }

    /// Save all graphs.
    pub fn save_all(&self) {
        for (name, handle) in &self.graphs {
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
        }
    }
}
