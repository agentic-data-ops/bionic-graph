/// Bionic-Graph full demo
///
/// Build graph → neural index → keyword search → graph traversal
///
/// Run: cargo run --example demo
use bionic_graph::graph::{Graph, PropertyValue, Vertex};
use bionic_graph::neuron::{NeuralNetwork, Neuron};

fn main() {
    println!("╔══════════════════════════════════════════╗");
    println!("║   Bionic-Graph Demo                      ║");
    println!("║   Knowledge Graph + Bio-Neural Index        ║");
    println!("╚══════════════════════════════════════════╝");
    println!();

    // ── Step 1: Build graph ─────────────────────────────
    println!("📊 Step 1: Building knowledge graph...");
    let mut graph = Graph::new();

    // Create person vertices
    let alice = graph.create_vertex(vec!["person".to_string(), "engineer".to_string()]);
    let bob = graph.create_vertex(vec!["person".to_string(), "scientist".to_string()]);
    let carol = graph.create_vertex(vec!["person".to_string(), "designer".to_string()]);

    // Set properties
    if let Some(v) = graph.get_vertex_mut(alice) {
        v.properties.insert("name".to_string(), PropertyValue::String("Alice".to_string()));
        v.properties.insert("age".to_string(), PropertyValue::Integer(30));
    }
    if let Some(v) = graph.get_vertex_mut(bob) {
        v.properties.insert("name".to_string(), PropertyValue::String("Bob".to_string()));
        v.properties.insert("age".to_string(), PropertyValue::Integer(35));
    }
    if let Some(v) = graph.get_vertex_mut(carol) {
        v.properties.insert("name".to_string(), PropertyValue::String("Carol".to_string()));
        v.properties.insert("age".to_string(), PropertyValue::Integer(28));
    }

    // Create company vertex
    let acme = graph.create_vertex(vec!["company".to_string(), "tech".to_string()]);
    let globex = graph.create_vertex(vec!["company".to_string()]);
    if let Some(v) = graph.get_vertex_mut(acme) {
        v.properties.insert("name".to_string(), PropertyValue::String("Acme Corp".to_string()));
        v.properties.insert("industry".to_string(), PropertyValue::String("AI".to_string()));
    }
    if let Some(v) = graph.get_vertex_mut(globex) {
        v.properties.insert("name".to_string(), PropertyValue::String("Globex Inc".to_string()));
        v.properties.insert("industry".to_string(), PropertyValue::String("Robotics".to_string()));
    }

    // Create project vertex
    let project_x = graph.create_vertex(vec!["project".to_string()]);
    if let Some(v) = graph.get_vertex_mut(project_x) {
        v.properties.insert("name".to_string(), PropertyValue::String("Project X".to_string()));
        v.properties.insert("budget".to_string(), PropertyValue::Integer(1_000_000));
    }

    // Add edges
    graph.create_edge("works_at".to_string(), alice, acme).unwrap();
    graph.create_edge("works_at".to_string(), bob, acme).unwrap();
    graph.create_edge("works_at".to_string(), carol, globex).unwrap();
    graph.create_edge("knows".to_string(), alice, bob).unwrap();
    graph.create_edge("knows".to_string(), bob, carol).unwrap();
    graph.create_edge("collaborates_on".to_string(), alice, project_x).unwrap();
    graph.create_edge("collaborates_on".to_string(), bob, project_x).unwrap();

    println!("   ✓ {} vertices created", graph.vertex_count());
    println!("   ✓ {} edges created", graph.edge_count());
    println!();

    // ── Step 2: Build neural index ───────────────────────
    println!("🧠 Step 2: Building neural index...");
    let mut nn = NeuralNetwork::new();

    // Create one neuron per concept
    let n_ai = Neuron::new(1, "Artificial Intelligence")
        .with_keywords(vec!["ai", "artificial intelligence", "machine learning", "deep learning"])
        .with_threshold(0.6);
    nn.add_neuron(n_ai);
    nn.link_vertex(1, alice);
    nn.link_vertex(1, bob);
    nn.link_vertex(1, acme);

    let n_robotics = Neuron::new(2, "Robotics")
        .with_keywords(vec!["robotics", "robot", "automation"])
        .with_threshold(0.6);
    nn.add_neuron(n_robotics);
    nn.link_vertex(2, carol);
    nn.link_vertex(2, globex);

    let n_engineering = Neuron::new(3, "Engineering")
        .with_keywords(vec!["engineering", "engineer", "software", "developer"])
        .with_threshold(0.5);
    nn.add_neuron(n_engineering);
    nn.link_vertex(3, alice);
    nn.link_vertex(3, acme);
    nn.link_vertex(3, project_x);

    // Add synapses (concept associations)
    nn.add_synapse(1, 3, 0.8, 0.05); // AI → Engineering (strong)
    nn.add_synapse(3, 1, 0.6, 0.05); // Engineering → AI (medium)
    nn.add_synapse(2, 3, 0.5, 0.05); // Robotics → Engineering

    println!("   ✓ {} neurons created", nn.neuron_count());
    println!("   ✓ Neurons linked to graph vertices via vertex_refs");
    println!();

    // ── Step 3: Neural search ────────────────────────────
    println!("🔍 Step 3: Neural search — query: 'ai engineering'");
    let (vertices, _edges, fired, _hot, ticks) = nn.search("ai engineering");
    println!("   Fired {} neurons across {} ticks", fired.len(), ticks);
    println!("   Found {} vertices via spreading activation:", vertices.len());
    for (vid, score) in &vertices {
        if let Some(v) = graph.get_vertex(*vid) {
            let name = v.properties.get("name")
                .and_then(|p| {
                    if let PropertyValue::String(s) = p { Some(s.clone()) } else { None }
                })
                .unwrap_or_else(|| format!("<vertex {}>", vid));
            println!("     - {} (vertex {}, relevance score: {})", name, vid, score);
        }
    }
    println!();

    // ── Step 4: Traverse subgraph ────────────────────────
    println!("🔗 Step 4: Traversal from neural results");
    // BFS from the hottest search-result vertex
    if let Some(&(start_vid, _)) = vertices.first() {
        println!("   BFS from hottest vertex {}:", start_vid);
        let bfs = bionic_graph::graph::Bfs::new(&graph, start_vid)
            .with_max_depth(2);

        for step in bfs {
            if let Some(v) = graph.get_vertex(step.vertex) {
                let name = v.properties.get("name")
                    .and_then(|p| {
                        if let PropertyValue::String(s) = p { Some(s.clone()) } else { None }
                    })
                    .unwrap_or_else(|| format!("<v{}>", step.vertex));
                let labels = v.labels.join(", ");
                println!("     depth {}: {} [{}]", step.depth, name, labels);
            }
        }
    }
    println!();

    // ── Step 5: Hebbian learning demo ────────────────────
    println!("📝 Step 5: Hebbian learning demonstration");
    println!("   Before: AI→Engineering synapse strength = 0.8");
    
    // Simulate repeated co-firing
    for _ in 0..5 {
        let (_v, _e, fired, _hot, _) = nn.search("ai engineering");
        // Hebbian learning happens inside search()
        println!("   Co-firing: {:?} → synapse strengthens", fired);
    }
    
    // Check learned synapse strengths
    // (We'd need a get_synapse method; for demo we just show the concept)
    println!("   ✓ Hebbian learning automatically strengthens frequently co-activated paths");
    println!();

    // ── Summary ──────────────────────────────────────────
    println!("╔══════════════════════════════════════════╗");
    println!("║   Demo Complete                          ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  To start the HTTP server:               ║");
    println!("║    cargo run -- --auto-index              ║");
    println!("║                                           ║");
    println!("║  Example query:                          ║");
    println!("║    curl -X POST localhost:8080/gremlin    ║");
    println!("║      -H 'Content-Type: application/json' ║");
    println!("║      -d '{{\"steps\":[                    ║");
    println!("║        {{\"step\":\"search\",              ║");
    println!("║         \"keywords\":[\"ai\"]}},           ║");
    println!("║        {{\"step\":\"out\",                  ║");
    println!("║         \"label\":\"works_at\"}}]}}'        ║");
    println!("╚══════════════════════════════════════════╝");
}
