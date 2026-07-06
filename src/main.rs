use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use bionic_graph::config::load_or_create_settings;
use bionic_graph::gremlin::build_router as build_new_router;
use bionic_graph::graph_manager2::GraphManager2;

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
    let mut settings = load_or_create_settings();

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
        "Starting Bionic-Graph (new engine) — data: {}, listen: {}:{}",
        settings.storage.data_dir,
        settings.server.host,
        settings.server.port,
    );

    // Initialize the new block-based graph manager.
    let data_dir = std::path::PathBuf::from(&settings.storage.data_dir);
    let gm = GraphManager2::new(data_dir);

    // Build the new router.
    let app = build_new_router(gm);

    let addr: SocketAddr = format!("{}:{}", settings.server.host, settings.server.port)
        .parse()
        .expect("Invalid address");

    log_info_banner(&addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
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
        })
        .await
        .expect("Server error");

    log::info!("Server shut down. Goodbye.");
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
