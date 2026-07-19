"""CLI for Bionic-Graph — topic/action based command-line interface."""
from __future__ import annotations

import json
import sys
import time
from typing import Optional

import click

from .client import Client
from .exceptions import BionicGraphError


# ── Helpers ────────────────────────────────────────────────────────

def _client(ctx: click.Context) -> Client:
    """Get or create the client from context."""
    if ctx.obj is None:
        ctx.obj = Client(
            base_url=ctx.params.get("base_url", "http://127.0.0.1:8080"),
            api_key=ctx.params.get("api_key"),
            timeout=ctx.params.get("timeout", 30.0),
        )
    return ctx.obj


def _fmt(ctx: click.Context) -> str:
    """Get output format from the root context."""
    root = ctx
    while root.parent:
        root = root.parent
    return root.params.get("fmt", "text")


def _output(data, fmt: str = "text"):
    """Print output in requested format."""
    if fmt == "json":
        click.echo(json.dumps(data, indent=2, ensure_ascii=False, default=str))
    else:
        if isinstance(data, str):
            click.echo(data)
        elif isinstance(data, dict):
            # Print all key-value pairs
            for k, v in data.items():
                if isinstance(v, (list, dict)):
                    click.echo(f"{k}: {json.dumps(v, ensure_ascii=False, default=str)}")
                else:
                    click.echo(f"{k}: {v}")
        elif isinstance(data, list):
            for item in data:
                click.echo(json.dumps(item, ensure_ascii=False, default=str))
        else:
            click.echo(json.dumps(data, ensure_ascii=False, default=str))


def _parse_json_arg(ctx, param, value):
    """Parse a JSON string argument."""
    if value is None:
        return value
    try:
        return json.loads(value)
    except json.JSONDecodeError:
        raise click.BadParameter(f"Invalid JSON: {value}")


# ── Global options ──────────────────────────────────────────────────

@click.group()
@click.option("--base-url", default="http://127.0.0.1:8080", envvar="BIONIC_GRAPH_BASE_URL", show_default=True, show_envvar=True, help="Backend server URL")
@click.option("--api-key", default=None, envvar="BIONIC_GRAPH_API_KEY", show_envvar=True, help="API key for authentication")
@click.option("--timeout", default=30.0, show_default=True, help="Request timeout (seconds)")
@click.option("--output", "fmt", type=click.Choice(["text", "json"]), default="text", help="Output format")
@click.pass_context
def main(ctx, base_url, api_key, timeout, fmt):
    """Bionic-Graph CLI — interact with a Bionic-Graph knowledge graph server."""
    ctx.ensure_object(dict)
    ctx.obj = None  # will be lazily created
    pass  # fmt is stored in ctx.parent.params


# ── health ─────────────────────────────────────────────────────────

@main.group()
def health():
    """Health check commands."""


@health.command("check")
@click.pass_context
def health_check(ctx):
    """Check server health."""
    c = _client(ctx)
    _output(c.health().model_dump(), _fmt(ctx))


# ── graph ─────────────────────────────────────────────────────────

@main.group()
def graph():
    """Graph lifecycle management."""


@graph.command("list")
@click.pass_context
def graph_list(ctx):
    """List all graphs and show the default graph."""
    c = _client(ctx)
    resp = c.list_graphs()
    if _fmt(ctx) == "json":
        _output(resp.model_dump(), "json")
    else:
        click.echo(f"Default graph: {resp.default}")
        for g in resp.graphs:
            click.echo(f"  {g.name}  time_travel={g.time_travel}")


@graph.command("create")
@click.argument("name")
@click.option("--description", default="", help="Graph description")
@click.option("--time-travel/--no-time-travel", default=False, help="Enable time-travel query support")
@click.pass_context
def graph_create(ctx, name, description, time_travel):
    """Create a new graph with the given name."""
    c = _client(ctx)
    _output(c.create_graph(name, description, time_travel).model_dump(), _fmt(ctx))


@graph.command("set-default")
@click.argument("name")
@click.pass_context
def graph_set_default(ctx, name):
    """Set a graph as the default for subsequent operations."""
    c = _client(ctx)
    _output(c.set_default_graph(name).model_dump(), _fmt(ctx))


@graph.command("delete")
@click.argument("name")
@click.option("--force", is_flag=True, default=False, help="Hard-delete (irreversible)")
@click.pass_context
def graph_delete(ctx, name, force):
    """Delete a graph. Use --force for hard delete."""
    c = _client(ctx)
    _output(c.delete_graph(name, force).model_dump(), _fmt(ctx))


@graph.command("update-meta")
@click.argument("name")
@click.option("--description", help="New description")
@click.option("--time-travel", type=bool, help="Enable/disable time-travel")
@click.pass_context
def graph_update_meta(ctx, name, description, time_travel):
    """Update graph metadata (description, time-travel)."""
    c = _client(ctx)
    _output(c.update_graph_meta(name, description, time_travel).model_dump(), _fmt(ctx))


@graph.command("get-config")
@click.argument("name")
@click.pass_context
def graph_get_config(ctx, name):
    """Get the storage config for a graph."""
    c = _client(ctx)
    _output(c.get_graph_config(name), _fmt(ctx))


@graph.command("set-config")
@click.argument("name")
@click.option("--config", required=True, callback=_parse_json_arg,
              help='JSON: storage config. Fields: cache_capacity (int), rotation_size_mb (int), rotation_interval_min (int). Example: \'{"cache_capacity": 8192, "rotation_size_mb": 64}\'')
@click.pass_context
def graph_set_config(ctx, name, config):
    c = _client(ctx)
    _output(c.set_graph_config(name, config).model_dump(), _fmt(ctx))


# ── vertex ─────────────────────────────────────────────────────────

@main.group()
def vertex():
    """Vertex CRUD."""


@vertex.command("create")
@click.option("--name", default="", help="Vertex name / label text")
@click.option("--labels", default=None, callback=_parse_json_arg,
              help='JSON array of labels. Example: \'["person", "noble"]\'')
@click.option("--keywords", default=None, callback=_parse_json_arg,
              help='JSON array of keywords. Example: \'["winterfell", "stark"]\'')
@click.option("--properties", default=None, callback=_parse_json_arg,
              help='JSON object of custom properties. Example: \'{"age": 35, "house": "Stark"}\'')
@click.option("--graph", default=None, help="Target graph (default: graph0)")
@click.pass_context
def vertex_create(ctx, name, labels, keywords, properties, graph):
    """Create a new vertex in the graph."""
    c = _client(ctx)
    _output(c.create_vertex(name, labels, keywords, properties, graph).model_dump(), _fmt(ctx))


@vertex.command("update")
@click.argument("vid", type=int)
@click.option("--name", help="New name")
@click.option("--labels", callback=_parse_json_arg,
              help='JSON array of labels. Example: \'["person", "noble"]\'')
@click.option("--keywords", callback=_parse_json_arg,
              help='JSON array of keywords. Example: \'["winterfell", "stark"]\'')
@click.option("--properties", callback=_parse_json_arg,
              help='JSON object of custom properties. Example: \'{"age": 35, "house": "Stark"}\'')
@click.option("--graph", help="Target graph (default: graph0)")
@click.pass_context
def vertex_update(ctx, vid, name, labels, keywords, properties, graph):
    """Update an existing vertex's attributes."""
    c = _client(ctx)
    c.update_vertex(vid, name, labels, keywords, properties, graph)
    _output({"status": "ok"}, _fmt(ctx))


@vertex.command("delete")
@click.argument("vid", type=int)
@click.option("--force", is_flag=True, help="Hard-delete (irreversible)")
@click.option("--graph", help="Target graph (default: graph0)")
@click.pass_context
def vertex_delete(ctx, vid, force, graph):
    """Delete a vertex. Use --force for hard delete."""
    c = _client(ctx)
    c.delete_vertex(vid, force, graph)
    _output({"status": "ok"}, _fmt(ctx))


@vertex.command("get-meta")
@click.argument("vid", type=int)
@click.option("--graph", help="Target graph (default: graph0)")
@click.pass_context
def vertex_get_meta(ctx, vid, graph):
    """Get vertex metadata (rank, atime, status)."""
    c = _client(ctx)
    _output(c.get_vertex_meta(vid, graph).model_dump(), _fmt(ctx))


@vertex.command("update-meta")
@click.argument("vid", type=int)
@click.option("--rank", type=int, help="New rank value")
@click.option("--graph", help="Target graph (default: graph0)")
@click.pass_context
def vertex_update_meta(ctx, vid, rank, graph):
    """Update vertex metadata (rank)."""
    c = _client(ctx)
    c.update_vertex_meta(vid, {"rank": rank} if rank else {})
    _output({"status": "ok"}, _fmt(ctx))


# ── edge ───────────────────────────────────────────────────────────

@main.group()
def edge():
    """Edge CRUD."""


@edge.command("create")
@click.option("--source", required=True, type=int, help="Source vertex ID")
@click.option("--target", required=True, type=int, help="Target vertex ID")
@click.option("--name", default="", help="Edge name / relationship type")
@click.option("--labels", callback=_parse_json_arg,
              help='JSON array of labels. Example: \'["family", "marriage"]\'')
@click.option("--keywords", callback=_parse_json_arg,
              help='JSON array of keywords. Example: \'["catelyn", "tully"]\'')
@click.option("--strength", default=1.0, type=float, help="Relationship strength 0.0-1.0")
@click.option("--properties", callback=_parse_json_arg,
              help='JSON object of custom properties. Example: \'{"year": 280}\'')
@click.option("--graph", help="Target graph (default: graph0)")
@click.pass_context
def edge_create(ctx, source, target, name, labels, keywords, strength, properties, graph):
    """Create a new edge between two vertices."""
    c = _client(ctx)
    _output(c.create_edge(source, target, name, labels, keywords, strength, properties, graph).model_dump(), _fmt(ctx))


@edge.command("update")
@click.argument("eid", type=int)
@click.option("--name", help="New edge name")
@click.option("--labels", callback=_parse_json_arg,
              help='JSON array of labels. Example: \'["family", "marriage"]\'')
@click.option("--keywords", callback=_parse_json_arg,
              help='JSON array of keywords. Example: \'["catelyn", "tully"]\'')
@click.option("--strength", type=float, help="New strength value 0.0-1.0")
@click.option("--properties", callback=_parse_json_arg,
              help='JSON object of custom properties. Example: \'{"year": 280}\'')
@click.option("--graph", help="Target graph (default: graph0)")
@click.pass_context
def edge_update(ctx, eid, name, labels, keywords, strength, properties, graph):
    """Update an existing edge's attributes."""
    c = _client(ctx)
    c.update_edge(eid, name, labels, keywords, strength, properties, graph)
    _output({"status": "ok"}, _fmt(ctx))


@edge.command("delete")
@click.argument("eid", type=int)
@click.option("--force", is_flag=True, help="Hard-delete (irreversible)")
@click.option("--graph", help="Target graph (default: graph0)")
@click.pass_context
def edge_delete(ctx, eid, force, graph):
    """Delete an edge. Use --force for hard delete."""
    c = _client(ctx)
    c.delete_edge(eid, force, graph)
    _output({"status": "ok"}, _fmt(ctx))


@edge.command("get-meta")
@click.argument("eid", type=int)
@click.option("--graph", help="Target graph (default: graph0)")
@click.pass_context
def edge_get_meta(ctx, eid, graph):
    """Get edge metadata (rank, atime, status)."""
    c = _client(ctx)
    _output(c.get_edge_meta(eid, graph).model_dump(), _fmt(ctx))


@edge.command("update-meta")
@click.argument("eid", type=int)
@click.option("--rank", type=int, help="New rank value")
@click.option("--graph", help="Target graph (default: graph0)")
@click.pass_context
def edge_update_meta(ctx, eid, rank, graph):
    """Update edge metadata (rank)."""
    c = _client(ctx)
    c.update_edge_meta(eid, {"rank": rank} if rank else {})
    _output({"status": "ok"}, _fmt(ctx))


# ── gremlin ────────────────────────────────────────────────────────

@main.group()
def gremlin():
    """Gremlin query pipeline."""


@gremlin.command("execute")
@click.option("--steps", required=True, callback=_parse_json_arg,
              help='JSON array of pipeline step objects. Steps: V, E, has, hasNot, hasLabel, hasText, hasKey, hasValue, out, in, both, outE, inE, bothE, search, traverse, repeat, expand, limit, count, dedup, values, timeTravel, rank. Example: \'[{"step":"V","ids":[1]},{"step":"out","labels":["married_to"]}]\'')
@click.option("--graph", help="Target graph (default: graph0)")
@click.pass_context
def gremlin_execute(ctx, steps, graph):
    """Execute a Gremlin pipeline query."""
    c = _client(ctx)
    resp = c.execute_gremlin(steps, graph)
    _output(resp.model_dump(), _fmt(ctx))


# ── search ──────────────────────────────────────────────────────────


@main.command()
@click.option("--text", required=True, help="Search text")
@click.option("--mode", default="greedy", show_default=True, help="Search mode: greedy or exact")
@click.option("--limit", default=20, type=int, help="Max results")
@click.option("--graph", help="Graph name (default: graph0)")
@click.pass_context
def search(ctx, text, mode, limit, graph):
    """Full-text search across vertices and edges."""
    c = _client(ctx)
    resp = c.search(text, mode, limit, graph=graph)
    _output(resp.model_dump(), _fmt(ctx))


# ── document ───────────────────────────────────────────────────────

@main.group()
def document():
    """Document management."""


@document.command("list")
@click.option("--graph", help="Filter by graph (default: graph0)")
@click.pass_context
def document_list(ctx, graph):
    """List all documents."""
    c = _client(ctx)
    docs = c.list_documents(graph)
    _output([d.model_dump() for d in docs], _fmt(ctx))


@document.command("create")
@click.option("--title", required=True, help="Document title")
@click.option("--content", required=True, help="Document content (markdown)")
@click.option("--tags", callback=_parse_json_arg,
              help='JSON array of tags. Example: \'["important", "research"]\'')
@click.option("--graph", help="Target graph for extraction (default: graph0)")
@click.pass_context
def document_create(ctx, title, content, tags, graph):
    """Create a new document."""
    c = _client(ctx)
    _output(c.create_document(title, content, tags, graph), _fmt(ctx))


@document.command("get")
@click.argument("doc_id")
@click.pass_context
def document_get(ctx, doc_id):
    """Get document metadata by ID."""
    c = _client(ctx)
    _output(c.get_document(doc_id).model_dump(), _fmt(ctx))


@document.command("update")
@click.argument("doc_id")
@click.option("--title", help="New title")
@click.option("--tags", callback=_parse_json_arg,
              help='JSON array of tags. Example: \'["important", "research"]\'')
@click.pass_context
def document_update(ctx, doc_id, title, tags):
    """Update document metadata."""
    c = _client(ctx)
    _output(c.update_document(doc_id, title, tags), _fmt(ctx))


@document.command("delete")
@click.argument("doc_id")
@click.pass_context
def document_delete(ctx, doc_id):
    """Delete a document."""
    c = _client(ctx)
    c.delete_document(doc_id)
    _output({"status": "ok"}, _fmt(ctx))


@document.command("get-content")
@click.argument("doc_id")
@click.pass_context
def document_get_content(ctx, doc_id):
    """Get the raw content (markdown body) of a document."""
    c = _client(ctx)
    content = c.get_document_content(doc_id)
    if _fmt(ctx) == "json":
        _output({"content": content}, "json")
    else:
        click.echo(content)


@document.command("extract")
@click.argument("doc_id")
@click.option("--graph", help="Target graph for extracted entities (default: graph0)")
@click.option("--model", help="LLM model key (overrides settings default)")
@click.pass_context
def document_extract(ctx, doc_id, graph, model):
    """Submit a document for background entity/relation extraction."""
    c = _client(ctx)
    _output(c.extract_document(doc_id, graph, model).model_dump(), _fmt(ctx))


# ── task ───────────────────────────────────────────────────────────

@main.group()
def task():
    """Async task management (extraction, etc.)."""


@task.command("list")
@click.pass_context
def task_list(ctx):
    """List all async tasks (newest first)."""
    c = _client(ctx)
    tasks = c.list_tasks()
    _output([t.model_dump() for t in tasks], _fmt(ctx))


@task.command("get")
@click.option("--task-id", required=True, help="Task UUID")
@click.pass_context
def task_get(ctx, task_id):
    """Get task status and progress by ID."""
    c = _client(ctx)
    _output(c.get_task(task_id).model_dump(), _fmt(ctx))


@task.command("wait")
@click.option("--task-id", required=True, help="Task UUID")
@click.option("--poll-interval", default=1.0, type=float, help="Poll interval in seconds")
@click.option("--timeout", default=300.0, type=float, help="Max wait time in seconds")
@click.pass_context
def task_wait(ctx, task_id, poll_interval, timeout):
    """Wait for a task to complete (poll until done or timeout)."""
    c = _client(ctx)
    task = c.wait_for_extraction(task_id, poll_interval, timeout)
    _output(task.model_dump(), _fmt(ctx))


# ── settings ───────────────────────────────────────────────────────

@main.group()
def settings():
    """Settings management."""


@settings.command("get-search")
@click.pass_context
def settings_get_search(ctx):
    """Get current search settings (greedy/exact modes)."""
    c = _client(ctx)
    _output(c.get_search_settings().model_dump(), _fmt(ctx))


@settings.command("set-search")
@click.option("--config", required=True, callback=_parse_json_arg,
              help='JSON: search config. Fields: greedy: {mode: "prefix"|"fuzzy"}, exact: {}. Example: \'{"greedy": {"mode": "prefix"}}\'')
@click.pass_context
def settings_set_search(ctx, config):
    """Update search settings (greedy/exact modes)."""
    c = _client(ctx)
    _output(c.set_search_settings(config).model_dump(), _fmt(ctx))


@settings.command("get-llm")
@click.pass_context
def settings_get_llm(ctx):
    """Get LLM provider configuration."""
    c = _client(ctx)
    _output(c.get_llm_settings(), _fmt(ctx))


@settings.command("set-llm")
@click.option("--providers", callback=_parse_json_arg,
              help='JSON array of LLM provider configs. Each: {name, api_base_url, api_key, models: [...], default_model, id}. Example: \'[{"name":"openai","api_base_url":"https://api.openai.com/v1","api_key":"sk-...","models":["gpt-4"]}]\'')
@click.option("--default-model", help="Default model key (Provider/Model)")
@click.pass_context
def settings_set_llm(ctx, providers, default_model):
    """Update LLM provider and model configuration."""
    c = _client(ctx)
    _output(c.set_llm_settings(providers, default_model).model_dump(), _fmt(ctx))


@settings.command("get-rank")
@click.pass_context
def settings_get_rank(ctx):
    """Get rank decay settings."""
    c = _client(ctx)
    _output(c.get_rank_settings().model_dump(), _fmt(ctx))


@settings.command("set-rank")
@click.option("--config", required=True, callback=_parse_json_arg,
              help='JSON: rank decay config. Fields: auto_inc_rank_when_update (bool), auto_inc_rank_when_read (bool), auto_dec_rank_when_inactive (bool), inactive_after_accessed_secs (int), inactive_rank_update_period (int). Example: \'{"auto_inc_rank_when_update": false}\'')
@click.pass_context
def settings_set_rank(ctx, config):
    """Update rank decay settings."""
    c = _client(ctx)
    _output(c.set_rank_settings(config).model_dump(), _fmt(ctx))


@settings.command("get-web-search")
@click.pass_context
def settings_get_web_search(ctx):
    """Get web search provider configuration."""
    c = _client(ctx)
    _output(c.get_web_search_settings().model_dump(), _fmt(ctx))


@settings.command("set-web-search")
@click.option("--config", required=True, callback=_parse_json_arg,
              help='JSON: web search config. Fields: providers: [{id, name, search_url, method, params, headers}], default_provider (string). Example: \'{"default_provider":"bing","providers":[{"id":"bing","name":"Bing","search_url":"https://api.bing.com/search","method":"GET","params":{"q":"{query}"}}]}\'')
@click.pass_context
def settings_set_web_search(ctx, config):
    """Update web search provider configuration."""
    c = _client(ctx)
    _output(c.set_web_search_settings(config).model_dump(), _fmt(ctx))


@settings.command("get-tokenizer")
@click.pass_context
def settings_get_tokenizer(ctx):
    """Get custom tokenizer dictionary words."""
    c = _client(ctx)
    _output(c.get_tokenizer_words().model_dump(), _fmt(ctx))


@settings.command("add-tokenizer-words")
@click.option("--words", required=True, callback=_parse_json_arg,
              help='JSON array of custom words to add. Example: \'["deep learning", "knowledge graph"]\'')
@click.pass_context
def settings_add_tokenizer_words(ctx, words):
    """Add custom words to the tokenizer dictionary."""
    c = _client(ctx)
    _output(c.add_tokenizer_words(words).model_dump(), _fmt(ctx))


@settings.command("remove-tokenizer-words")
@click.option("--words", required=True, callback=_parse_json_arg,
              help='JSON array of custom words to remove. Example: \'["deep learning"]\'')
@click.pass_context
def settings_remove_tokenizer_words(ctx, words):
    """Remove custom words from the tokenizer dictionary."""
    c = _client(ctx)
    _output(c.remove_tokenizer_words(words).model_dump(), _fmt(ctx))


# ── proxy ──────────────────────────────────────────────────────────

@main.group()
def proxy():
    """Proxy services (web search, LLM)."""


@proxy.command("web-search")
@click.option("--query", required=True, help="Search query text")
@click.option("--provider-id", help="Web search provider ID (default from settings)")
@click.pass_context
def proxy_web_search(ctx, query, provider_id):
    """Execute a web search via the configured proxy."""
    c = _client(ctx)
    data = c.web_search_proxy(query, provider_id)
    if _fmt(ctx) == "json":
        _output({"data": data[:500]}, "json")
    else:
        click.echo(data[:2000])


@proxy.command("openai-models")
@click.pass_context
def proxy_openai_models(ctx):
    """List available LLM models from configured providers."""
    c = _client(ctx)
    _output(c.list_models().model_dump(), _fmt(ctx))


@proxy.command("openai-chat")
@click.option("--messages", required=True, callback=_parse_json_arg,
              help='JSON array of chat messages. Each: {role: "user"|"assistant"|"system", content: string}. Example: \'[{"role":"user","content":"Hello"}]\'')
@click.option("--model", help="Model key (Provider/Model). Falls back to default_model from settings.")
@click.option("--stream/--no-stream", default=False)
@click.pass_context
def proxy_openai_chat(ctx, messages, model, stream):
    """Send a chat completion request to the LLM (non-interactive)."""
    c = _client(ctx)
    fmt = _fmt(ctx)
    if model is None:
        try:
            llm_settings = c.get_llm_settings()
            model = llm_settings.get("llm", {}).get("default_model", "")
        except Exception:
            model = ""
    if not model:
        click.echo("Error: No model specified and no default model configured. Use --model.", err=True)
        sys.exit(1)
    resp = c.chat_completion(messages, model, stream,
                             on_chunk=lambda c: click.echo(c, nl=False) if stream else None)
    if stream:
        click.echo()  # final newline after streaming
    else:
        content = (resp.get("choices") or [{}])[0].get("message", {}).get("content", "")
        if fmt == "json":
            _output({"content": content}, "json")
        else:
            click.echo(content)


# ── chat ───────────────────────────────────────────────────────────

@main.command()
@click.option("--web-search/--no-web-search", default=True, help="Enable web search")
@click.option("--graph-search/--no-graph-search", default=True, help="Enable graph search")
@click.option("--extract-keywords/--no-extract-keywords", default=True, help="Extract keywords via LLM")
@click.option("--graph", default=None, help="Graph name for graph search (default: graph0)")
@click.option("--search-mode", default="greedy", show_default=True, help="Graph search mode")
@click.option("--model", default=None, help="LLM model key (Provider/Model)")
@click.pass_context
def chat(ctx, web_search, graph_search, extract_keywords, graph, search_mode, model):
    """Interactive chat session with the LLM, optionally using web/graph search."""
    c = _client(ctx)
    fmt = _fmt(ctx)

    # Fetch model key from settings if not specified
    if model is None:
        try:
            llm_settings = c.get_llm_settings()
            model = llm_settings.get("llm", {}).get("default_model", "")
        except Exception:
            model = ""

    if not model:
        click.echo("Error: No model specified and no default model configured. Use --model.", err=True)
        sys.exit(1)

    conversation: list[dict] = []  # list of {"role": ..., "content": ...}

    click.echo(f"Chat session started (model: {model}). Type /help for commands, /exit to quit.")
    click.echo("─" * 60)

    while True:
        try:
            user_input = click.prompt("You", prompt_suffix=" > ")
        except (EOFError, KeyboardInterrupt):
            click.echo()
            break

        if not user_input.strip():
            continue

        # Handle internal commands
        if user_input.startswith("/"):
            cmd = user_input[1:].strip().lower()
            if cmd in ("exit", "quit"):
                break
            elif cmd == "help":
                click.echo("Commands:  /exit  /clear  /graph <name>  /help")
                continue
            elif cmd.startswith("graph "):
                graph = cmd[6:].strip()
                click.echo(f"Switched to graph: {graph}")
                continue
            elif cmd == "clear":
                conversation.clear()
                click.echo("Conversation cleared.")
                continue
            else:
                click.echo(f"Unknown command: {cmd}")
                continue

        # Add user message to conversation
        conversation.append({"role": "user", "content": user_input})

        search_context = ""
        search_detail = ""
        search_query = user_input

        # ── Extract keywords once (shared by web + graph search) ──
        if extract_keywords and (web_search or graph_search):
            click.echo("  🔑 Extracting keywords...", err=True)
            kw_msgs = conversation[-6:]
            kw_msgs.insert(0, {
                "role": "system",
                "content": "Extract 2-5 concise search keywords from the conversation. Return ONLY keywords separated by spaces.",
            })
            kw_resp = c.chat_completion(kw_msgs, model=model, stream=False)
            kw_content = (kw_resp.get("choices") or [{}])[0].get("message", {}).get("content", "").strip()
            if kw_content and len(kw_content.split()) <= 8:
                search_query = kw_content

        # ── Web search ──
        if web_search:
            try:
                ws_config = c.get_web_search_settings()
                provider_id = ws_config.default_provider

                click.echo(f"  🌐 Searching web... ({provider_id})", err=True)
                raw_html = c.web_search_proxy(search_query, provider_id)
                search_context = raw_html[:32000]
            except Exception as e:
                click.echo(f"  ⚠️ Web search error: {e}", err=True)

        # ── Graph search ──
        if graph_search:
            try:
                click.echo(f"  🔍 Searching graph...", err=True)
                graph_resp = c.search(search_query, mode=search_mode, graph=graph)
                if graph_resp.data:
                    items = []
                    for item in graph_resp.data:
                        if isinstance(item, dict):
                            name = item.get("name", f"#{item.get('id', '?')}")
                            items.append(f"- {name} ({item.get('type', '?')})")
                        else:
                            items.append(f"- {item}")
                    search_detail = "\n".join(items)
            except Exception as e:
                click.echo(f"  ⚠️ Graph search error: {e}", err=True)

        # ── Build messages ──
        llm_msgs = list(conversation)

        if search_context:
            llm_msgs.insert(0, {
                "role": "system",
                "content": f"The following information was retrieved from web search. Use it to answer the user's question.\n\n{search_context}",
            })
        if search_detail:
            llm_msgs.insert(0, {
                "role": "system",
                "content": f"The following information was retrieved from the knowledge graph. Use it to answer the user's question.\n{search_detail}",
            })

        # ── Call LLM (streaming) ──
        try:
            full_content = []
            click.echo("  🤖 Calling LLM ...", err=True)
            click.echo()  # blank line before response
            resp = c.chat_completion(llm_msgs, model=model, stream=True,
                                     on_chunk=lambda c: (full_content.append(c), click.echo(c, nl=False)))
            click.echo()  # final newline
            content = "".join(full_content)
            if content:
                conversation.append({"role": "assistant", "content": content})
            else:
                click.echo("(empty response)")
        except Exception as e:
            click.echo(f"⚠️ Error: {e}", err=True)

        click.echo()


if __name__ == "__main__":
    main()
