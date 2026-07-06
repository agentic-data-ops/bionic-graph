//! Multi-graph manager — opens graphs on demand, caches per name.
//!
//! Global defaults come from `settings.json`. Each graph can override
//! its own settings in `<data_dir>/graphs/<name>/config.json`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::graph::graph::{Graph, GraphConfig};
use crate::storage::types::StorageResult;

/// Manages lifecycle of multiple named graphs.
pub struct GraphManager2 {
    graphs: RwLock<HashMap<String, Arc<Graph>>>,
    data_dir: PathBuf,
}

impl GraphManager2 {
    /// Create a new graph manager.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            graphs: RwLock::new(HashMap::new()),
            data_dir,
        }
    }

    /// Return the data directory path.
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Get or open a graph by name.
    pub fn get(&self, name: &str) -> StorageResult<Arc<Graph>> {
        {
            let graphs = self.graphs.read().unwrap();
            if let Some(g) = graphs.get(name) {
                return Ok(g.clone());
            }
        }

        let graph = Graph::open(&self.data_dir, name, GraphConfig::default())?;

        let mut graphs = self.graphs.write().unwrap();
        if let Some(g) = graphs.get(name) {
            return Ok(g.clone());
        }
        graphs.insert(name.to_string(), graph.clone());
        Ok(graph)
    }

    /// Get the per-graph config (loads from disk, returns defaults if missing).
    pub fn get_graph_config(&self, name: &str) -> GraphConfig {
        let path = self.data_dir.join(name);
        GraphConfig::load(&path)
    }

    /// Update the per-graph config and persist to disk.
    pub fn set_graph_config(&self, name: &str, config: &GraphConfig) -> StorageResult<()> {
        let path = self.data_dir.join(name);
        config.save(&path)?;
        // If the graph is loaded in memory, update its settings
        // (currently Graph fields are immutable after open — close/reopen needed)
        Ok(())
    }

    /// List all known graph names.
    pub fn list(&self) -> StorageResult<Vec<String>> {
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.data_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map_or(false, |t| t.is_dir()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if entry.path().join("data").exists() {
                        names.push(name);
                    }
                }
            }
        }
        names.sort();
        Ok(names)
    }

    /// Delete a graph from memory and disk.
    pub fn delete(&self, name: &str) -> StorageResult<()> {
        {
            let mut graphs = self.graphs.write().unwrap();
            if let Some(graph) = graphs.remove(name) {
                if let Err(e) = graph.close() {
                    log::warn!("Error closing graph '{}': {}", name, e);
                }
            }
        }
        let path = self.data_dir.join(name);
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        Ok(())
    }

    /// Pre-load all graphs.
    pub fn load_all(&self) -> StorageResult<Vec<String>> {
        let names = self.list()?;
        for name in &names {
            self.get(name)?;
        }
        Ok(names)
    }
}
