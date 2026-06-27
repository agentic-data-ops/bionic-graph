use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::graph::{EdgeId, VertexId};

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

    /// Create a new NeuralNetwork with the given activation and learning configs.
    pub fn with_config(activation_config: ActivationConfig, learning_config: LearningConfig) -> Self {
        Self {
            neurons: HashMap::new(),
            synapses: HashMap::new(),
            activation_config,
            learning_config,
            total_ticks: 0,
            dirty: false,
        }
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
    pub fn set_search_mode(&mut self, mode: Option<&str>) {
        self.activation_config.search_mode = match mode {
            Some("exact") => crate::neuron::SearchMode::Exact,
            _ => crate::neuron::SearchMode::Greedy,
        };
    }

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

    /// Auto-create synapses between neurons that reference two vertices.
    /// For every neuron whose vertex_refs contains `source`, creates a synapse
    /// to every neuron whose vertex_refs contains `target` (if not already present).
    /// Rebuild synapses for all edge neurons from scratch.
    /// Used after WAL replay to recover auto_synapse connections that
    /// were never written to the WAL.
    pub fn rebuild_synapses(&mut self) {
        // Clear existing synapses to ensure a complete rebuild
        // (old synapses from before the auto_synapse fix may be incomplete)
        self.synapses = self.neurons.keys().map(|&id| (id, Vec::new())).collect();
        let pairs: Vec<(VertexId, VertexId)> = self.neurons.values()
            .filter_map(|n| {
                if let Some(crate::neuron::neuron::EntityType::Edge(_)) = &n.entity_type {
                    if n.vertex_refs.len() >= 2 {
                        Some((n.vertex_refs[0], n.vertex_refs[1]))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        for (source, target) in pairs {
            self.auto_synapse(source, target);
        }
    }

    /// Auto-create synapses between neurons that reference two vertices.
    /// For every neuron whose vertex_refs contains `source`, creates a synapse
    /// to every neuron whose vertex_refs contains `target` (if not already present).
    pub fn auto_synapse(&mut self, source: VertexId, target: VertexId) {
        let src_ids: Vec<NeuronId> = self.neurons.iter()
            .filter(|(_, n)| n.vertex_refs.contains(&source))
            .map(|(&id, _)| id).collect();
        let tgt_ids: Vec<NeuronId> = self.neurons.iter()
            .filter(|(_, n)| n.vertex_refs.contains(&target))
            .map(|(&id, _)| id).collect();
        for &pre in &src_ids {
            for &post in &tgt_ids {
                if pre == post { continue; }
                let exists = self.synapses.get(&pre)
                    .map_or(false, |s| s.iter().any(|syn| syn.post_neuron_id == post));
                if !exists {
                    let _ = self.add_synapse(pre, post, 0.8, 0.1);
                }
            }
        }
    }

    // ─── Query ────────────────────────────────────────────────────

    /// Search the neural index with query keywords.
    ///
    /// `search_at` — optional timestamp for time-travel queries; filters out
    /// neurons that were soft-deleted before this time.
    /// Returns ranked vertices, fired neuron IDs, hot neuron IDs, and ticks run.
    pub fn search(
        &mut self,
        query: &str,
        search_at: Option<i64>,
    ) -> (Vec<(VertexId, u32)>, Vec<(EdgeId, u32)>, Vec<NeuronId>, Vec<NeuronId>, usize) {
        // Reset all neuron states to ensure deterministic results per query
        activation::reset(&mut self.neurons);

        // Tokenize query
        let tokens: Vec<&str> = query
            .split_whitespace()
            .flat_map(|t| t.split(|c: char| !c.is_alphanumeric() && c != '\''))
            .filter(|t| !t.is_empty())
            .collect();

        if tokens.is_empty() {
            return (Vec::new(), Vec::new(), Vec::new(), Vec::new(), 0);
        }

        let result = activation::search(
            &mut self.neurons,
            &self.synapses,
            &self.activation_config,
            &tokens,
            search_at,
        );

        // Run Hebbian learning
        let mut history = FiringHistory::new(self.learning_config.co_fire_window);
        history.record_tick(&result.2);
        learning::hebbian_update(
            &self.neurons,
            &mut self.synapses,
            &history,
            &self.learning_config,
        );

        self.total_ticks += result.4 as u64;
        if !result.2.is_empty() {
            self.dirty = true;
        }

        result
    }

    /// Create neurons for all edges in the graph that don't have one yet.
    /// Returns the number of edge neurons created.
    pub fn reindex_edges(&mut self, graph: &crate::graph::Graph) -> usize {
        let mut count = 0;
        // Collect existing edge neurons
        let indexed_edges: std::collections::HashSet<EdgeId> = self.neurons.values()
            .filter_map(|n| {
                if let Some(crate::neuron::neuron::EntityType::Edge(eid)) = &n.entity_type {
                    Some(*eid)
                } else {
                    None
                }
            })
            .collect();

        for eid in graph.edge_ids() {
            if indexed_edges.contains(eid) {
                continue; // already has a neuron
            }
            if let Some(edge) = graph.get_edge(*eid) {
                let nid = (self.neuron_count() as u64) + 1;
                let mut neuron = crate::neuron::Neuron::for_edge(nid, &edge.label, *eid);
                neuron.vertex_refs = vec![edge.source, edge.target];
                // Build keywords from edge label, and source/target entity names
                let mut keywords = vec![edge.label.clone()];
                if let Some(src) = graph.get_vertex(edge.source) {
                    if let Some(crate::graph::PropertyValue::String(name)) = src.properties.get("name") {
                        keywords.push(name.clone());
                    }
                    if let Some(crate::graph::PropertyValue::String(id)) = src.properties.get("extracted_id") {
                        keywords.push(id.clone());
                    }
                }
                if let Some(tgt) = graph.get_vertex(edge.target) {
                    if let Some(crate::graph::PropertyValue::String(name)) = tgt.properties.get("name") {
                        keywords.push(name.clone());
                    }
                    if let Some(crate::graph::PropertyValue::String(id)) = tgt.properties.get("extracted_id") {
                        keywords.push(id.clone());
                    }
                }
                neuron.keywords = keywords;
                self.add_neuron(neuron);
                count += 1;
            }
        }
        count
    }

    /// Run a single tick and return results.
    pub fn tick(&mut self) -> TickResult {
        let result = activation::tick(&mut self.neurons, &self.synapses, None, &mut std::collections::HashSet::new(), &mut std::collections::HashMap::new());
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
