//! Crash consistency tests for WAL batch mode.
//!
//! These tests verify that the graph engine remains internally consistent
//! after simulated crashes, especially when WAL batch mode is enabled.
//! The key risk: during batch mode, WAL entries are buffered in memory.
//! If a crash occurs before `end_batch()` flushes them, the data blocks
//! on disk must still form a self-consistent state.

use std::collections::HashMap;
use std::sync::Arc;

use bionic_graph::graph::batch::{self, BatchEntity, BatchRelation};
use bionic_graph::graph::crud;
use bionic_graph::graph::graph::Graph;
use bionic_graph::graph::gremlin::{execute_gremlin, GremlinQuery, GremlinResult, GremlinStep};
use bionic_graph::storage::types::PropertyValue;

/// Helper: count Gremlin results.
fn count_results(r: &[GremlinResult]) -> usize {
    r.len()
}

// ── Test 1: Proper shutdown + WAL loss ────────────────────────────────
//
// Scenario: write data normally, close properly, delete WAL files,
// reopen. Verifies the data file is self-consistent (memory index rebuild
// works correctly from raw data blocks).
#[test]
fn proper_shutdown_then_wal_loss() {
    let dir = tempfile::tempdir().unwrap();
    let graph_path = dir.path().join("test_graph");

    // Phase 1: Open graph and create data
    let g = Graph::open(&graph_path, "g0").unwrap();

    let v1 = crud::create_vertex(&g, "Alice", &[], &[], &HashMap::new()).unwrap();
    let v2 = crud::create_vertex(&g, "Bob", &[], &[], &HashMap::new()).unwrap();
    let v3 = crud::create_vertex(&g, "Charlie", &[], &[], &HashMap::new()).unwrap();

    // Phase 2: Proper close (flushes everything + WAL checkpoint)
    g.close().unwrap();
    drop(g);

    // Phase 3: Delete all WAL files (simulating loss after shutdown)
    let wal_dir = graph_path.join("g0");
    for entry in std::fs::read_dir(&wal_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap().to_str().unwrap().starts_with("redo_") {
            std::fs::remove_file(&path).unwrap();
        }
    }

    // Phase 4: Reopen (WAL replay has nothing to replay)
    let g2 = Graph::open(&graph_path, "g0").unwrap();

    // Phase 5: Verify all data is intact from data file rebuild
    let alice2 = crud::get_vertex(&g2, v1).unwrap().unwrap();
    assert_eq!(alice2.name, "Alice");

    let bob2 = crud::get_vertex(&g2, v2).unwrap().unwrap();
    assert_eq!(bob2.name, "Bob");

    let charlie2 = crud::get_vertex(&g2, v3).unwrap().unwrap();
    assert_eq!(charlie2.name, "Charlie");

    // Gremlin should also work
    let r = execute_gremlin(&g2, &GremlinQuery {
        steps: vec![GremlinStep::V { ids: None, limit: None }],
    }, None);
    assert!(r.success, "gremlin failed: {:?}", r.error);
    assert_eq!(count_results(&r.data), 3, "should have 3 vertices");

    drop(g2);
}

// ── Test 2: Batch mode + block cache flush + WAL loss ──────────────────
//
// Scenario: enable batch mode, create vertices (WAL buffered),
// force-flush dirty blocks to disk, drop graph without end_batch()
// (simulating crash where in-memory WAL entries are lost).
// Reopen and verify the data file is self-consistent.
#[test]
fn batch_crash_with_flushed_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let graph_path = dir.path().join("test_graph");

    // Phase 1: Open graph
    let g = Arc::new(Graph::open(&graph_path, "g0").unwrap());

    // Phase 2: Start WAL batch mode (entries will be buffered in memory)
    g.redo_log.start_batch();

    // Phase 3: Create vertices (data goes to block cache, WAL is buffered)
    let v1 = crud::create_vertex(&g, "Alice", &["person".into()], &[], &HashMap::new()).unwrap();
    let v2 = crud::create_vertex(&g, "Bob", &["person".into()], &[], &HashMap::new()).unwrap();
    let v3 = crud::create_vertex(&g, "Charlie", &["person".into()], &[], &HashMap::new()).unwrap();

    // Phase 4: Force-flush all dirty blocks to disk.
    // This simulates LRU eviction under memory pressure during a batch.
    // After this, the bitmap and data are on disk, but the WAL entries
    // are still only in memory (not flushed via end_batch).
    g.block_cache
        .flush_dirty(&|idx, data| {
            g.data_file.write_block(idx, data)?;
            Ok(())
        })
        .unwrap();

    // Phase 5: Drop graph WITHOUT calling end_batch() or close().
    // This simulates a crash: WAL entries in batch_buffer are lost,
    // but dirty blocks were already flushed to disk.
    // The RedoLog::Drop sends Shutdown to the writer (which has nothing
    // since entries were never sent from batch_buffer), then joins
    // the writer thread.
    drop(g);

    // Phase 6: Reopen graph (WAL files exist but have no entries from
    // this batch; memory index is rebuilt from data file).
    let g2 = Graph::open(&graph_path, "g0").unwrap();

    // Phase 7: Verify the data file is self-consistent.
    // We can't guarantee ALL vertices survived (blocks might not have
    // been fully flushed), but the data file must NOT corrupt the
    // index rebuild — whatever is on disk must be valid.
    //
    // The memory index builder scans every allocated chunk by reading
    // the block header bitmap. If a chunk was allocated but the data
    // wasn't written, it reads old/garbage data. Garbage that doesn't
    // start with 0x01/0x02/0x03 is skipped as Empty.
    //
    // At minimum, the reopen must not panic or error.

    // Try to read vertices — those that were flushed should be found
    let alice = crud::get_vertex(&g2, v1);
    assert!(alice.is_ok(), "get_vertex must not error");

    // The graph should be functional for new operations
    let v4 = crud::create_vertex(&g2, "Dave", &[], &[], &HashMap::new()).unwrap();
    let dave = crud::get_vertex(&g2, v4).unwrap().unwrap();
    assert_eq!(dave.name, "Dave");

    drop(g2);
}

// ── Test 3: Batch import followed by WAL loss ──────────────────────────
//
// Scenario: use batch import (which uses start_batch + end_batch),
// close properly, delete WAL files, reopen. Verifies the full
// batch import path produces a self-consistent data file.
#[test]
fn batch_import_then_wal_loss() {
    let dir = tempfile::tempdir().unwrap();
    let graph_path = dir.path().join("test_graph");

    // Phase 1: Open graph and import data via batch
    let g = Arc::new(Graph::open(&graph_path, "g0").unwrap());

    let entities = vec![
        BatchEntity { name: "Alice".into(), labels: vec!["person".into()], keywords: vec![], properties: HashMap::new() },
        BatchEntity { name: "Bob".into(), labels: vec!["person".into()], keywords: vec![], properties: HashMap::new() },
        BatchEntity { name: "Charlie".into(), labels: vec!["person".into()], keywords: vec![], properties: HashMap::new() },
    ];
    let relations = vec![
        BatchRelation { source: "Alice".into(), target: "Bob".into(), name: "knows".into(), labels: vec![], keywords: vec![], strength: 0.8, properties: HashMap::new() },
        BatchRelation { source: "Bob".into(), target: "Charlie".into(), name: "knows".into(), labels: vec![], keywords: vec![], strength: 0.6, properties: HashMap::new() },
    ];

    // Call batch_import with update_existing = false (append mode)
    let result = batch::batch_import(&g, &entities, &relations, "", false);
    assert_eq!(result.vertices_created, 3);
    assert_eq!(result.edges_created, 2);

    // Verify data visible
    let r = execute_gremlin(&g, &GremlinQuery {
        steps: vec![GremlinStep::V { ids: None, limit: None }],
    }, None);
    assert!(r.success);
    assert_eq!(count_results(&r.data), 3, "should have 3 vertices");

    // Phase 2: Close properly
    // Graph::close takes &self, works on Arc reference
    g.close().unwrap();

    // Phase 3: Delete WAL files (simulating WAL loss)
    let wal_dir = graph_path.join("g0");
    for entry in std::fs::read_dir(&wal_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap().to_str().unwrap().starts_with("redo_") {
            std::fs::remove_file(&path).unwrap();
        }
    }
    drop(g);

    // Phase 4: Reopen
    let g2 = Graph::open(&graph_path, "g0").unwrap();

    // Phase 5: Verify data from batch import survived
    let r2 = execute_gremlin(&g2, &GremlinQuery {
        steps: vec![GremlinStep::V { ids: None, limit: None }],
    }, None);
    assert!(r2.success, "gremlin after reopen failed: {:?}", r2.error);
    assert_eq!(count_results(&r2.data), 3, "batch vertices should survive WAL loss");

    // Verify edges also survived
    let r3 = execute_gremlin(&g2, &GremlinQuery {
        steps: vec![GremlinStep::E { ids: None, limit: None }],
    }, None);
    assert!(r3.success);
    assert_eq!(count_results(&r3.data), 2, "batch edges should survive WAL loss");

    drop(g2);
}

// ── Test 4: Repeated batch import + WAL loss cycles ───────────────────
//
// Stress test: repeatedly import batches, close, lose WALs, reopen.
// Verifies the engine can survive multiple crash-recovery cycles.
#[test]
fn repeated_batch_import_cycles() {
    let dir = tempfile::tempdir().unwrap();
    let graph_path = dir.path().join("test_graph");

    for cycle in 0..5 {
        // Open (or reopen) graph
        let g = Arc::new(Graph::open(&graph_path, "g0").unwrap());

        // Import a batch of 10 vertices
        let entities: Vec<BatchEntity> = (0..10)
            .map(|i| BatchEntity {
                name: format!("User_{}_{}", cycle, i),
                labels: vec!["user".into()],
                keywords: vec![],
                properties: HashMap::new(),
            })
            .collect();

        let result = batch::batch_import(&g, &entities, &[], "", false);
        assert_eq!(result.vertices_created, 10);

        // Close properly
        g.close().unwrap();
        drop(g);

        // Delete WAL files (simulate WAL loss between cycles)
        let wal_dir = graph_path.join("g0");
        if let Ok(dir) = std::fs::read_dir(&wal_dir) {
            for entry in dir {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.file_name().unwrap().to_str().unwrap_or("").starts_with("redo_") {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
    }

    // Final reopen: all 50 vertices should be present
    let g2 = Graph::open(&graph_path, "g0").unwrap();

    let r = execute_gremlin(&g2, &GremlinQuery {
        steps: vec![GremlinStep::V { ids: None, limit: None }],
    }, None);
    assert!(r.success, "final gremlin failed: {:?}", r.error);
    assert_eq!(count_results(&r.data), 50, "all 50 vertices from 5 cycles must survive");

    drop(g2);
}
