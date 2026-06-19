use std::path::Path;

use crate::graph::Graph;
use crate::graph::vertex::{VertexId, now_micros};

use super::version_log::{self, VlogEntry, VlogStats};

/// Statistics from a compaction run.
#[derive(Debug, Default, Clone)]
pub struct CompactionStats {
    /// Number of vertices processed.
    pub vertices_scanned: usize,
    /// Number of vertices that had history compacted.
    pub vertices_compacted: usize,
    /// Total version records archived.
    pub records_archived: usize,
    /// Total version records removed (truncated beyond max_history).
    pub records_truncated: usize,
    /// Result of the vlog write.
    pub vlog: Option<VlogStats>,
    /// Execution time in microseconds.
    pub elapsed_us: i64,
}

/// Run compaction on a graph: archive records before `before_timestamp` to vlog.
pub fn compact_graph(
    graph: &mut Graph,
    data_dir: &Path,
    before_timestamp: i64,
    sequence: u32,
) -> CompactionStats {
    let start = now_micros();
    let mut stats = CompactionStats::default();

    // Collect all vertex IDs first to avoid borrow issues
    let all_ids: Vec<VertexId> = graph.vertex_ids().copied().collect();
    stats.vertices_scanned = all_ids.len();

    let mut vlog_entries: Vec<VlogEntry> = Vec::new();

    for &vid in &all_ids {
        let v = match graph.get_vertex_mut(vid) {
            Some(v) => v,
            None => continue,
        };

        let removed = v.compact(before_timestamp);
        if !removed.is_empty() {
            stats.vertices_compacted += 1;
            stats.records_archived += removed.len();
            for record in &removed {
                vlog_entries.push(VlogEntry {
                    vertex_id: vid,
                    version: record.version,
                    record: record.clone(),
                });
            }
        }

        // Also enforce max_history (default 100) for records kept in-memory
        let max_keep = 100;
        let truncated = v.compact_max(max_keep);
        stats.records_truncated += truncated.len();
    }

    // Write archived records to vlog
    if !vlog_entries.is_empty() {
        match version_log::write_vlog(data_dir, &vlog_entries, sequence) {
            Ok(vlog_stats) => stats.vlog = Some(vlog_stats),
            Err(e) => log::error!("Failed to write version log: {}", e),
        }
    }

    stats.elapsed_us = now_micros() - start;
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use tempfile::tempdir;

    #[test]
    fn test_compact_empty_graph() {
        let mut graph = Graph::new();
        let dir = tempdir().unwrap();
        let stats = compact_graph(&mut graph, dir.path(), 9999999999, 1);
        assert_eq!(stats.vertices_scanned, 0);
        assert_eq!(stats.records_archived, 0);
    }

    #[test]
    fn test_compact_single_vertex() {
        let mut graph = Graph::new();
        let vid = graph.create_vertex(vec!["test".to_string()]);

        // Make some updates to generate history
        for i in 0..5 {
            let mut props = std::collections::HashMap::new();
            props.insert("val".to_string(), crate::graph::PropertyValue::Integer(i));
            graph.get_vertex_mut(vid).unwrap().update_properties(props, true);
        }

        let dir = tempdir().unwrap();
        // Compact everything before a far-future timestamp
        let stats = compact_graph(&mut graph, dir.path(), 9999999999999_i64, 1);
        assert!(stats.records_archived >= 5, "Should archive old records");
        assert!(stats.vlog.is_some(), "Should have written vlog");

        // Check vlog was created
        let files = version_log::list_vlog_files(dir.path()).unwrap();
        assert!(!files.is_empty(), "Vlog files should exist");
    }
}
