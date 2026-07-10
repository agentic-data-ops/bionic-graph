use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use bionic_graph::cluster::node::NodeRegistry;
use bionic_graph::cluster::server::{build_cluster_router, ClusterAppState};
use bionic_graph::config::load_or_create_settings_from;
use bionic_graph::config::NodeRole;
use bionic_graph::gremlin::build_router as build_new_router;
use bionic_graph::graph_manager::GraphManager;

/// Bionic-Graph: Block-based knowledge graph with token-indexed search.
///
/// A high-performance graph database with:
/// - Block-based storage engine (16 KB blocks, LRU cache, WAL)
/// - Token-indexed search (replaces old neuron activation network)
/// - Gremlin-compatible query pipeline
/// - MaaS proxy for LLM integration
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
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    let args = Args::parse();
    let mut settings = load_or_create_settings_from(args.config.map(std::path::PathBuf::from));

    if let Some(dir) = &args.data_dir {
        settings.storage.data_dir = dir.clone();
    }
    if let Some(h) = &args.host {
        settings.server.host = h.clone();
    }
    if let Some(p) = args.port {
        settings.server.port = p;
    }

    // Shared shutdown signal for all servers.
    let shutdown = Arc::new(tokio::sync::Notify::new());

    // Signal handler task — notifies shutdown on SIGINT / SIGTERM.
    {
        let sig = shutdown.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                let mut term = tokio::signal::unix::signal(
                    tokio::signal::unix::SignalKind::terminate(),
                )
                .expect("Failed to install SIGTERM handler");
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {},
                    _ = term.recv() => {},
                }
            }
            #[cfg(not(unix))]
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
            log::info!("Shutdown signal received — finishing requests...");
            sig.notify_one();
        });
    }

    log::info!(
        "Starting Bionic-Graph (new engine) — data: {}, listen: {}:{}",
        settings.storage.data_dir,
        settings.server.host,
        settings.server.port,
    );

    // Initialize the new block-based graph manager.
    let data_dir = std::path::PathBuf::from(&settings.storage.data_dir);
    let gm = Arc::new(GraphManager::new(data_dir));

    // Ensure the default graph "graph0" exists on first startup.
    if let Err(e) = gm.get("graph0") {
        log::warn!("Failed to create default graph 'graph0': {}", e);
    }

    // Create cluster registry early so it can be shared between
    // the main API server and the cluster communication server.
    let cluster_registry: Option<Arc<bionic_graph::cluster::node::NodeRegistry>> =
        if settings.cluster.enabled {
            Some(Arc::new(bionic_graph::cluster::node::NodeRegistry::new(&settings.cluster)))
        } else {
            None
        };

    // ── Main API server ──────────────────────────────────────────────────────
    let app = build_new_router(gm.clone(), settings.clone(), cluster_registry.clone());

    let api_addr: SocketAddr = format!("{}:{}", settings.server.host, settings.server.port)
        .parse()
        .expect("Invalid address");

    log_info_banner(&api_addr);

    let api_listener = tokio::net::TcpListener::bind(api_addr)
        .await
        .expect("Failed to bind API address");

    let main_shutdown = shutdown.clone();
    let api_server = async {
        axum::serve(api_listener, app)
            .with_graceful_shutdown(async move { main_shutdown.notified().await })
            .await
            .expect("Main server error");
    };

    // ── Cluster server ───────────────────────────────────────────────────────
    let is_master = settings.cluster.role == NodeRole::Master;

    let cluster_server = async {
        if !settings.cluster.enabled {
            return;
        }

        log::info!(
            "Cluster mode enabled — role: {}, bind: {}",
            if is_master { "master" } else { "worker" },
            settings.cluster.bind_addr,
        );

        let registry = cluster_registry.clone().unwrap();
        let api_addr_str = format!("{}:{}", settings.server.host, settings.server.port);
        let cluster_state = ClusterAppState {
            gm: gm.clone(),
            registry: registry.clone(),
            is_master,
            api_addr: api_addr_str,
        };
        let cluster_router = build_cluster_router(cluster_state);

        let cluster_addr: SocketAddr = settings
            .cluster
            .bind_addr
            .parse()
            .expect("Invalid cluster bind address");

        let cluster_listener = tokio::net::TcpListener::bind(cluster_addr)
            .await
            .expect("Failed to bind cluster address");

        // If master, spawn heartbeat cleanup task.
        if is_master {
            let reg = registry.clone();
            let interval = Duration::from_secs(settings.cluster.heartbeat_interval_secs);
            let sig = shutdown.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(interval) => {
                            let expired = reg.purge_expired();
                            if !expired.is_empty() {
                                log::info!("Purged {} expired worker(s): {:?}", expired.len(), expired);
                            }
                        }
                        _ = sig.notified() => break,
                    }
                }
            });
        }

        // If worker, start heartbeat sender.
        if !is_master {
            if let Some(ref master_addr) = settings.cluster.master_addr {
                let master_cluster = master_addr.clone();
                let interval = Duration::from_secs(settings.cluster.heartbeat_interval_secs);
                let sig = shutdown.clone();
                tokio::spawn(async move {
                    let client = reqwest::Client::new();
                    loop {
                        let heartbeat = bionic_graph::cluster::node::ClusterMessage::Heartbeat {
                            node_id: "worker".to_string(),
                            api_addr: settings.server.host.clone(),
                            cluster_addr: settings.cluster.bind_addr.clone(),
                            last_acked_seq: 0,
                        };
                        let url = format!("http://{}/cluster/heartbeat", master_cluster);
                        if let Err(e) = client
                            .post(&url)
                            .json(&heartbeat)
                            .send()
                            .await
                        {
                            log::warn!("Heartbeat to master failed: {}", e);
                        }
                        tokio::select! {
                            _ = tokio::time::sleep(interval) => {},
                            _ = sig.notified() => break,
                        }
                    }
                });
            }
        }

        let cluster_shutdown = shutdown.clone();
        axum::serve(cluster_listener, cluster_router)
            .with_graceful_shutdown(async move { cluster_shutdown.notified().await })
            .await
            .expect("Cluster server error");
    };

    // Start rank decay background task (if enabled).
    if let Ok(default_graph) = gm.get("graph0") {
        bionic_graph::graph::rank_decay::spawn_rank_decay(
            default_graph,
            settings.rank.auto_dec_rank_when_inactive,
            settings.rank.inactive_after_accessed_secs,
            settings.rank.inactive_rank_update_period,
        );
    }

    // Run both servers concurrently.
    tokio::join!(api_server, cluster_server);

    log::info!("Server shut down. Goodbye.");

    // Flush and checkpoint all graphs before exiting.
    gm.close_all();
    log::info!("All graphs flushed and checkpointed.");
}

fn log_info_banner(addr: &SocketAddr) {
    println!();
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║                 Bionic-Graph v{:<13} ║", env!("CARGO_PKG_VERSION"));
    println!("║            Block-based Knowledge Graph                  ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  HTTP server → {addr:<21}            ║");
    println!("║                                                          ║");
    println!("║  Knowledge Graph API                                     ║");
    println!("║    POST /gremlin      Gremlin pipeline query             ║");
    println!("║    GET  /search       Keyword search                     ║");
    println!("║    POST /vertices     Add vertex                         ║");
    println!("║    POST /edges        Add edge                           ║");
    println!("║    GET  /graphs       List graphs                        ║");
    println!("║                                                          ║");
    println!("║  Settings                                                 ║");
    println!("║    GET/PUT /settings/Search    Search/explore config     ║");
    println!("║    GET/PUT /settings/neural    Legacy compat              ║");
    println!("║    GET/PUT /settings/llm       LLM provider config        ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();
}
