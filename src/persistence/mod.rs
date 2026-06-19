pub mod auto_save;
pub mod graph_store;
pub mod neuron_store;

use std::fmt;

pub use auto_save::{
    load_or_create, start_auto_save, start_disk_graph_checkpoint, AutoSaveConfig, AutoSaveHandle,
};
pub use graph_store::{checkpoint, load_graph, open_disk_graph, save_graph};
pub use neuron_store::{load_neural_network, save_neural_network};

/// Errors that can occur during persistence operations.
#[derive(Debug)]
pub enum StoreError {
    Serialize {
        source: bincode::Error,
        description: String,
    },
    Deserialize {
        source: bincode::Error,
        description: String,
    },
    Io {
        source: std::io::Error,
        description: String,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Serialize {
                source,
                description,
            } => write!(f, "failed to serialize {}: {}", description, source),
            StoreError::Deserialize {
                source,
                description,
            } => write!(f, "failed to deserialize {}: {}", description, source),
            StoreError::Io { source, description } => {
                write!(f, "io error {}: {}", description, source)
            }
        }
    }
}

impl std::error::Error for StoreError {}
