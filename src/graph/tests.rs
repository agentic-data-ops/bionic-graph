use std::collections::HashMap;
use std::sync::Arc;
use crate::graph::crud;
use crate::graph::graph::{Graph, GraphConfig};
use crate::graph::gremlin::{execute_gremlin, GremlinQuery, GremlinResult, GremlinStep};
use crate::graph::locked;
use crate::storage::types::PropertyValue;

fn setup_graph() -> (Arc<Graph>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let g = Graph::open(dir.path(), "test").unwrap();
    (g, dir)
}

fn run_steps(g: &Arc<Graph>, steps: Vec<GremlinStep>) -> Vec<GremlinResult> {
    let q = GremlinQuery { steps };
    let r = execute_gremlin(g, &q);
    assert!(r.success, "gremlin failed: {:?}", r.error);
    r.data
}

fn vids(r: &[GremlinResult]) -> Vec<u32> {
    r.iter().filter_map(|x| if let GremlinResult::Vertex { id, .. } = x { Some(*id) } else { None }).collect()
}

fn eids(r: &[GremlinResult]) -> Vec<u32> {
    r.iter().filter_map(|x| if let GremlinResult::Edge { id, .. } = x { Some(*id) } else { None }).collect()
}

fn props(kvs: &[(&str, PropertyValue)]) -> HashMap<String, PropertyValue> {
    kvs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

// ── T3.1: Vertex CRUD ───────────────────────────────────────────────────────

#[test]
fn vertex_create_read() {
    let (g, _) = setup_graph();
    let vid = crud::create_vertex(&g, "Alice", &["person".into()], &["alice".into()],
        &props(&[("age", PropertyValue::Integer(30))])).unwrap();
    let v = crud::get_vertex(&g, vid).unwrap().unwrap();
    assert_eq!(v.name, "Alice");
    assert_eq!(v.properties.get("age").unwrap(), &PropertyValue::Integer(30));
}

#[test]
fn vertex_multiple() {
    let (g, _) = setup_graph();
    let a = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let b = crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    let ids = vids(&run_steps(&g, vec![GremlinStep::V { ids: None, at: None }]));
    assert!(ids.contains(&a) && ids.contains(&b) && ids.len() == 2);
}

#[test]
fn vertex_update() {
    let (g, _) = setup_graph();
    let vid = crud::create_vertex(&g, "Alice", &[], &[], &HashMap::new()).unwrap();
    crud::update_vertex(&g, vid, Some("Alice2"), None, None, None, true).unwrap();
    assert_eq!(crud::get_vertex(&g, vid).unwrap().unwrap().name, "Alice2");
}

#[test]
fn vertex_soft_delete() {
    let (g, _) = setup_graph();
    let vid = crud::create_vertex(&g, "X", &[], &[], &HashMap::new()).unwrap();
    crud::soft_delete_vertex(&g, vid).unwrap();
    assert!(crud::get_vertex(&g, vid).unwrap().is_none());
}

#[test]
fn vertex_hard_delete() {
    let (g, _) = setup_graph();
    let vid = crud::create_vertex(&g, "X", &[], &[], &HashMap::new()).unwrap();
    crud::hard_delete_vertex(&g, vid).unwrap();
    assert!(crud::get_vertex(&g, vid).unwrap().is_none());
}

#[test]
fn vertex_not_found() {
    let (g, _) = setup_graph();
    assert!(crud::get_vertex(&g, 99999).unwrap().is_none());
}

// ── T3.2: Edge CRUD ─────────────────────────────────────────────────────────

#[test]
fn edge_create_read() {
    let (g, _) = setup_graph();
    let a = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let b = crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    let eid = crud::create_edge(&g, a, b, "knows", &[], &[], 0.9,
        &props(&[("since", PropertyValue::Integer(2020))])).unwrap();
    let e = crud::get_edge(&g, eid).unwrap().unwrap();
    assert_eq!(e.name, "knows");
    assert_eq!(e.source, a);
    assert_eq!(e.target, b);
}

#[test]
fn edge_update() {
    let (g, _) = setup_graph();
    let a = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let b = crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    let eid = crud::create_edge(&g, a, b, "x", &[], &[], 0.5, &HashMap::new()).unwrap();
    crud::update_edge(&g, eid, Some("y"), None, None, Some(0.9), None, true).unwrap();
    let e = crud::get_edge(&g, eid).unwrap().unwrap();
    assert_eq!(e.name, "y");
    assert!((e.strength - 0.9).abs() < 0.001);
}

#[test]
fn edge_soft_delete() {
    let (g, _) = setup_graph();
    let a = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let b = crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    let eid = crud::create_edge(&g, a, b, "x", &[], &[], 0.5, &HashMap::new()).unwrap();
    crud::soft_delete_edge(&g, eid).unwrap();
    assert!(crud::get_edge(&g, eid).unwrap().is_none());
}

// ── T4.1: Gremlin Basic Steps ──────────────────────────────────────────────

#[test]
fn gremlin_v_all() {
    let (g, _) = setup_graph();
    crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    assert_eq!(run_steps(&g, vec![GremlinStep::V { ids: None, at: None }]).len(), 2);
}

#[test]
fn gremlin_v_by_id() {
    let (g, _) = setup_graph();
    let vid = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let r = run_steps(&g, vec![GremlinStep::V { ids: Some(vec![vid]), at: None }]);
    assert_eq!(r.len(), 1);
}

#[test]
fn gremlin_count() {
    let (g, _) = setup_graph();
    crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let r = run_steps(&g, vec![GremlinStep::V { ids: None, at: None }, GremlinStep::Count]);
    if let GremlinResult::Count { count } = &r[0] { assert_eq!(*count, 1); } else { panic!(); }
}

#[test]
fn gremlin_has_label() {
    let (g, _) = setup_graph();
    crud::create_vertex(&g, "A", &["person".into()], &[], &HashMap::new()).unwrap();
    crud::create_vertex(&g, "B", &["animal".into()], &[], &HashMap::new()).unwrap();
    let r = run_steps(&g, vec![
        GremlinStep::V { ids: None, at: None },
        GremlinStep::HasLabel { label: "person".into() },
    ]);
    assert_eq!(r.len(), 1);
}

// ── T4.2: Traversal ─────────────────────────────────────────────────────────

#[test]
fn traversal_out() {
    let (g, _) = setup_graph();
    let a = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let b = crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    crud::create_edge(&g, a, b, "e", &[], &[], 0.9, &HashMap::new()).unwrap();
    let ids = vids(&run_steps(&g, vec![
        GremlinStep::V { ids: Some(vec![a]), at: None },
        GremlinStep::Out { depth: None, labels: None },
    ]));
    assert!(ids.contains(&b));
}

#[test]
fn traversal_in() {
    let (g, _) = setup_graph();
    let a = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let b = crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    crud::create_edge(&g, a, b, "e", &[], &[], 0.9, &HashMap::new()).unwrap();
    let ids = vids(&run_steps(&g, vec![
        GremlinStep::V { ids: Some(vec![b]), at: None },
        GremlinStep::In { depth: None, labels: None },
    ]));
    assert!(ids.contains(&a));
}

#[test]
fn traversal_expand() {
    let (g, _) = setup_graph();
    let a = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let b = crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    crud::create_edge(&g, a, b, "e", &[], &[], 0.9, &HashMap::new()).unwrap();
    let r = run_steps(&g, vec![
        GremlinStep::V { ids: Some(vec![a]), at: None },
        GremlinStep::Expand { depth: None, label: None },
    ]);
    assert!(vids(&r).contains(&a) && vids(&r).contains(&b));
    assert_eq!(eids(&r).len(), 1);
}

// ── T4.3: Search ────────────────────────────────────────────────────────────

#[test]
fn search_english() {
    let (g, _) = setup_graph();
    crud::create_vertex(&g, "Alice", &[], &["alice".into()], &HashMap::new()).unwrap();
    let r = run_steps(&g, vec![GremlinStep::Search {
        text: "alice".into(), mode: Some("greedy".into()), match_mode: None,
        at: None, limit: None, min_rank: None,
    }]);
    assert!(!r.is_empty());
}

#[test]
fn search_greedy_exact() {
    let (g, _) = setup_graph();
    crud::create_vertex(&g, "Alice", &[], &["alice".into()], &HashMap::new()).unwrap();
    let gr = run_steps(&g, vec![GremlinStep::Search {
        text: "alice missing".into(), mode: Some("greedy".into()), match_mode: None,
        at: None, limit: None, min_rank: None,
    }]);
    assert!(!gr.is_empty(), "greedy should match partial");
    let er = run_steps(&g, vec![GremlinStep::Search {
        text: "alice missing".into(), mode: Some("exact".into()), match_mode: None,
        at: None, limit: None, min_rank: None,
    }]);
    assert!(er.is_empty(), "exact should not match missing keyword");
}

#[test]
fn search_cjk() {
    let (g, _) = setup_graph();
    crud::create_vertex(&g, "张三", &[], &["工程师".into()], &HashMap::new()).unwrap();
    let r = run_steps(&g, vec![GremlinStep::Search {
        text: "张三".into(), mode: Some("greedy".into()), match_mode: None,
        at: None, limit: None, min_rank: None,
    }]);
    assert!(!r.is_empty(), "CJK search should find 张三");
}

// ── T4.4: Activation ────────────────────────────────────────────────────────

#[test]
fn activate_basic() {
    let (g, _) = setup_graph();
    let a = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let b = crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    crud::create_edge(&g, a, b, "e", &[], &[], 0.8, &HashMap::new()).unwrap();
    let r = run_steps(&g, vec![
        GremlinStep::V { ids: Some(vec![a]), at: None },
        GremlinStep::Traverse { decay: Some(1.0), activate: Some(0.0), max_depth: Some(1), min_score: Some(0.0) },
    ]);
    assert_eq!(vids(&r).len(), 2, "A + B");
}

#[test]
fn activate_depth() {
    let (g, _) = setup_graph();
    let a = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let b = crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    let c = crud::create_vertex(&g, "C", &[], &[], &HashMap::new()).unwrap();
    crud::create_edge(&g, a, b, "e", &[], &[], 0.8, &HashMap::new()).unwrap();
    crud::create_edge(&g, b, c, "e", &[], &[], 0.6, &HashMap::new()).unwrap();
    assert_eq!(vids(&run_steps(&g, vec![
        GremlinStep::V { ids: Some(vec![a]), at: None },
        GremlinStep::Traverse { decay: Some(1.0), activate: Some(0.0), max_depth: Some(1), min_score: Some(0.0) },
    ])).len(), 2, "depth=1: A+B");
    assert_eq!(vids(&run_steps(&g, vec![
        GremlinStep::V { ids: Some(vec![a]), at: None },
        GremlinStep::Traverse { decay: Some(1.0), activate: Some(0.0), max_depth: Some(2), min_score: Some(0.0) },
    ])).len(), 3, "depth=2: A+B+C");
}

#[test]
fn activate_min_score() {
    let (g, _) = setup_graph();
    let a = crud::create_vertex(&g, "A", &[], &[], &HashMap::new()).unwrap();
    let b = crud::create_vertex(&g, "B", &[], &[], &HashMap::new()).unwrap();
    crud::create_edge(&g, a, b, "e", &[], &[], 0.5, &HashMap::new()).unwrap();
    let r = run_steps(&g, vec![
        GremlinStep::V { ids: Some(vec![a]), at: None },
        GremlinStep::Traverse { decay: Some(1.0), activate: Some(0.0), max_depth: Some(1), min_score: Some(0.6) },
    ]);
    assert_eq!(vids(&r).len(), 1, "B score=0.5 should be filtered by min_score=0.6");
}

// ── T6.1: Data Persistence ──────────────────────────────────────────────────

#[test]
fn data_persistence() {
    let dir = tempfile::tempdir().unwrap();
    {
        let g = Graph::open(dir.path(), "test").unwrap();
        let vid = crud::create_vertex(&g, "Alice", &["person".into()], &["alice".into()],
            &props(&[("age", PropertyValue::Integer(30))])).unwrap();
        crud::create_edge(&g, vid, vid, "self", &[], &[], 1.0, &HashMap::new()).unwrap();
        g.close().unwrap();
    }
    {
        let g = Graph::open(dir.path(), "test").unwrap();
        let r = run_steps(&g, vec![GremlinStep::V { ids: None, at: None }]);
        assert_eq!(r.len(), 1);
        if let GremlinResult::Vertex { name, properties, .. } = &r[0] {
            assert_eq!(name, "Alice");
            assert_eq!(properties.get("age").unwrap(), &PropertyValue::Integer(30));
        } else { panic!("expected vertex"); }
    }
}

// ── Locked wrappers ─────────────────────────────────────────────────────────

#[test]
fn locked_crud() {
    let (g, _) = setup_graph();
    let vid = locked::create_vertex_locked(&g, "Alice", &[], &[], &HashMap::new()).unwrap();
    assert_eq!(locked::get_vertex_locked(&g, vid).unwrap().unwrap().name, "Alice");
}
