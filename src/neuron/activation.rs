use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::graph::{EdgeId, VertexId};

use super::neuron::{Neuron, NeuronId, ScoreConfig, Synapse};
#[derive(Debug, Clone, Default)]
pub struct TickResult {
    /// Neurons that fired this tick.
    pub fired: Vec<NeuronId>,
    /// Average activation level across all neurons.
    pub avg_activation: f32,
    /// Whether any neuron fired.
    pub has_activity: bool,
}

/// Configuration for the spreading activation algorithm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationConfig {
    /// How many ticks to run per query.
    pub max_ticks: usize,
    /// Minimum activation for a neuron to be considered "hot".
    pub hot_threshold: f32,
    /// Search mode: Greedy (any keyword match) or Exact (all keywords).
    #[serde(default)]
    pub search_mode: crate::neuron::SearchMode,
    /// Minimum strength for a synapse to pass activation.
    pub min_synapse_strength: f32,
    /// Whether to run until no more neurons fire (auto-stabilize).
    pub auto_stabilize: bool,

    // ── Search score thresholds ────────────────────────────
    /// Score for exact keyword match in greedy mode.
    pub greedy_exact_score: f32,
    /// Score for partial (substring) keyword match in greedy mode.
    pub greedy_partial_score: f32,
    /// Minimum score threshold for exact mode match.
    pub exact_min_score: f32,

    // ── Fuzzy matching ─────────────────────────────────────
    /// Enable Levenshtein-distance fuzzy matching fallback.
    pub fuzzy_match_enabled: bool,
    /// Normalized Levenshtein threshold (0.0 = exact, 1.0 = any).
    pub fuzzy_match_threshold: f32,
}

impl Default for ActivationConfig {
    fn default() -> Self {
        Self {
            max_ticks: 20,
            hot_threshold: 0.3,
            search_mode: crate::neuron::SearchMode::Greedy,
            min_synapse_strength: 0.01,
            auto_stabilize: true,
            greedy_exact_score: 1.0,
            greedy_partial_score: 0.8,
            exact_min_score: 0.5,
            fuzzy_match_enabled: false,
            fuzzy_match_threshold: 0.6,
        }
    }
}

/// Execute one tick of the spreading activation algorithm.
///
/// For each neuron that fired last tick, spread its activation to
/// post-synaptic neurons via synapses, weighted by synapse strength.
///
/// Then run `tick()` on every neuron (which handles decay, refractory, and firing).
pub fn tick(
    neurons: &mut HashMap<NeuronId, Neuron>,
    synapses: &HashMap<NeuronId, Vec<Synapse>>,
) -> TickResult {
    // Phase 1: collect all neurons that fired on the previous tick
    // (they have activation >= threshold and haven't yet spread this tick)
    // Actually, in our model, firing happens inside neuron.tick().
    // So we need a two-phase approach:
    // Phase A: Every neuron ticks -> some fire
    // Phase B: Fired neurons spread activation

    // Phase A: Store pre-tick activations to know who *just* fired
    let _pre_tick_activations: HashMap<NeuronId, f32> =
        neurons.iter().map(|(&id, n)| (id, n.activation)).collect();

    // Phase B: Run tick on all neurons (returns who fires this tick)
    let mut fired = Vec::new();
    let mut total_activation = 0.0;

    for neuron in neurons.values_mut() {
        let did_fire = neuron.tick();
        if did_fire {
            fired.push(neuron.id);
        }
        total_activation += neuron.activation;
    }

    // Phase C: Spread activation from neurons that just fired
    // A neuron "just fired" if pre-tick activation >= threshold and now it's refractory
    let now_fired: HashSet<NeuronId> = fired.iter().copied().collect();

    for firing_id in &now_fired {
        if let Some(out_synapses) = synapses.get(firing_id) {
            for synapse in out_synapses {
                if synapse.strength < 0.01 {
                    continue;
                }
                let post_activation = synapse.strength; // Pass fraction = strength
                if let Some(post_neuron) = neurons.get_mut(&synapse.post_neuron_id) {
                    if !post_neuron.is_refractory() {
                        post_neuron.receive_activation(post_activation);
                    }
                }
            }
        }
    }

    let count = neurons.len();
    let avg_activation = if count > 0 {
        total_activation / count as f32
    } else {
        0.0
    };

    TickResult {
        fired: fired.clone(),
        avg_activation,
        has_activity: !fired.is_empty(),
    }
}

/// Run the full spreading activation cycle for a keyword query.
///
/// 1. Find all neurons matching the query keywords and activate them.
/// 2. Run ticks until stabilization or max_ticks.
/// 3. Collect vertex refs from all neurons that fired or are "hot".
/// 4. Return ranked vertices (by how many firing neurons referenced them).
///
/// Returns `(vertices, fired_neuron_ids, hot_neuron_ids, ticks_run)`.
pub fn search(
    neurons: &mut HashMap<NeuronId, Neuron>,
    synapses: &HashMap<NeuronId, Vec<Synapse>>,
    config: &ActivationConfig,
    query_tokens: &[&str],
    search_at: Option<i64>,
) -> (Vec<(VertexId, u32)>, Vec<(EdgeId, u32)>, Vec<NeuronId>, Vec<NeuronId>, usize) {
    // Step 1: Activate input neurons by keyword matching (skip soft-deleted)
    for neuron in neurons.values_mut() {
        if neuron.is_deleted_at(search_at) { continue; }
        let score_config = ScoreConfig::new(
            config.search_mode,
            config.greedy_exact_score,
            config.greedy_partial_score,
            config.exact_min_score,
            config.fuzzy_match_enabled,
            config.fuzzy_match_threshold,
        );
        let score = neuron.match_keywords(query_tokens, &score_config);
        if score > 0.0 {
            neuron.activation = score;
        }
    }

    // Step 2: Run tick cycles
    let mut ticks_run = 0;
    for _ in 0..config.max_ticks {
        let result = tick(neurons, synapses);
        ticks_run += 1;
        if config.auto_stabilize && !result.has_activity {
            break;
        }
    }

    // Step 3: Collect results — only from neurons that participated in the query
    let mut vertex_score: HashMap<VertexId, u32> = HashMap::new();
    let mut edge_score: HashMap<EdgeId, u32> = HashMap::new();
    let mut fired_ids = Vec::new();
    let mut hot_ids = Vec::new();

    for neuron in neurons.values() {
        let is_active = neuron.activation > 0.0 || neuron.is_refractory();
        if !is_active {
            continue; // Skip neurons that never matched or received activation
        }
        if neuron.activation >= config.hot_threshold {
            hot_ids.push(neuron.id);
        }
        // Collect vertex refs — only from active/fired neurons
        for &vref in &neuron.vertex_refs {
            *vertex_score.entry(vref).or_insert(0) += 1;
        }
        // Collect edge entities
        if let Some(ref et) = neuron.entity_type {
            use crate::neuron::neuron::EntityType;
            if let EntityType::Edge(eid) = et {
                *edge_score.entry(*eid).or_insert(0) += 1;
            }
        }
    }

    // Track which neurons fired or were involved
    for neuron in neurons.values() {
        if !(neuron.activation > 0.0 || neuron.is_refractory()) {
            continue;
        }
        if neuron.is_refractory() || neuron.activation >= config.hot_threshold {
            fired_ids.push(neuron.id);
        }
    }

    // Sort vertices by score descending
    let mut ranked_vertices: Vec<(VertexId, u32)> = vertex_score.into_iter().collect();
    ranked_vertices.sort_by(|a, b| b.1.cmp(&a.1));
    // Sort edges by score descending
    let mut ranked_edges: Vec<(EdgeId, u32)> = edge_score.into_iter().collect();
    ranked_edges.sort_by(|a, b| b.1.cmp(&a.1));

    (ranked_vertices, ranked_edges, fired_ids, hot_ids, ticks_run)
}

/// Reset all neurons to resting state (zero activation, no refractory).
pub fn reset(neurons: &mut HashMap<NeuronId, Neuron>) {
    for neuron in neurons.values_mut() {
        neuron.activation = 0.0;
        neuron.refractory_remaining = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_network() -> (HashMap<NeuronId, Neuron>, HashMap<NeuronId, Vec<Synapse>>) {
        let mut neurons = HashMap::new();
        let mut synapses: HashMap<NeuronId, Vec<Synapse>> = HashMap::new();

        // Neuron 1: "AI" concept
        let mut n1 = Neuron::new(1, "Artificial Intelligence");
        n1.keywords = vec!["ai".to_string(), "artificial intelligence".to_string()];
        n1.vertex_refs = vec![101, 102];
        n1.threshold = 0.5;
        neurons.insert(1, n1);

        // Neuron 2: "Machine Learning" concept
        let mut n2 = Neuron::new(2, "Machine Learning");
        n2.keywords = vec!["machine learning".to_string(), "ml".to_string()];
        n2.vertex_refs = vec![102, 103];
        n2.threshold = 0.6;
        neurons.insert(2, n2);

        // Neuron 3: "Neural Networks" concept
        let mut n3 = Neuron::new(3, "Neural Networks");
        n3.keywords = vec!["neural network".to_string(), "deep learning".to_string()];
        n3.vertex_refs = vec![103, 104];
        n3.threshold = 0.6;
        neurons.insert(3, n3);

        // Synapses: AI -> ML (strong), ML -> NN (medium)
        synapses.insert(1, vec![Synapse {
            post_neuron_id: 2,
            strength: 0.7,
            plasticity: 0.05,
        }]);
        synapses.insert(2, vec![Synapse {
            post_neuron_id: 3,
            strength: 0.5,
            plasticity: 0.05,
        }]);
        synapses.insert(3, vec![]);

        (neurons, synapses)
    }

    #[test]
    fn test_keyword_activation() {
        let mut n = Neuron::new(1, "test");
        n.keywords = vec!["hello".to_string(), "world".to_string()];
        let greedy_scores = ScoreConfig::new(crate::neuron::SearchMode::Greedy, 1.0, 0.8, 0.5, false, 0.6);
        assert_eq!(n.match_keywords(&["hello"], &greedy_scores), 1.0);
        assert_eq!(n.match_keywords(&["world"], &greedy_scores), 1.0);
        assert_eq!(n.match_keywords(&["nope"], &greedy_scores), 0.0);
    }

    #[test]
    fn test_neuron_tick_fires_on_threshold() {
        let mut n = Neuron::new(1, "test");
        n.activation = 0.8;
        n.threshold = 0.7;
        assert!(n.tick());
        assert!(n.is_refractory());
    }

    #[test]
    fn test_neuron_tick_decays() {
        let mut n = Neuron::new(1, "test");
        n.activation = 0.5;
        n.threshold = 1.0; // Won't fire
        assert!(!n.tick());
        assert!(n.activation < 0.5);
    }

    #[test]
    fn test_spreading_activation_search() {
        let (mut neurons, synapses) = build_test_network();

        let config = ActivationConfig {
            max_ticks: 10,
            hot_threshold: 0.1,
            min_synapse_strength: 0.01,
            auto_stabilize: true,
            search_mode: crate::neuron::SearchMode::Greedy,
            greedy_exact_score: 1.0,
            greedy_partial_score: 0.8,
            exact_min_score: 0.5,
            fuzzy_match_enabled: false,
            fuzzy_match_threshold: 0.6,
        };

        let (vertices, _edges, fired, hot, ticks) = search(&mut neurons, &synapses, &config, &["ai"]);
        assert!(vertices.len() >= 2, "Should find vertices via AI neuron");
        assert!(ticks > 0, "Should run at least one tick");
        println!("Fired: {:?}, Hot: {:?}, Ticks: {}", fired, hot, ticks);
        println!("Vertices: {:?}", vertices);
    }
}
