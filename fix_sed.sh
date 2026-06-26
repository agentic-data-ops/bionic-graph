#!/bin/bash
for f in src/gremlin/server.rs src/gremlin/steps.rs; do
    sed -i 's/\.all_edges()\.filter(/.all_edges().into_iter().filter(/g' "$f"
    sed -i 's/\.vertex_ids()\.filter_map(/.vertex_ids().into_iter().filter_map(/g' "$f"
    sed -i 's/\.create_edge(/.add_edge(/g' "$f"
    sed -i 's/\.get_edge_mut(/.update_edge(/g' "$f"
    sed -i 's/\.remove_vertex(id, true)/.remove_vertex(id)/g' "$f"
done
