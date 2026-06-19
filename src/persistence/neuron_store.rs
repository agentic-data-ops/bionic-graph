use std::path::Path;

use crate::neuron::NeuralNetwork;

use super::StoreError;

/// Save the neural network to disk as a binary blob.
pub fn save_neural_network(
    network: &NeuralNetwork,
    path: impl AsRef<Path>,
) -> Result<(), StoreError> {
    let encoded =
        bincode::serialize(network).map_err(|e| StoreError::Serialize {
            source: e,
            description: "neural_network".to_string(),
        })?;
    std::fs::write(path.as_ref(), &encoded).map_err(|e| StoreError::Io {
        source: e,
        description: format!("writing neural network to {}", path.as_ref().display()),
    })?;
    Ok(())
}

/// Load the neural network from a binary blob on disk.
pub fn load_neural_network(path: impl AsRef<Path>) -> Result<NeuralNetwork, StoreError> {
    let data = std::fs::read(path.as_ref()).map_err(|e| StoreError::Io {
        source: e,
        description: format!("reading neural network from {}", path.as_ref().display()),
    })?;
    let network: NeuralNetwork =
        bincode::deserialize(&data).map_err(|e| StoreError::Deserialize {
            source: e,
            description: "neural_network".to_string(),
        })?;
    Ok(network)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neuron::Neuron;
    use tempfile::tempdir;

    #[test]
    fn test_neural_network_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("neural.bin");

        let mut network = NeuralNetwork::new();
        let n1 = Neuron::new(1, "AI").with_keywords(vec!["ai"]);
        network.add_neuron(n1);

        save_neural_network(&network, &path).unwrap();

        let loaded = load_neural_network(&path).unwrap();
        assert_eq!(loaded.neuron_count(), 1);
        assert!(loaded.get_neuron(1).is_some());
    }
}
