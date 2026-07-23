//! Multi-graph manager — opens graphs on demand, caches per name.
//!
//! Global defaults come from `settings.json`. Each graph can override
//! its own settings in `<data_dir>/graphs/<name>/config.json`.
//! Graph metadata (descriptions, time-travel, default) is persisted in
//! `<data_dir>/graphs/metadata.json`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::graph::graph::{Graph, GraphConfig};
use crate::graph::graph_registry::{GraphMetadata, GraphRegistry};
use crate::storage::types::StorageResult;

/// Manages lifecycle of multiple named graphs.
pub struct GraphManager {
    graphs: RwLock<HashMap<String, Arc<Graph>>>,
    data_dir: PathBuf,
    registry: RwLock<GraphRegistry>,
}

impl GraphManager {
    /// Create a new graph manager, loading or initializing the registry.
    pub fn new(data_dir: PathBuf) -> Self {
        let graphs_dir = data_dir.join("graphs");
        let registry = GraphRegistry::load(&graphs_dir)
            .unwrap_or_else(|| GraphRegistry::create_initial(&graphs_dir).unwrap_or_else(|_| {
                GraphRegistry {
                    default: "graph0".to_string(),
                    graphs: vec![GraphMetadata {
                        name: "graph0".to_string(),
                        description: "".to_string(),
                        time_travel: true,
                    }],
                }
            }));
        Self {
            graphs: RwLock::new(HashMap::new()),
            data_dir,
            registry: RwLock::new(registry),
        }
    }

    /// Return the data directory path.
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Get or open a graph by name.
    pub fn get(&self, name: &str) -> StorageResult<Arc<Graph>> {
        {
            let graphs = self.graphs.read().unwrap_or_else(|e| e.into_inner());
            if let Some(g) = graphs.get(name) {
                return Ok(g.clone());
            }
        }

        let graphs_dir = self.data_dir.join("graphs");
        std::fs::create_dir_all(&graphs_dir)?;
        let graph = Graph::open(&graphs_dir, name)?;

        // Ensure the graph is tracked in the registry.
        {
            let mut reg = self.registry.write().unwrap_or_else(|e| e.into_inner());
            if !reg.exists(name) {
                reg.ensure(name, "", false);
                reg.save(&graphs_dir)?;
            }
        }

        let mut graphs = self.graphs.write().unwrap_or_else(|e| e.into_inner());
        if let Some(g) = graphs.get(name) {
            return Ok(g.clone());
        }
        graphs.insert(name.to_string(), graph.clone());
        Ok(graph)
    }

    /// Get the per-graph config.
    pub fn get_graph_config(&self, name: &str) -> GraphConfig {
        let path = self.data_dir.join("graphs").join(name);
        GraphConfig::load(&path)
    }

    /// Update the per-graph config and persist to disk.
    pub fn set_graph_config(&self, name: &str, config: &GraphConfig) -> StorageResult<()> {
        let path = self.data_dir.join("graphs").join(name);
        config.save(&path)?;
        Ok(())
    }

    /// List all known graph names.
    pub fn list(&self) -> StorageResult<Vec<String>> {
        let reg = self.registry.read().unwrap_or_else(|e| e.into_inner());
        Ok(reg.list())
    }

    /// Get the default graph name.
    pub fn get_default_name(&self) -> String {
        let reg = self.registry.read().unwrap_or_else(|e| e.into_inner());
        reg.get_default().to_string()
    }

    /// Get all graph metadata (including the default name).
    pub fn get_registry(&self) -> (Vec<GraphMetadata>, String) {
        let reg = self.registry.read().unwrap_or_else(|e| e.into_inner());
        (reg.graphs.clone(), reg.default.clone())
    }

    /// Get metadata for a specific graph.
    pub fn get_meta(&self, name: &str) -> Option<GraphMetadata> {
        let reg = self.registry.read().unwrap_or_else(|e| e.into_inner());
        reg.get_meta(name).cloned()
    }

    /// Check if a graph has time-travel enabled.
    pub fn time_travel_enabled(&self, name: &str) -> bool {
        let reg = self.registry.read().unwrap_or_else(|e| e.into_inner());
        reg.time_travel_enabled(name)
    }

    /// Set the default graph.
    pub fn set_default(&self, name: &str) -> StorageResult<()> {
        let graphs_dir = self.data_dir.join("graphs");
        let mut reg = self.registry.write().unwrap_or_else(|e| e.into_inner());
        if !reg.exists(name) {
            return Err(crate::storage::types::StorageError::GraphNotFound(name.to_string()));
        }
        reg.set_default(name);
        reg.save(&graphs_dir)?;
        Ok(())
    }

    /// Update a graph's metadata (description, time_travel).
    pub fn update_meta(&self, name: &str, description: &str, time_travel: bool) -> StorageResult<bool> {
        let graphs_dir = self.data_dir.join("graphs");
        let mut reg = self.registry.write().unwrap_or_else(|e| e.into_inner());
        let found = reg.update(name, description, time_travel);
        if found {
            reg.save(&graphs_dir)?;
        }
        Ok(found)
    }

    /// Delete a graph from memory, disk, and registry.
    pub fn delete(&self, name: &str) -> StorageResult<()> {
        {
            let mut graphs = self.graphs.write().unwrap_or_else(|e| e.into_inner());
            if let Some(graph) = graphs.remove(name) {
                if let Err(e) = graph.close() {
                    log::warn!("Error closing graph '{}': {}", name, e);
                }
            }
        }
        let graph_path = self.data_dir.join("graphs").join(name);
        if graph_path.exists() {
            std::fs::remove_dir_all(&graph_path)?;
        }
        // Remove from registry.
        let graphs_dir = self.data_dir.join("graphs");
        let mut reg = self.registry.write().unwrap_or_else(|e| e.into_inner());
        reg.remove(name);
        reg.save(&graphs_dir)?;
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

    /// Close all open graphs.
    pub fn close_all(&self) {
        let names: Vec<String> = {
            let graphs = self.graphs.read().unwrap_or_else(|e| e.into_inner());
            graphs.keys().cloned().collect()
        };
        for name in &names {
            if let Some(graph) = self.graphs.read().unwrap_or_else(|e| e.into_inner()).get(name).cloned() {
                if let Err(e) = graph.close() {
                    log::warn!("Error closing graph '{}': {}", name, e);
                }
            }
        }
        if let Ok(mut graphs) = self.graphs.write() {
            graphs.clear();
        }
    }
}
