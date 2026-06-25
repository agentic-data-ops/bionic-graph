use std::cmp::{max, min};

use serde::{Deserialize, Serialize};

use crate::graph::{EdgeId, VertexId};

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

/// What graph entity this neuron represents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntityType {
    /// This neuron indexes a single vertex.
    Vertex(VertexId),
    /// This neuron indexes a single edge.
    Edge(EdgeId),
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
/// Supports time-travel via `version`, `updated_at`, and `is_deleted` fields.
/// Search mode for keyword matching.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SearchMode {
    Greedy,
    Exact,
}

impl Default for SearchMode {
    fn default() -> Self {
        Self::Greedy
    }
}

/// Configurable scores and thresholds for keyword matching in search.
#[derive(Debug, Clone, Copy)]
pub struct ScoreConfig {
    pub search_mode: SearchMode,
    /// Score for exact keyword match in greedy mode.
    pub greedy_exact_score: f32,
    /// Score for partial (substring) keyword match in greedy mode.
    pub greedy_partial_score: f32,
    /// Minimum score threshold for exact mode match.
    pub exact_min_score: f32,
    /// Enable Levenshtein-distance fuzzy matching fallback.
    pub fuzzy_match_enabled: bool,
    /// Normalized Levenshtein threshold (0.0 = exact, 1.0 = any).
    pub fuzzy_match_threshold: f32,
}

impl ScoreConfig {
    /// Create a ScoreConfig from activation parameters.
    pub fn new(
        search_mode: SearchMode,
        greedy_exact_score: f32,
        greedy_partial_score: f32,
        exact_min_score: f32,
        fuzzy_match_enabled: bool,
        fuzzy_match_threshold: f32,
    ) -> Self {
        Self {
            search_mode,
            greedy_exact_score,
            greedy_partial_score,
            exact_min_score,
            fuzzy_match_enabled,
            fuzzy_match_threshold,
        }
    }
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            search_mode: SearchMode::Greedy,
            greedy_exact_score: 1.0,
            greedy_partial_score: 0.8,
            exact_min_score: 0.5,
            fuzzy_match_enabled: false,
            fuzzy_match_threshold: 0.6,
        }
    }
}

/// Compute normalized Levenshtein similarity between two strings.
/// Returns 1.0 - (edit_distance / max_len), i.e. 1.0 = identical, 0.0 = completely different.
pub fn levenshtein_similarity(a: &str, b: &str) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();
    let max_len = max(a_len, b_len);
    if max_len == 0 {
        return 1.0;
    }

    // Use two-row DP for efficiency
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr: Vec<usize> = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr[j] = min(
                min(curr[j - 1] + 1, prev[j] + 1),
                prev[j - 1] + cost,
            );
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    1.0 - (prev[b_len] as f32 / max_len as f32)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// The graph entity this neuron represents (vertex or edge), if any.
    pub entity_type: Option<EntityType>,
    /// Outgoing synapses to other neurons.
    pub synapses: Vec<Synapse>,

    // ─── Version fields (time-travel support) ─────────────────────
    pub _version: u64,
    pub _updated_at: i64,
    pub _is_deleted: bool,
    /// Microsecond timestamp when this neuron was soft-deleted (0 = not deleted).
    #[serde(default)]
    pub _deleted_at: i64,
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
            entity_type: None,
            synapses: Vec::new(),
            _version: 1,
            _updated_at: crate::graph::vertex::now_micros(),
            _is_deleted: false,
            _deleted_at: 0,
        }
    }

    /// Create a neuron representing a single vertex.
    pub fn for_vertex(id: NeuronId, label: impl Into<String>, vid: VertexId) -> Self {
        let mut n = Self::new(id, label);
        n.vertex_refs.push(vid);
        n.entity_type = Some(EntityType::Vertex(vid));
        n
    }

    /// Create a neuron representing a single edge.
    pub fn for_edge(id: NeuronId, label: impl Into<String>, eid: EdgeId) -> Self {
        let mut n = Self::new(id, label);
        n.entity_type = Some(EntityType::Edge(eid));
        n
    }

    /// Mark this neuron as soft-deleted with the given timestamp.
    /// Idempotent — subsequent calls keep the original `_deleted_at`.
    pub fn mark_deleted(&mut self, deleted_at: i64) {
        if !self._is_deleted {
            self._is_deleted = true;
            self._deleted_at = deleted_at;
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

    /// Check if this neuron was already deleted at `search_at` time.
    /// If `search_at` is None, checks if deleted at all.
    pub fn is_deleted_at(&self, search_at: Option<i64>) -> bool {
        if !self._is_deleted { return false; }
        match search_at {
            Some(ts) => self._deleted_at <= ts,
            None => true,
        }
    }

    /// Check if any keyword matches the given query tokens.
    /// Returns the maximum match score (0.0 = no match, 1.0 = exact keyword match).
    pub fn match_keywords(&self, query_tokens: &[&str], scores: &ScoreConfig) -> f32 {
        let lower_keywords: Vec<String> = self.keywords.iter().map(|k| k.to_lowercase()).collect();
        let lower_tokens: Vec<String> = query_tokens.iter().map(|t| t.to_lowercase()).collect();
        let fuzzy_enabled = scores.fuzzy_match_enabled;
        let fuzzy_threshold = scores.fuzzy_match_threshold;

        match scores.search_mode {
            SearchMode::Exact => {
                // All tokens must match (exactly, by substring, or via fuzzy)
                for token in &lower_tokens {
                    let any_match = lower_keywords.iter().any(|k| {
                        k == token
                            || k.contains(token.as_str())
                            || (fuzzy_enabled && levenshtein_similarity(k, token) >= fuzzy_threshold)
                    });
                    if !any_match {
                        return 0.0;
                    }
                }
                // Score = ratio of tokens that matched exactly
                let exact_count = lower_tokens.iter()
                    .filter(|t| lower_keywords.iter().any(|k| k == *t))
                    .count();
                let ratio = exact_count as f32 / lower_tokens.len().max(1) as f32;
                if ratio >= scores.exact_min_score { ratio } else { 0.0 }
            }
            SearchMode::Greedy => {
                for token in query_tokens {
                    let lower_token = token.to_lowercase();
                    // Exact match
                    if lower_keywords.iter().any(|k| k == &lower_token) {
                        return scores.greedy_exact_score;
                    }
                    // Substring/partial match — keyword contains query token
                    if lower_keywords.iter().any(|k| k.contains(&lower_token)) {
                        return scores.greedy_partial_score;
                    }
                    // Fuzzy match fallback
                    if fuzzy_enabled {
                        if lower_keywords.iter().any(|k| levenshtein_similarity(k, &lower_token) >= fuzzy_threshold) {
                            return scores.greedy_partial_score.max(scores.fuzzy_match_threshold);
                        }
                    }
                }
                0.0
            }
        }
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
