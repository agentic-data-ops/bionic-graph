use serde::{Deserialize, Serialize};

use crate::graph::VertexId;

/// Unique identifier for a neuron.
pub type NeuronId = u64;

/// A synapse connects one neuron (pre-synaptic) to another (post-synaptic).
///
/// The `strength` determines how much activation is passed along when the
/// pre-synaptic neuron fires. The `plasticity` controls how fast Hebbian
/// learning modifies strength.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Synapse {
    pub post_neuron_id: NeuronId,
    /// Connection strength 0.0..1.0 — fraction of activation passed on firing.
    pub strength: f32,
    /// Hebbian learning rate — how much strength changes per co-firing event.
    pub plasticity: f32,
}

/// A single neuron in the bio-inspired activation-spreading network.
///
/// Each neuron represents a concept or topic. It:
/// - Is triggered by matching keywords
/// - Accumulates activation from pre-synaptic neurons
/// - Fires when activation exceeds threshold
/// - Spreads activation to post-synaptic neurons on firing
/// - Enters a refractory period after firing
/// - Links to one or more knowledge graph vertices for retrieval
///
/// Supports time-travel via `version`, `updated_at`, and `is_deleted` fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Neuron {
    pub id: NeuronId,
    /// Human-readable name of the concept this neuron represents.
    pub label: String,
    /// Keywords that can trigger this neuron (lowercased).
    pub keywords: Vec<String>,
    /// Current activation level 0.0..1.0.
    pub activation: f32,
    /// Activation level at which the neuron fires.
    pub threshold: f32,
    /// Per-tick decay: activation *= (1.0 - decay_rate) each tick when not firing.
    pub decay_rate: f32,
    /// Number of ticks the neuron rests after firing.
    pub refractory_ticks: usize,
    /// Remaining ticks in refractory period (0 means not refractory).
    pub refractory_remaining: usize,
    /// Knowledge graph vertices indexed by this neuron.
    pub vertex_refs: Vec<VertexId>,
    /// Outgoing synapses to other neurons.
    pub synapses: Vec<Synapse>,

    // ─── Version fields (time-travel support) ─────────────────────
    pub _version: u64,
    pub _updated_at: i64,
    pub _is_deleted: bool,
}

impl Neuron {
    /// Create a new neuron with default parameters.
    pub fn new(id: NeuronId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            keywords: Vec::new(),
            activation: 0.0,
            threshold: 0.7,
            decay_rate: 0.1,
            refractory_ticks: 3,
            refractory_remaining: 0,
            vertex_refs: Vec::new(),
            synapses: Vec::new(),
            _version: 1,
            _updated_at: crate::graph::vertex::now_micros(),
            _is_deleted: false,
        }
    }

    /// Set the keywords that trigger this neuron.
    pub fn with_keywords(mut self, keywords: Vec<impl Into<String>>) -> Self {
        self.keywords = keywords.into_iter().map(|k| k.into()).collect();
        self
    }

    /// Set custom threshold.
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Set custom decay rate.
    pub fn with_decay(mut self, decay: f32) -> Self {
        self.decay_rate = decay;
        self
    }

    /// Add a link to a graph vertex.
    pub fn link_vertex(&mut self, vertex_id: VertexId) {
        if !self.vertex_refs.contains(&vertex_id) {
            self.vertex_refs.push(vertex_id);
        }
    }

    /// Add an outgoing synapse.
    pub fn add_synapse(&mut self, post_id: NeuronId, strength: f32, plasticity: f32) {
        self.synapses.push(Synapse {
            post_neuron_id: post_id,
            strength: strength.clamp(0.0, 1.0),
            plasticity,
        });
    }

    /// Check if any keyword matches the given query tokens.
    /// Returns the maximum match score (0.0 = no match, 1.0 = exact keyword match).
    pub fn match_keywords(&self, query_tokens: &[&str]) -> f32 {
        let lower_keywords: Vec<String> = self.keywords.iter().map(|k| k.to_lowercase()).collect();
        for token in query_tokens {
            let lower_token = token.to_lowercase();
            if lower_keywords.iter().any(|k| k == &lower_token) {
                return 1.0;
            }
            // Also check partial matches
            if lower_keywords.iter().any(|k| k.contains(&lower_token) || lower_token.contains(k.as_str())) {
                return 0.8;
            }
        }
        0.0
    }

    /// Advance the neuron by one tick.
    /// Returns `true` if the neuron fires this tick.
    pub fn tick(&mut self) -> bool {
        // Handle refractory period
        if self.refractory_remaining > 0 {
            self.refractory_remaining -= 1;
            self.activation = 0.0;
            return false;
        }

        // Check if we fire
        if self.activation >= self.threshold {
            self.fire();
            return true;
        }

        // Decay activation
        self.activation *= 1.0 - self.decay_rate;
        if self.activation < 0.01 {
            self.activation = 0.0;
        }
        false
    }

    /// Fire the neuron: sets up refractory period.
    fn fire(&mut self) {
        self.refractory_remaining = self.refractory_ticks;
        // Activation peaks at 1.0 on fire, then resets
        self.activation = 1.0;
    }

    /// Receive activation from a pre-synaptic neuron.
    pub fn receive_activation(&mut self, amount: f32) {
        if self.refractory_remaining == 0 {
            self.activation = (self.activation + amount).min(1.0);
        }
    }

    /// Soft-delete this neuron.
    pub fn soft_delete(&mut self) {
        self._is_deleted = true;
        self._version += 1;
        self._updated_at = crate::graph::vertex::now_micros();
    }

    /// Restore a soft-deleted neuron.
    pub fn restore(&mut self) {
        self._is_deleted = false;
        self._version += 1;
        self._updated_at = crate::graph::vertex::now_micros();
    }

    /// Whether the neuron is currently in its refractory period.
    pub fn is_refractory(&self) -> bool {
        self.refractory_remaining > 0
    }
}
