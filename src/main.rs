use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use clap::Parser;
use bionic_graph::config::load_or_create_settings;
use bionic_graph::graph_manager::GraphManager;
use bionic_graph::memory_system::MemorySystem;

/// Bionic-Graph: Ultral fast graph indexed with bionic neural net.
///
/// A low-cost AI memory system that caches knowledge graph structure in a
/// spreading-activation neural network for fast keyword-based retrieval.
/// Provides a Gremlin-compatible query interface via REST API.
#[derive(Parser, Debug)]
#[command(name = "bionic-graph", version, about)]
struct Args {
    /// Data directory for persistence (overrides settings.json)
    #[arg(short = 'd', long = "data-dir")]
    data_dir: Option<String>,

    /// Host to bind (overrides settings.json)
    #[arg(short = 'H', long = "host")]
    host: Option<String>,

    /// Port for the HTTP server (overrides settings.json)
    #[arg(short = 'P', long = "port")]
    port: Option<u16>,

    /// Path to settings.json (default: ~/.config/bionic-graph/settings.json)
    #[arg(long = "config")]
    config: Option<String>,

    /// Auto-index vertices by label on startup
    #[arg(short = 'i', long = "auto-index", default_value_t = true)]
    auto_index: bool,

    /// Disable auto-save background thread
    #[arg(long = "no-auto-save")]
    no_auto_save: bool,
}

#[tokio::main]
async fn main() {
    // Initialize logger
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    let args = Args::parse();

    // Load settings from config file (or create defaults)
    let mut settings = load_or_create_settings();

    // CLI args override settings
    if let Some(dir) = &args.data_dir {
        settings.storage.data_dir = dir.clone();
    }
    if let Some(h) = &args.host {
        settings.server.host = h.clone();
    }
    if let Some(p) = args.port {
        settings.server.port = p;
    }

    log::info!(
        "Starting Bionic-Graph — data: {}, listen: {}:{}",
        settings.storage.data_dir,
        settings.server.host,
        settings.server.port,
    );

    // Initialize GraphManager (scans/creates graphs under data dir)
    let graph_manager = GraphManager::open(&settings.storage.data_dir)
        .expect("Failed to initialize graph manager");

    // Auto-index all graphs if requested
    if args.auto_index {
        let names = graph_manager.list();
        for name in &names {
            if let Some(handle) = graph_manager.get(name) {
                let mut g = handle.graph.lock().unwrap();
                // Simple auto-index: create neurons from vertex labels
                let mut label_groups: std::collections::HashMap<String, Vec<u64>> =
                    std::collections::HashMap::new();
                for vid in g.vertex_ids() {
                    if let Some(v) = g.get_vertex(*vid) {
                        for label in &v.labels {
                            label_groups.entry(label.clone()).or_default().push(*vid);
                        }
                    }
                }
                drop(g);
                let mut nn = handle.neural_network.lock().unwrap();
                for (label, vrefs) in label_groups {
                    let nid = (nn.neuron_count() as u64) + 1;
                    let mut neuron = bionic_graph::neuron::Neuron::new(nid, &label)
                        .with_keywords(vec![label.clone()]);
                    neuron.vertex_refs = vrefs;
                    nn.add_neuron(neuron);
                }
                log::info!("Auto-indexed graph '{}' in neural network", name);
            }
        }
    }

    // Wrap graph_manager in Arc<Mutex<>>
    let gm = Arc::new(Mutex::new(graph_manager));

    // Start auto-save for all graphs
    if !args.no_auto_save {
        let bg_gm = gm.clone();
        let interval = settings.storage.auto_save_interval_secs;
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(interval));
                if let Ok(gm) = bg_gm.lock() {
                    gm.save_all();
                }
            }
        });
        log::info!("Auto-save background thread started for all graphs");
    }

    // Build the router
    let app = MemorySystem::into_router_with_manager(gm.clone());

    // Start server
    let addr: SocketAddr = format!("{}:{}", settings.server.host, settings.server.port)
        .parse()
        .expect("Invalid address");

    log_info_banner(&addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app)
        .await
        .expect("Server error");
}

fn log_info_banner(addr: &SocketAddr) {
    println!();
    println!("╔══════════════════════════════════════════════════╗");
    println!("║            Bionic-Graph v{}            ║", env!("CARGO_PKG_VERSION"));
    println!("║  Bio-inspired Neural Knowledge Graph             ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  HTTP server listening on: {addr:<15}    ║");
    println!("║                                                  ║");
    println!("║  Endpoints:                                      ║");
    println!("║    GET  /health   — System health                ║");
    println!("║    POST /gremlin  — Gremlin query                ║");
    println!("║    POST /search   — Neural keyword search        ║");
    println!("║    POST /vertices — Add vertex                   ║");
    println!("║    POST /edges    — Add edge                     ║");
    println!("║    POST /neurons  — Create neuron                ║");
    println!("║    POST /extract      — Submit document extraction   ║");
    println!("║    GET  /extract/task/:id — Poll extraction task      ║");
    println!("║    GET  /extract/tasks   — List all extraction tasks  ║");
    println!("╚══════════════════════════════════════════════════╝");
    println!();
}
