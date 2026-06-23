use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::graph_store;
use super::neuron_store;
use super::StoreError;
use crate::graph::Graph;
use crate::neuron::NeuralNetwork;
use crate::storage::DiskGraph;

/// Configuration for the auto-save / checkpoint background thread.
#[derive(Debug, Clone)]
pub struct AutoSaveConfig {
    /// Path for the in-memory graph data file (legacy).
    pub graph_path: PathBuf,
    /// Path for the neural network data file.
    pub neural_path: PathBuf,
    /// Data directory for disk-backed graph (new).
    pub disk_data_dir: PathBuf,
    /// How often to check for work (seconds).
    pub check_interval_secs: u64,
    /// How many WAL entries before triggering a checkpoint (disk graph).
    pub checkpoint_interval_entries: u64,
    /// Whether to save on shutdown.
    pub save_on_shutdown: bool,
}

impl Default for AutoSaveConfig {
    fn default() -> Self {
        Self {
            graph_path: PathBuf::from("data/graph.bin"),
            neural_path: PathBuf::from("data/neural.bin"),
            disk_data_dir: PathBuf::from("data"),
            check_interval_secs: 5,
            checkpoint_interval_entries: 1000,
            save_on_shutdown: true,
        }
    }
}

/// Handle for controlling the auto-save background thread.
pub struct AutoSaveHandle {
    shutdown_flag: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl AutoSaveHandle {
    pub fn shutdown(mut self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AutoSaveHandle {
    fn drop(&mut self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
    }
}

// ─── Background thread for Legacy (in-memory Graph) ─────────────

/// Start the auto-save background thread for the legacy in-memory graph.
pub fn start_auto_save(
    graph: Arc<Mutex<Graph>>,
    neural_network: Arc<Mutex<NeuralNetwork>>,
    config: AutoSaveConfig,
) -> AutoSaveHandle {
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let flag = shutdown_flag.clone();
    let interval = Duration::from_secs(config.check_interval_secs);

    // Ensure data directory exists
    if let Some(parent) = config.graph_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Some(parent) = config.neural_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let g_path = config.graph_path.clone();
    let n_path = config.neural_path.clone();

    let handle = thread::Builder::new()
        .name("auto-save".to_string())
        .spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                thread::sleep(interval);

                // Save graph if dirty
                if let Ok(graph) = graph.lock() {
                    let _ = graph_store::save_graph(&graph, &g_path);
                }

                // Save neural network if dirty
                if let Ok(mut nn) = neural_network.lock() {
                    if nn.is_dirty() {
                        if let Err(e) = neuron_store::save_neural_network(&nn, &n_path) {
                            eprintln!("Auto-save neural network error: {}", e);
                        } else {
                            nn.mark_clean();
                        }
                    }
                }
            }

            // Final save on shutdown
            if let Ok(graph) = graph.lock() {
                let _ = graph_store::save_graph(&graph, &g_path);
            }
            if let Ok(mut nn) = neural_network.lock() {
                if nn.is_dirty() {
                    let _ = neuron_store::save_neural_network(&nn, &n_path);
                    nn.mark_clean();
                }
            }
        })
        .expect("failed to spawn auto-save thread");

    AutoSaveHandle {
        shutdown_flag,
        thread_handle: Some(handle),
    }
}

// ─── Background thread for DiskGraph (checkpoint-based) ─────────

/// Start the checkpoint background thread for the disk-backed graph.
///
/// Periodically:
/// 1. Check the redo log entry count
/// 2. If over the threshold, trigger a checkpoint
/// 3. Also save the neural network independently
pub fn start_disk_graph_checkpoint(
    disk_graph: Arc<Mutex<DiskGraph>>,
    neural_network: Arc<Mutex<NeuralNetwork>>,
    config: AutoSaveConfig,
) -> AutoSaveHandle {
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let flag = shutdown_flag.clone();
    let interval = Duration::from_secs(config.check_interval_secs);
    let max_entries = config.checkpoint_interval_entries;

    let n_path = config.neural_path.clone();

    if let Some(parent) = n_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let handle = thread::Builder::new()
        .name("checkpoint".to_string())
        .spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                thread::sleep(interval);

                // Checkpoint disk graph if needed
                if let Ok(mut dg) = disk_graph.lock() {
                    if dg.redo_log.should_checkpoint(max_entries) {
                        if let Err(e) = dg.checkpoint() {
                            log::error!("Checkpoint error: {}", e);
                        }
                    }
                }

                // Save neural network independently
                if let Ok(mut nn) = neural_network.lock() {
                    if nn.is_dirty() {
                        if let Err(e) = neuron_store::save_neural_network(&nn, &n_path) {
                            log::error!("Auto-save neural network error: {}", e);
                        } else {
                            nn.mark_clean();
                        }
                    }
                }
            }

            // Final checkpoint on shutdown
            if let Ok(mut dg) = disk_graph.lock() {
                let _ = dg.checkpoint();
            }
            if let Ok(mut nn) = neural_network.lock() {
                if nn.is_dirty() {
                    let _ = neuron_store::save_neural_network(&nn, &n_path);
                    nn.mark_clean();
                }
            }
        })
        .expect("failed to spawn checkpoint thread");

    AutoSaveHandle {
        shutdown_flag,
        thread_handle: Some(handle),
    }
}

/// Load the graph and neural network from disk (legacy), or create empty ones.
pub fn load_or_create(
    config: &AutoSaveConfig,
) -> Result<(Graph, NeuralNetwork), StoreError> {
    let graph = if config.graph_path.exists() {
        graph_store::load_graph(&config.graph_path)?
    } else {
        Graph::new()
    };

    let neural = if config.neural_path.exists() {
        neuron_store::load_neural_network(&config.neural_path)?
    } else {
        NeuralNetwork::new()
    };

    Ok((graph, neural))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_load_or_create_new() {
        let dir = tempdir().unwrap();
        let config = AutoSaveConfig {
            graph_path: dir.path().join("graph.bin"),
            neural_path: dir.path().join("neural.bin"),
            disk_data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };

        let (graph, nn) = load_or_create(&config).unwrap();
        assert_eq!(graph.vertex_count(), 0);
        assert_eq!(nn.neuron_count(), 0);
    }
}
