use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::graph::VertexId;

use super::activation::{self, ActivationConfig, TickResult};
use super::learning::{self, FiringHistory, LearningConfig};
use super::neuron::{Neuron, NeuronId, Synapse};

/// The top-level neural network that manages all neurons, their synapses,
/// activation spreading, and learning.
///
/// This is the "index layer" for the knowledge graph — it caches topic-level
/// structure and provides fast keyword-to-vertex lookups via bio-inspired
/// spreading activation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralNetwork {
    neurons: HashMap<NeuronId, Neuron>,
    /// Pre-computed synapse lookup: neuron_id → Vec<Synapse>
    synapses: HashMap<NeuronId, Vec<Synapse>>,
    activation_config: ActivationConfig,
    learning_config: LearningConfig,
    total_ticks: u64,
    dirty: bool,
}

impl Default for NeuralNetwork {
    fn default() -> Self {
        Self {
            neurons: HashMap::new(),
            synapses: HashMap::new(),
            activation_config: ActivationConfig::default(),
            learning_config: LearningConfig::default(),
            total_ticks: 0,
            dirty: false,
        }
    }
}

impl NeuralNetwork {
    pub fn new() -> Self {
        Self::default()
    }

    // ─── Neuron Management ────────────────────────────────────────

    /// Add a neuron to the network.
    pub fn add_neuron(&mut self, neuron: Neuron) -> NeuronId {
        let id = neuron.id;
        self.neurons.insert(id, neuron);
        self.synapses.entry(id).or_default();
        self.dirty = true;
        id
    }

    /// Remove a neuron and all its synapses.
    pub fn remove_neuron(&mut self, id: NeuronId) {
        self.neurons.remove(&id);
        self.synapses.remove(&id);
        // Remove all synapses pointing TO this neuron
        for synapses in self.synapses.values_mut() {
            synapses.retain(|s| s.post_neuron_id != id);
        }
        self.dirty = true;
    }

    /// Get a reference to a neuron.
    pub fn get_neuron(&self, id: NeuronId) -> Option<&Neuron> {
        self.neurons.get(&id)
    }

    /// Get a mutable reference to a neuron.
    pub fn get_neuron_mut(&mut self, id: NeuronId) -> Option<&mut Neuron> {
        self.neurons.get_mut(&id)
    }

    /// Get all neurons.
    pub fn all_neurons(&self) -> impl Iterator<Item = &Neuron> {
        self.neurons.values()
    }

    /// Number of neurons in the network.
    pub fn neuron_count(&self) -> usize {
        self.neurons.len()
    }

    /// Add a synapse from one neuron to another.
    pub fn add_synapse(
        &mut self,
        pre_id: NeuronId,
        post_id: NeuronId,
        strength: f32,
        plasticity: f32,
    ) -> Option<()> {
        if !self.neurons.contains_key(&pre_id) || !self.neurons.contains_key(&post_id) {
            return None;
        }
        self.synapses.entry(pre_id).or_default().push(Synapse {
            post_neuron_id: post_id,
            strength: strength.clamp(0.0, 1.0),
            plasticity,
        });
        self.dirty = true;
        Some(())
    }

    /// Link a neuron to a graph vertex.
    pub fn link_vertex(&mut self, neuron_id: NeuronId, vertex_id: VertexId) {
        if let Some(neuron) = self.neurons.get_mut(&neuron_id) {
            neuron.link_vertex(vertex_id);
            self.dirty = true;
        }
    }

    // ─── Query ────────────────────────────────────────────────────

    /// Search the neural index with query keywords.
    ///
    /// Returns ranked vertices, fired neuron IDs, hot neuron IDs, and ticks run.
    pub fn search(
        &mut self,
        query: &str,
    ) -> (Vec<(VertexId, u32)>, Vec<NeuronId>, Vec<NeuronId>, usize) {
        // Tokenize query
        let tokens: Vec<&str> = query
            .split_whitespace()
            .flat_map(|t| t.split(|c: char| !c.is_alphanumeric() && c != '\''))
            .filter(|t| !t.is_empty())
            .collect();

        if tokens.is_empty() {
            return (Vec::new(), Vec::new(), Vec::new(), 0);
        }

        let result = activation::search(
            &mut self.neurons,
            &self.synapses,
            &self.activation_config,
            &tokens,
        );

        // Run Hebbian learning
        let mut history = FiringHistory::new(self.learning_config.co_fire_window);
        history.record_tick(&result.1);
        learning::hebbian_update(
            &self.neurons,
            &mut self.synapses,
            &history,
            &self.learning_config,
        );

        self.total_ticks += result.3 as u64;
        if !result.1.is_empty() {
            self.dirty = true;
        }

        result
    }

    /// Run a single tick and return results.
    pub fn tick(&mut self) -> TickResult {
        let result = activation::tick(&mut self.neurons, &self.synapses);
        self.total_ticks += 1;
        self.dirty = true;
        result
    }

    /// Reset all neurons to resting state.
    pub fn reset(&mut self) {
        activation::reset(&mut self.neurons);
    }

    // ─── Configuration ────────────────────────────────────────────

    pub fn activation_config(&self) -> &ActivationConfig {
        &self.activation_config
    }

    pub fn activation_config_mut(&mut self) -> &mut ActivationConfig {
        &mut self.activation_config
    }

    pub fn learning_config(&self) -> &LearningConfig {
        &self.learning_config
    }

    pub fn learning_config_mut(&mut self) -> &mut LearningConfig {
        &mut self.learning_config
    }

    pub fn total_ticks(&self) -> u64 {
        self.total_ticks
    }

    /// Mark the network as needing persistence.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Check if the network has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark as clean (after saving).
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }
}
