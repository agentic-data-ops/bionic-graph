//! Graph metadata registry persisted at `<data_dir>/graphs/metadata.json`.
//!
//! Tracks all known graphs, their descriptions, time-travel setting,
//! and which graph is the default.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use crate::storage::types::StorageResult;

/// Per-graph metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMetadata {
    /// Graph name (also used as its data directory name).
    pub name: String,
    /// Human-readable description of the graph's contents.
    #[serde(default)]
    pub description: String,
    /// Whether time-travel (history / soft-delete) is enabled.
    #[serde(default)]
    pub time_travel: bool,
}

/// The on-disk registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRegistry {
    /// Name of the default graph.
    pub default: String,
    /// All known graphs.
    #[serde(default)]
    pub graphs: Vec<GraphMetadata>,
}

impl GraphRegistry {
    /// Path to the metadata file.
    fn path(graphs_dir: &Path) -> PathBuf {
        graphs_dir.join("metadata.json")
    }

    /// Load the registry from disk. Returns `None` if the file doesn't exist.
    pub fn load(graphs_dir: &Path) -> Option<Self> {
        let path = Self::path(graphs_dir);
        if path.exists() {
            std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        }
    }

    /// Save the registry to disk.
    pub fn save(&self, graphs_dir: &Path) -> StorageResult<()> {
        let path = Self::path(graphs_dir);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| crate::storage::types::StorageError::Other(format!("serialize registry: {}", e)))?;
        std::fs::write(&path, &json)?;
        Ok(())
    }

    /// Create the initial registry (first-time setup).
    /// Scans existing graph directories first; if none found, creates graph0.
    pub fn create_initial(graphs_dir: &Path) -> StorageResult<Self> {
        std::fs::create_dir_all(graphs_dir)?;

        // Scan for existing graph directories (legacy data from before registry).
        let mut existing: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(graphs_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if entry.file_type().map_or(false, |t| t.is_dir()) && entry.path().join("data").exists() {
                    existing.push(name);
                }
            }
        }
        existing.sort();

        let reg = if existing.is_empty() {
            // No existing graphs — create the default.
            Self {
                default: "graph0".to_string(),
                graphs: vec![GraphMetadata {
                    name: "graph0".to_string(),
                    description: "".to_string(),
                    time_travel: true,
                }],
            }
        } else {
            // Use the first existing graph as default.
            Self {
                default: existing[0].clone(),
                graphs: existing.into_iter().map(|name| GraphMetadata {
                    name,
                    description: "".to_string(),
                    time_travel: false,
                }).collect(),
            }
        };
        reg.save(graphs_dir)?;
        Ok(reg)
    }

    /// Ensure a graph exists in the registry, adding it if missing.
    pub fn ensure(&mut self, name: &str, description: &str, time_travel: bool) {
        if !self.graphs.iter().any(|g| g.name == name) {
            self.graphs.push(GraphMetadata {
                name: name.to_string(),
                description: description.to_string(),
                time_travel,
            });
        }
    }

    /// Remove a graph from the registry.
    pub fn remove(&mut self, name: &str) {
        self.graphs.retain(|g| g.name != name);
        // If the default was removed, pick the first remaining graph.
        if self.default == name {
            self.default = self.graphs.first().map(|g| g.name.clone()).unwrap_or_default();
        }
    }

    /// Set the default graph.
    pub fn set_default(&mut self, name: &str) {
        self.default = name.to_string();
    }

    /// Update a graph's metadata (description, time_travel). Returns true if found.
    pub fn update(&mut self, name: &str, description: &str, time_travel: bool) -> bool {
        if let Some(meta) = self.graphs.iter_mut().find(|g| g.name == name) {
            meta.description = description.to_string();
            meta.time_travel = time_travel;
            true
        } else {
            false
        }
    }

    /// Get the default graph name.
    pub fn get_default(&self) -> &str {
        &self.default
    }

    /// List all graph names.
    pub fn list(&self) -> Vec<String> {
        self.graphs.iter().map(|g| g.name.clone()).collect()
    }

    /// Get metadata for a specific graph.
    pub fn get_meta(&self, name: &str) -> Option<&GraphMetadata> {
        self.graphs.iter().find(|g| g.name == name)
    }

    /// Check if a graph exists.
    pub fn exists(&self, name: &str) -> bool {
        self.graphs.iter().any(|g| g.name == name)
    }

    /// Check if a graph has time-travel enabled.
    pub fn time_travel_enabled(&self, name: &str) -> bool {
        self.graphs.iter().find(|g| g.name == name).map_or(false, |g| g.time_travel)
    }
}
