use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::config::Settings;
use std::path::PathBuf;

use crate::graph::{Graph, VertexId};
use crate::graph::graph::GraphError;
use crate::graph_manager::GraphManager;
use crate::gremlin::{
    build_router, execute_query, AppState, GremlinQuery, QueryResponse,
};
use crate::neuron::{NeuralNetwork, Neuron, NeuronId};
use crate::persistence::{
    self, auto_save, load_or_create, AutoSaveConfig, AutoSaveHandle,
};

/// The top-level memory system that integrates the knowledge graph,
/// neural index, persistence, and REST API into one unified interface.
pub struct MemorySystem {
    pub graph: Arc<Mutex<Graph>>,
    pub neural_network: Arc<Mutex<NeuralNetwork>>,
    auto_save: Option<AutoSaveHandle>,
    data_dir: String,
}

impl MemorySystem {
    /// Create a new memory system with the given data directory.
    /// If data files exist, they are loaded; otherwise fresh state is created.
    pub fn new(data_dir: impl Into<String>) -> Result<Self, persistence::StoreError> {
        let data_dir = data_dir.into();
        let path = Path::new(&data_dir);

        // Ensure data directory exists
        std::fs::create_dir_all(path).map_err(|e| persistence::StoreError::Io {
            source: e,
            description: format!("creating data directory {}", data_dir),
        })?;

        let config = AutoSaveConfig {
            graph_path: path.join("graph.bin"),
            neural_path: path.join("neural.bin"),
            ..Default::default()
        };

        let (graph, neural_network) = load_or_create(&config)?;

        let graph = Arc::new(Mutex::new(graph));
        let neural_network = Arc::new(Mutex::new(neural_network));

        Ok(Self {
            graph,
            neural_network,
            auto_save: None,
            data_dir,
        })
    }

    /// Start the auto-save background thread.
    pub fn start_auto_save(&mut self) {
        let config = AutoSaveConfig {
            graph_path: Path::new(&self.data_dir).join("graph.bin"),
            neural_path: Path::new(&self.data_dir).join("neural.bin"),
            ..Default::default()
        };

        let handle = auto_save::start_auto_save(
            self.graph.clone(),
            self.neural_network.clone(),
            config,
        );
        self.auto_save = Some(handle);
    }

    // ─── Graph Operations ─────────────────────────────────────────

    /// Add a vertex to the graph. Also creates a neuron for it.
    pub fn add_vertex(&self, labels: Vec<String>) -> VertexId {
        let vid = self.graph.lock().unwrap().create_vertex(labels.clone());
        let mut nn = self.neural_network.lock().unwrap();
        let nid = (nn.neuron_count() as u64) + 1;
        let label = labels.first().cloned().unwrap_or_else(|| "entity".to_string());
        let neuron = crate::neuron::neuron::Neuron::for_vertex(nid, &label, vid)
            .with_keywords(labels.clone());
        nn.add_neuron(neuron);
        vid
    }

    /// Add an edge between two vertices. Also creates a neuron + synapses.
    pub fn add_edge(
        &self,
        label: String,
        source: VertexId,
        target: VertexId,
    ) -> Result<u64, crate::graph::graph::GraphError> {
        let eid = self.graph.lock().unwrap().create_edge(label.clone(), source, target)?;
        let mut nn = self.neural_network.lock().unwrap();
        let nid = (nn.neuron_count() as u64) + 1;
        let neuron = crate::neuron::neuron::Neuron::for_edge(nid, &label, eid)
            .with_keywords(vec![label.clone()]);
        nn.add_neuron(neuron);
        nn.auto_synapse(source, target);
        Ok(eid)
    }

    /// Get a vertex by ID.
    pub fn get_vertex(&self, id: VertexId) -> Option<crate::graph::Vertex> {
        self.graph
            .lock()
            .unwrap()
            .get_vertex(id)
            .cloned()
    }

    /// Get graph statistics.
    pub fn graph_stats(&self) -> (usize, usize) {
        let g = self.graph.lock().unwrap();
        (g.vertex_count(), g.edge_count())
    }

    // ─── Neural Index Operations ──────────────────────────────────

    /// Create a neuron in the neural index.
    pub fn create_neuron(
        &self,
        label: impl Into<String>,
        keywords: Vec<String>,
    ) -> NeuronId {
        let mut nn = self.neural_network.lock().unwrap();
        let id = (nn.neuron_count() as u64) + 1;
        let neuron = Neuron::new(id, label).with_keywords(keywords);
        nn.add_neuron(neuron);
        id
    }

    /// Link a neuron to a graph vertex.
    pub fn link_neuron(&self, neuron_id: NeuronId, vertex_id: VertexId) {
        self.neural_network
            .lock()
            .unwrap()
            .link_vertex(neuron_id, vertex_id);
    }

    /// Add a synapse between two neurons.
    pub fn add_synapse(
        &self,
        pre_id: NeuronId,
        post_id: NeuronId,
        strength: f32,
        plasticity: f32,
    ) -> Option<()> {
        self.neural_network
            .lock()
            .unwrap()
            .add_synapse(pre_id, post_id, strength, plasticity)
    }

    /// Auto-index vertices into the neural network based on their labels.
    ///
    /// Creates one neuron per unique vertex label, linking all vertices
    /// with that label to the neuron.
    pub fn auto_index_by_label(&self) -> usize {
        let g = self.graph.lock().unwrap();
        // Collect all labels and their vertex IDs
        let mut label_groups: std::collections::HashMap<String, Vec<VertexId>> =
            std::collections::HashMap::new();

        for vid in g.vertex_ids() {
            if let Some(v) = g.get_vertex(*vid) {
                for label in &v.labels {
                    label_groups
                        .entry(label.clone())
                        .or_default()
                        .push(*vid);
                }
            }
        }
        drop(g); // Release graph lock before acquiring neural network lock

        let mut nn = self.neural_network.lock().unwrap();
        let mut count = 0;

        for (label, vrefs) in label_groups {
            let id = (nn.neuron_count() as u64) + 1;
            let mut neuron = Neuron::new(id, &label)
                .with_keywords(vec![label.clone()]);
            neuron.vertex_refs = vrefs;
            nn.add_neuron(neuron);
            count += 1;
        }

        count
    }

    /// Search the neural index and return ranked graph vertices.
    pub fn search(&self, query: &str) -> QueryResponse {
        let gremlin_query = GremlinQuery::new(vec![
            crate::gremlin::query::TraversalStep::Search {
                keywords: query
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect(),
            },
        ]);

        execute_query(&self.graph, &self.neural_network, &gremlin_query)
    }

    /// Execute a full Gremlin pipeline query.
    pub fn query_gremlin(&self, query: &GremlinQuery) -> QueryResponse {
        execute_query(&self.graph, &self.neural_network, query)
    }

    /// Get neural network statistics.
    pub fn neural_stats(&self) -> (usize, u64) {
        let nn = self.neural_network.lock().unwrap();
        (nn.neuron_count(), nn.total_ticks())
    }

    // ─── Server ───────────────────────────────────────────────────

    /// Build the REST API router using GraphManager (multi-graph).
    pub fn into_router_with_manager(gm: Arc<Mutex<GraphManager>>) -> axum::Router {
        let state = AppState {
            graph_manager: gm,
            document_manager: crate::documents::DocumentManager::new("data"),
        };
        build_router(state)
    }

    /// Build the REST API router (single graph, backward compat).
    pub fn into_router(self) -> axum::Router {
        Self::into_router_with_settings_v2(
            self.graph.clone(),
            self.neural_network.clone(),
            self.data_dir.clone(),
        )
    }

    /// Build router with settings (single graph, backward compat).
    pub fn into_router_with_settings(self, _settings: Settings) -> axum::Router {
        Self::into_router_with_settings_v2(
            self.graph.clone(),
            self.neural_network.clone(),
            self.data_dir.clone(),
        )
    }

    /// Internal: wrap a single graph into a GraphManager and build router.
    fn into_router_with_settings_v2(
        graph: Arc<Mutex<Graph>>,
        neural_network: Arc<Mutex<NeuralNetwork>>,
        data_dir: impl Into<PathBuf>,
    ) -> axum::Router {
        use crate::graph_manager::GraphHandle;
        let mut gm = GraphManager::empty(data_dir);
        gm.insert("default".to_string(), GraphHandle {
            name: "default".to_string(),
            graph,
            neural_network,
            data_dir: PathBuf::new(),
        });
        let state = AppState {
            graph_manager: Arc::new(Mutex::new(gm)),
            document_manager: crate::documents::DocumentManager::new("data"),
        };
        build_router(state)
    }

    /// Save state immediately (blocking).
    pub fn save_now(&self) -> Result<(), persistence::StoreError> {
        let config = AutoSaveConfig {
            graph_path: Path::new(&self.data_dir).join("graph.bin"),
            neural_path: Path::new(&self.data_dir).join("neural.bin"),
            ..Default::default()
        };

        {
            let g = self.graph.lock().unwrap();
            persistence::graph_store::save_graph(&g, &config.graph_path)?;
        }

        {
            let mut nn = self.neural_network.lock().unwrap();
            if nn.is_dirty() {
                persistence::neuron_store::save_neural_network(&nn, &config.neural_path)?;
                nn.mark_clean();
            }
        }

        Ok(())
    }
}

impl Drop for MemorySystem {
    fn drop(&mut self) {
        // Auto-save handle's Drop will signal shutdown
    }
}
