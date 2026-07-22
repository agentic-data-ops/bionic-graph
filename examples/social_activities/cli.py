#!/usr/bin/env python3
"""Social Activities Knowledge Graph CLI — load / plan / act.

Usage:
  python cli.py load [--md PATH] [--graph NAME] [--model MODEL] [--base-url URL] [--force]
  python cli.py plan [--graph NAME] [--model MODEL] [--base-url URL]
  python cli.py act [--count N] [--graph NAME] [--model MODEL] [--base-url URL]
"""

from __future__ import annotations

import argparse
import json
import os
import sys

from bionic_graph import Client

# Local imports — works when running `python cli.py` from this directory
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from llm import call_llm, call_llm_json
from prompts import (
    EXTRACT_SYSTEM_PROMPT,
    EXTRACT_USER_PROMPT_TEMPLATE,
    PLAN_SYSTEM_PROMPT,
    PLAN_USER_PROMPT_TEMPLATE,
    EXEC_SYSTEM_PROMPT,
    EXEC_USER_PROMPT_TEMPLATE,
    build_person_context,
)
from graph_utils import (
    ensure_graph,
    load_json_to_graph,
    search_graph,
    fetch_plans_sorted_by_priority,
    update_plan_statuses,
)


# ── Helpers ────────────────────────────────────────────────────────────

def _make_client(base_url: str) -> Client:
    return Client(base_url=base_url, timeout=120.0)


def _get_default_model(client: Client) -> str:
    """Get the default LLM model from the server settings."""
    try:
        settings = client.get_llm_settings()
        default = settings.get("default_model", "")
        if default:
            return default
        providers = settings.get("providers", [])
        if providers:
            p = providers[0]
            models = p.get("models", [])
            if models:
                return f"{p.get('name', '')}/{models[0]}"
    except Exception:
        pass
    return "DeepSeek/deepseek-v4-flash"


def _read_file(path: str) -> str:
    with open(path, "r", encoding="utf-8") as f:
        return f.read()


def _write_json(path: str, data) -> None:
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
    print(f"  Written to {path}")


def _log_path(prefix: str) -> str:
    """Generate a timestamped log file path under the log/ directory.

    Example: log/plan_activities_20260721_163000.json
    """
    from datetime import datetime
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    return f"log/{prefix}_{ts}.json"


def _print_stats(stats: dict) -> None:
    print(f"\n  Summary: {stats['vertices_created']} vertices created, "
          f"{stats['vertices_skipped']} skipped, "
          f"{stats['edges_created']} edges created, "
          f"{stats['edges_skipped']} edges skipped")


# ── load ───────────────────────────────────────────────────────────────

def run_load(args: argparse.Namespace) -> None:
    """Load social activities from a Markdown document into the graph.

    Pipeline:
      1. Read the Markdown document.
      2. Call LLM to extract entities and relations as JSON.
      3. Save the extracted JSON to a file.
      4. Load the JSON into the Bionic-Graph graph (dedup by name).

    If --force is set, the existing graph is deleted and recreated.
    """
    print(f"\n{'='*60}")
    print(f"  Phase: load")
    print(f"  Document: {args.md}")
    print(f"  Graph: {args.graph}")
    print(f"{'='*60}\n")

    # Read the markdown document
    print("Reading document...")
    content = _read_file(args.md)
    print(f"  Read {len(content)} chars from {args.md}")

    # If --force, delete and recreate the graph for a clean slate
    client = _make_client(args.base_url)
    try:
        if args.force:
            print("\nForce mode: deleting existing graph...")
            try:
                client.delete_graph(args.graph, force=True)
                print(f"  Deleted graph '{args.graph}'")
            except Exception:
                print(f"  Graph '{args.graph}' does not exist, skipping delete")
    finally:
        client.close()

    # Call LLM to extract entities and relations
    print("\nCalling LLM to extract knowledge graph...")
    user_prompt = EXTRACT_USER_PROMPT_TEMPLATE.format(document_content=content)

    client = _make_client(args.base_url)
    try:
        model = args.model or _get_default_model(client)
        result = call_llm_json(
            EXTRACT_SYSTEM_PROMPT,
            user_prompt,
            model=model,
            client=client,
        )
    finally:
        client.close()

    # Validate the result
    if "entities" not in result or "relations" not in result:
        print("  ERROR: LLM output missing 'entities' or 'relations' key")
        print(f"  Raw keys: {list(result.keys())}")
        sys.exit(1)

    entities_count = len(result["entities"])
    relations_count = len(result["relations"])
    print(f"  Extracted {entities_count} entities and {relations_count} relations")

    # Save extraction result
    _write_json(args.output, result)

    # Load into graph
    print("\nLoading into graph...")
    client = _make_client(args.base_url)
    try:
        stats = load_json_to_graph(client, args.graph, result)
        _print_stats(stats)
    finally:
        client.close()

    print(f"\nLoad complete. Data saved to {args.output} and graph '{args.graph}'.")


# ── plan ───────────────────────────────────────────────────────────────

def run_plan(args: argparse.Namespace) -> None:
    """Generate new social activity plans based on current graph state.

    Pipeline:
      1. Search the graph for "activity plan" to gather context.
      2. Build a summary of the current graph state.
      3. Call LLM to generate social activity plans.
      4. Save the plan JSON.
      5. Load plans into the graph.
    """
    print(f"\n{'='*60}")
    print(f"  Phase: plan")
    print(f"  Graph: {args.graph}")
    print(f"{'='*60}\n")

    client = _make_client(args.base_url)

    try:
        # Ensure graph exists
        ensure_graph(client, args.graph)

        # Search for graph context
        print("Searching graph for current state...")
        all_results: list[dict] = []
        results = search_graph(client, "activity plan", args.graph, limit=80)
        all_results.extend(results)
        print(f"  Search results: {len(results)} items")

        # Fallback to full scan if search returned nothing
        if not all_results:
            print("  Search returned no results. Falling back to full graph scan via Gremlin...")
            try:
                resp = client.execute_gremlin([{"step": "V"}], graph=args.graph)
                if resp.success:
                    for item in resp.data:
                        d = item.model_dump() if hasattr(item, "model_dump") else dict(item)
                        all_results.append(d)
                    print(f"  Fetched {len(all_results)} vertices via Gremlin")
            except Exception as e:
                print(f"  Gremlin fallback failed: {e}")

        # Dedup results by id
        seen_ids = set()
        unique_results = []
        for r in all_results:
            rid = r.get("id")
            if rid not in seen_ids:
                seen_ids.add(rid)
                unique_results.append(r)

        # Sort by priority + rank
        def _sort_key(item: dict) -> tuple:
            props = item.get("properties", {})
            pval = str(props.get("priority", "")).lower()
            priority_order = {"high": 0, "medium": 1, "low": 2, "": 3}
            pnum = priority_order.get(pval, 3)
            rank_val = item.get("rank", 0) or 0
            return (pnum, -rank_val)

        unique_results.sort(key=_sort_key)

        # Build graph summary
        summary_lines = [
            f"Found {len(unique_results)} unique entities in the graph.",
            "---",
        ]
        for r in unique_results:
            name = r.get("name", "?")
            labels = r.get("labels", [])
            rank = r.get("rank", 0)
            props = r.get("properties", {})
            priority = props.get("priority", "-")
            props_str = "; ".join(f"{k}={v}" for k, v in list(props.items())[:4])
            summary_lines.append(f"- {name}  priority={priority}  rank={rank}  labels={labels}  {props_str}")

        graph_summary = "\n".join(summary_lines)
        print(f"\n  Graph summary ({len(unique_results)} unique entities):")
        for line in summary_lines:
            print(f"    {line}")

        # Call LLM to generate plans
        print("\nCalling LLM to generate social activity plans...")
        user_prompt = PLAN_USER_PROMPT_TEMPLATE.format(graph_summary=graph_summary)

        model = args.model or _get_default_model(client)
        result = call_llm_json(
            PLAN_SYSTEM_PROMPT,
            user_prompt,
            model=model,
            client=client,
        )

        if "entities" not in result or "relations" not in result:
            print("  ERROR: LLM output missing 'entities' or 'relations' key")
            sys.exit(1)

        plan_count = len(result["entities"])
        relation_count = len(result["relations"])
        print(f"  Generated {plan_count} plan items and {relation_count} relations")

        # Save plan JSON
        output_path = args.output or _log_path("plan_activities")
        _write_json(output_path, result)

        # Load plans into graph
        print("\nLoading plans into graph...")
        stats = load_json_to_graph(client, args.graph, result)
        _print_stats(stats)

    finally:
        client.close()

    print(f"\nPlan complete. Output: {output_path}. Loaded into graph '{args.graph}'.")


# ── act ────────────────────────────────────────────────────────────────

def run_act(args: argparse.Namespace) -> None:
    """Execute top-N social activities sorted by priority.

    Pipeline:
      1. Fetch plan vertices from the graph, sorted by priority descending.
      2. Select the top N plans.
      3. Call LLM to simulate execution of each plan as an activity.
      4. Save the activity log JSON.
      5. Create activity vertices + relations in the graph.
      6. Update plan statuses.
    """
    count = args.count
    print(f"\n{'='*60}")
    print(f"  Phase: act")
    print(f"  Graph: {args.graph}")
    print(f"  Top-N: {count}")
    print(f"{'='*60}\n")

    client = _make_client(args.base_url)

    try:
        # Fetch plans sorted by priority
        print("Searching 'activity plan' for plans sorted by priority...")
        plans = fetch_plans_sorted_by_priority(client, args.graph)
        print(f"  Found {len(plans)} plan(s) in graph")

        if not plans:
            print("  No plans found. Run 'plan' first.")
            sys.exit(1)

        # Select top N
        selected = plans[:count]
        print(f"\n  Selected top {len(selected)} plan(s) for execution:")
        for p in selected:
            priority = p.get("properties", {}).get("priority", "?")
            print(f"    [priority={priority}] {p.get('name', '?')}  "
                  f"labels={p.get('labels', [])}")

        # Build person context from the social_activities.md file
        md_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "social_activities.md")
        person_context = ""
        if os.path.exists(md_path):
            content = _read_file(md_path)
            person_context = build_person_context(content)
            print(f"\n  Person context: {len(person_context)} chars")
        else:
            person_context = "A group of ~10 young friends in Chengdu, including Zhang Wei (PM), Wang Qiang (sales), Chen Jie (fitness coach), and others."
            print(f"\n  No social_activities.md found, using default context")

        # Call LLM to simulate plan execution
        print("\nCalling LLM to simulate activities...")
        plans_json = json.dumps(selected, indent=2, ensure_ascii=False)
        user_prompt = EXEC_USER_PROMPT_TEMPLATE.format(
            person_context=person_context,
            plans_json=plans_json,
        )

        model = args.model or _get_default_model(client)
        result = call_llm_json(
            EXEC_SYSTEM_PROMPT,
            user_prompt,
            model=model,
            client=client,
        )

        if "entities" not in result:
            print("  ERROR: LLM output missing 'entities' key")
            sys.exit(1)

        activity_count = len(result["entities"])
        print(f"  Simulated {activity_count} activity/activities")

        # Save activity log
        output_path = args.output or _log_path("exec_activities")
        _write_json(output_path, result)

        # Load activities into graph
        print("\nLoading activities into graph...")
        stats = load_json_to_graph(client, args.graph, result)
        _print_stats(stats)

        # Update plan statuses
        plan_updates = result.get("plan_updates", [])
        if plan_updates:
            print(f"\nUpdating plan statuses ({len(plan_updates)} plan(s))...")
            updated = update_plan_statuses(client, args.graph, plan_updates)
            print(f"  Updated {updated} plan(s)")

    finally:
        client.close()

    print(f"\nAct complete. Output: {output_path}. Loaded into graph '{args.graph}'.")


# ── CLI Entry ──────────────────────────────────────────────────────────

def main() -> None:
    parser = argparse.ArgumentParser(
        description="Social Activities Knowledge Graph CLI",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    # load
    p_load = subparsers.add_parser("load", help="Load social activities from MD into graph")
    p_load.add_argument("--md", default="social_activities.md",
                        help="Markdown document path (default: social_activities.md)")
    p_load.add_argument("--graph", default="social-graph",
                        help="Graph name (default: social-graph)")
    p_load.add_argument("--model", default=None,
                        help="LLM model name (default: settings default_model)")
    p_load.add_argument("--output", default="log/social_activities.json",
                        help="Output JSON file path (default: log/social_activities.json)")
    p_load.add_argument("--base-url", default="http://127.0.0.1:8080",
                        help="Backend URL (default: http://127.0.0.1:8080)")
    p_load.add_argument("--force", action="store_true",
                        help="Delete and recreate graph before loading")

    # plan
    p_plan = subparsers.add_parser("plan",
                                   help="Generate new social activity plans")
    p_plan.add_argument("--graph", default="social-graph",
                        help="Graph name (default: social-graph)")
    p_plan.add_argument("--model", default=None,
                        help="LLM model name (default: settings default_model)")
    p_plan.add_argument("--output", default=None,
                        help="Output file path (default: log/plan_activities_<timestamp>.json)")
    p_plan.add_argument("--base-url", default="http://127.0.0.1:8080",
                        help="Backend URL (default: http://127.0.0.1:8080)")

    # act
    p_act = subparsers.add_parser("act",
                                  help="Simulate social activity execution")
    p_act.add_argument("--count", type=int, default=3,
                       help="Number of activities to simulate (default: 3)")
    p_act.add_argument("--graph", default="social-graph",
                       help="Graph name (default: social-graph)")
    p_act.add_argument("--model", default=None,
                       help="LLM model name (default: settings default_model)")
    p_act.add_argument("--output", default=None,
                       help="Output file path (default: log/exec_activities_<timestamp>.json)")
    p_act.add_argument("--base-url", default="http://127.0.0.1:8080",
                       help="Backend URL (default: http://127.0.0.1:8080)")

    args = parser.parse_args()

    if args.command == "load":
        run_load(args)
    elif args.command == "plan":
        run_plan(args)
    elif args.command == "act":
        run_act(args)


if __name__ == "__main__":
    main()
