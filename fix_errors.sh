#!/bin/bash
# Fix remaining compilation errors after DiskGraph migration
set -e

# 1. server.rs: fix method calls
cd src/gremlin
sed -i 's/handle\.graph\.lock/handle.disk_graph.lock/g' server.rs
sed -i 's/h\.graph\.lock/h.disk_graph.lock/g' server.rs
sed -i 's/let g = handle\.disk_graph\.lock/let mut g = handle.disk_graph.lock/g' server.rs
sed -i 's/g\.all_edges()\.filter(/g.all_edges().into_iter().filter(/g' server.rs
sed -i 's/\.cloned()//g' server.rs
sed -i 's/\.create_edge(/\.add_edge(/g' server.rs
sed -i 's/\.remove_vertex(id, \([^)]*\))/.remove_vertex(id)/g' server.rs
sed -i 's/\.soft_delete_edge(eid, \([^)]*\))/.soft_delete_edge(eid, true)/g' server.rs

echo "server.rs fixed"
