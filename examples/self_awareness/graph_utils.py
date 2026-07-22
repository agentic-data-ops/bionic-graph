"""Graph utility functions for the Self-Awareness CLI pipeline.

Handles graph lifecycle, dedup vertex loading, search, and plan updates.
All operations use the Bionic-Graph Python SDK client.
"""

from __future__ import annotations

import json
from typing import Any, Optional

from bionic_graph import Client


def ensure_graph(client: Client, graph_name: str) -> bool:
    """Ensure a graph exists. Creates it if missing.

    Args:
        client: Bionic-Graph Client instance.
        graph_name: Name of the graph.

    Returns:
        True if the graph was newly created, False if it already existed.
    """
    graphs = client.list_graphs()
    for g in graphs.graphs:
        if g.name == graph_name:
            print(f"  Using existing graph '{graph_name}'")
            return False
    client.create_graph(name=graph_name, description="Self-awareness knowledge graph")
    print(f"  Created graph '{graph_name}'")
    return True


def _to_dict(item) -> dict:
    """Convert a Gremlin response item (Pydantic model or dict) to a plain dict."""
    if isinstance(item, dict):
        return item
    if hasattr(item, 'model_dump'):
        return item.model_dump()
    return dict(item)


def get_all_vertex_names(client: Client, graph_name: str) -> dict[str, int]:
    """Get a mapping of vertex name → vertex ID for all vertices in the graph.

    Uses a Gremlin query to fetch all vertices with their names.
    Returns a dict like {"self": 1, "Vancouver": 2, ...}.
    """
    name_to_id: dict[str, int] = {}
    try:
        steps = [{"step": "V"}]
        resp = client.execute_gremlin(steps, graph=graph_name)
        if resp.success:
            for item in resp.data:
                d = _to_dict(item)
                if d.get("type") == "vertex":
                    vid = d.get("id")
                    vname = d.get("name", "")
                    if vname and vid is not None:
                        name_to_id[vname] = vid
    except Exception as e:
        print(f"  Warning: failed to fetch vertex names: {e}")
    return name_to_id


def load_json_to_graph(client: Client, graph_name: str, data: dict) -> dict[str, Any]:
    """Load entities and relations from a JSON dict into the graph.

    Uses the SDK's batch_load method which upserts vertices by name
    and edges by (source_name, target_name, name). Entity names are
    used as the unique key — no manual dedup or ID resolution needed.

    Args:
        client: Bionic-Graph Client instance.
        graph_name: Target graph name.
        data: Dict with "entities" list and "relations" list.
              Each entity: {name, labels?, keywords?, properties?}
              Each relation: {source, target, name, labels?, keywords?, strength?, properties?}

    Returns:
        Dict with "vertices_created", "vertices_skipped", "edges_created",
        "edges_skipped", and "name_to_id" mapping.
    """
    # Ensure graph exists
    ensure_graph(client, graph_name)

    # Build name -> id mapping from existing vertices (for progress reporting)
    name_to_id = get_all_vertex_names(client, graph_name)
    print(f"  Found {len(name_to_id)} existing vertices in graph")

    entities = data.get("entities", [])
    relations = data.get("relations", [])

    # Print per-entity progress messages
    for entity in entities:
        ename = entity.get("name", "")
        if not ename:
            print("  ⚠️  Skipping entity with empty name")
            continue
        if ename in name_to_id:
            print(f"  ⏭️  Skip '{ename}' — already exists (id={name_to_id[ename]})")
        else:
            print(f"  ✅ Created '{ename}'")

    # Batch load via SDK (upserts by name, no manual per-item calls needed)
    result = client.batch_load(entities, relations, graph=graph_name)

    # Refresh name_to_id after the batch load
    name_to_id = get_all_vertex_names(client, graph_name)

    # Print per-relation progress messages
    for rel in relations:
        src_name = rel.get("source", "")
        tgt_name = rel.get("target", "")
        rel_name = rel.get("name", "")

        if src_name not in name_to_id:
            print(f"  ⚠️  Skip relation '{rel_name}': source '{src_name}' not found")
            continue
        if tgt_name not in name_to_id:
            print(f"  ⚠️  Skip relation '{rel_name}': target '{tgt_name}' not found")
            continue
        print(f"  🔗 Created edge '{rel_name}'  {src_name} -> {tgt_name}")

    # Build stats from batch_load response
    stats = {
        "vertices_created": result.get("vertices_created", 0),
        "vertices_skipped": result.get("vertices_updated", 0),
        "edges_created": result.get("edges_created", 0),
        "edges_skipped": result.get("edges_updated", 0),
    }

    stats["name_to_id"] = name_to_id
    return stats


def search_vertex_by_name(client: Client, name: str, graph_name: str) -> Optional[dict]:
    """Search for a vertex by its exact name using Gremlin.

    Returns the vertex dict (with id, name, labels, keywords, properties, rank)
    or None if not found.
    """
    try:
        steps = [
            {"step": "V"},
            {"step": "has", "key": "name", "value": name},
        ]
        resp = client.execute_gremlin(steps, graph=graph_name)
        if resp.success and resp.data:
            return _to_dict(resp.data[0])
    except Exception:
        pass
    return None


def search_graph(client: Client, query: str, graph_name: str, limit: int = 30) -> list[dict]:
    """Search the graph using the full-text search endpoint.

    Args:
        client: Bionic-Graph Client instance.
        query: Search text.
        graph_name: Target graph.
        limit: Max results.

    Returns:
        List of vertex/edge result dicts.
    """
    try:
        resp = client.search(text=query, mode="greedy", limit=limit, graph=graph_name)
        if resp.success:
            # Convert Pydantic model items to plain dicts
            return [_to_dict(item) for item in resp.data]
    except Exception as e:
        print(f"  Warning: search failed: {e}")
    return []


def update_plan_statuses(client: Client, graph_name: str, plan_updates: list[dict]) -> int:
    """Update plan vertex properties (e.g., status, progress_pct).

    Args:
        client: Bionic-Graph Client instance.
        graph_name: Target graph.
        plan_updates: List of {name: str, properties: dict}.

    Returns:
        Number of successfully updated vertices.
    """
    # Get current name → id mapping
    name_to_id = get_all_vertex_names(client, graph_name)
    updated = 0

    for update in plan_updates:
        pname = update.get("name", "")
        props = update.get("properties", {})
        vid = name_to_id.get(pname)
        if vid is None:
            print(f"  ⚠️  Plan '{pname}' not found for status update")
            continue
        try:
            client.update_vertex(vid=vid, properties=props, graph=graph_name)
            print(f"  📝 Updated plan '{pname}' (id={vid}): {json.dumps(props)}")
            updated += 1
        except Exception as e:
            print(f"  ⚠️  Failed to update plan '{pname}': {e}")

    return updated


def fetch_plans_sorted_by_rank(client: Client, graph_name: str) -> list[dict]:
    """Fetch plans by searching 'my task', sorted by priority property descending.

    Returns:
        List of plan vertex dicts, each with id, name, labels, properties, rank, priority.
    """
    plans: list[dict] = []
    try:
        # Search using "my task" keyword
        resp = client.search(text="my task", mode="greedy", limit=50, graph=graph_name)
        if resp.success and resp.data:
            for item in resp.data:
                d = _to_dict(item)
                if d.get("type") == "vertex":
                    plans.append(d)

        # Fallback: if search returned nothing, scan all vertices for "plan" label
        if not plans:
            steps = [{"step": "V"}]
            resp2 = client.execute_gremlin(steps, graph=graph_name)
            if resp2.success:
                for item in resp2.data:
                    d = _to_dict(item)
                    if d.get("type") == "vertex":
                        labels = d.get("labels", [])
                        if "plan" in labels:
                            plans.append(d)

        # Sort by priority property descending (higher priority = more important)
        # priority can be in properties.priority as int or float
        def _priority(p: dict) -> float:
            props = p.get("properties", {})
            val = props.get("priority", 0)
            if val is None:
                return 0.0
            try:
                return float(val)
            except (ValueError, TypeError):
                return 0.0

        plans.sort(key=_priority, reverse=True)
    except Exception as e:
        print(f"  Warning: failed to fetch plans: {e}")

    return plans
