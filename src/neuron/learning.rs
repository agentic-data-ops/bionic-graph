use std::collections::{HashMap, HashSet};

use super::neuron::{Neuron, NeuronId};

/// Configuration for Hebbian learning in the neural network.
#[derive(Debug, Clone)]
pub struct LearningConfig {
    /// Enable or disable learning entirely.
    pub enabled: bool,
    /// How many ticks of history to track for co-firing detection.
    pub co_fire_window: usize,
    /// Minimum synapse plasticity to allow learning.
    pub min_plasticity: f32,
    /// Decay factor for synapse strength when pre fires without post.
    pub synaptic_decay: f32,
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            co_fire_window: 5,
            min_plasticity: 0.001,
            synaptic_decay: 0.01,
        }
    }
}

/// A circular buffer tracking which neurons fired in recent ticks.
///
/// This is used by the Hebbian learning rule: if neuron A fires,
/// and neuron B fires within the co-fire window, the A→B synapse
/// is strengthened.
#[derive(Debug, Clone)]
pub struct FiringHistory {
    /// One `HashSet<NeuronId>` per tracked tick, oldest first.
    history: Vec<HashSet<NeuronId>>,
    window: usize,
}

impl FiringHistory {
    pub fn new(window: usize) -> Self {
        Self {
            history: Vec::with_capacity(window),
            window,
        }
    }

    /// Record which neurons fired this tick.
    pub fn record_tick(&mut self, fired: &[NeuronId]) {
        self.history.push(fired.iter().copied().collect());
        if self.history.len() > self.window {
            self.history.remove(0);
        }
    }

    /// Get all neurons that fired in the last `window` ticks (including current).
    pub fn recent_firings(&self) -> HashSet<NeuronId> {
        self.history.iter().flatten().copied().collect()
    }

    /// Check if a neuron fired recently.
    pub fn fired_recently(&self, neuron_id: NeuronId) -> bool {
        self.history.iter().any(|h| h.contains(&neuron_id))
    }
}

/// Apply Hebbian learning to the network after one tick.
///
/// **Rule:** If neuron A fired this tick and neuron B fired within the
/// co-fire window (and B is a post-synaptic target of A), strengthen
/// the A→B synapse.
///
/// Conversely, if A fired but B has not fired recently, slightly weaken
/// the connection (synaptic decay).
///
/// The `synapses` map is mutated in place.
pub fn hebbian_update(
    neurons: &HashMap<NeuronId, Neuron>,
    synapses: &mut HashMap<NeuronId, Vec<super::neuron::Synapse>>,
    history: &FiringHistory,
    config: &LearningConfig,
) -> usize {
    if !config.enabled {
        return 0;
    }

    let mut changes = 0;

    for (pre_id, out_synapses) in synapses.iter_mut() {
        // Did this neuron fire this tick?
        let pre_fired_this_tick = neurons
            .get(pre_id)
            .map(|n| n.is_refractory() && n.activation >= n.threshold)
            .unwrap_or(false);

        if !pre_fired_this_tick {
            continue;
        }

        for synapse in out_synapses.iter_mut() {
            if synapse.plasticity < config.min_plasticity {
                continue;
            }

            let post_fired_recently = history.fired_recently(synapse.post_neuron_id);

            if post_fired_recently {
                // Hebbian potentiation: neurons that fire together wire together
                let delta = synapse.plasticity * (1.0 - synapse.strength);
                synapse.strength = (synapse.strength + delta).min(1.0);
                changes += 1;
            } else {
                // Synaptic decay: pre fired but post didn't — slightly weaken
                synapse.strength = (synapse.strength - config.synaptic_decay).max(0.01);
                changes += 1;
            }
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neuron::neuron::Neuron;

    #[test]
    fn test_firing_history() {
        let mut hist = FiringHistory::new(3);
        hist.record_tick(&[1, 2]);
        hist.record_tick(&[2, 3]);
        assert!(hist.fired_recently(1));
        assert!(hist.fired_recently(2));
        assert!(hist.fired_recently(3));

        hist.record_tick(&[4]);
        // Now tick 0's firing [1,2] falls out of window
        assert!(!hist.fired_recently(1));
        assert!(hist.fired_recently(4));
    }

    #[test]
    fn test_hebbian_strengthens() {
        let mut neurons = HashMap::new();
        let mut n1 = Neuron::new(1, "A");
        n1.activation = 0.9;
        n1.threshold = 0.5;
        n1.tick(); // Fires, enters refractory
        neurons.insert(1, n1);

        let mut synapses: HashMap<NeuronId, Vec<super::neuron::Synapse>> = HashMap::new();
        synapses.insert(
            1,
            vec![super::neuron::Synapse {
                post_neuron_id: 2,
                strength: 0.5,
                plasticity: 0.1,
            }],
        );

        let mut history = FiringHistory::new(5);
        history.record_tick(&[1, 2]); // Both fired

        let config = LearningConfig::default();
        let changes = hebbian_update(&neurons, &mut synapses, &history, &config);
        assert_eq!(changes, 1, "Should strengthen the 1→2 synapse");
        let strength = synapses[&1][0].strength;
        assert!(
            (strength - 0.55).abs() < 0.01,
            "Strength should increase from 0.5 toward 1.0: got {}",
            strength
        );
    }
}
