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
@click.option("--base-url", default="http://127.0.0.1:8080", show_default=True, help="Backend server URL")
@click.option("--api-key", default=None, help="API key for authentication")
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
@click.option("--description", default="")
@click.option("--time-travel/--no-time-travel", default=False)
@click.pass_context
def graph_create(ctx, name, description, time_travel):
    c = _client(ctx)
    _output(c.create_graph(name, description, time_travel).model_dump(), _fmt(ctx))


@graph.command("set-default")
@click.argument("name")
@click.pass_context
def graph_set_default(ctx, name):
    c = _client(ctx)
    _output(c.set_default_graph(name).model_dump(), _fmt(ctx))


@graph.command("delete")
@click.argument("name")
@click.option("--force", is_flag=True, default=False)
@click.pass_context
def graph_delete(ctx, name, force):
    c = _client(ctx)
    _output(c.delete_graph(name, force).model_dump(), _fmt(ctx))


@graph.command("update-meta")
@click.argument("name")
@click.option("--description")
@click.option("--time-travel", type=bool)
@click.pass_context
def graph_update_meta(ctx, name, description, time_travel):
    c = _client(ctx)
    _output(c.update_graph_meta(name, description, time_travel).model_dump(), _fmt(ctx))


@graph.command("get-config")
@click.argument("name")
@click.pass_context
def graph_get_config(ctx, name):
    c = _client(ctx)
    _output(c.get_graph_config(name), _fmt(ctx))


@graph.command("set-config")
@click.argument("name")
@click.option("--config", required=True, callback=_parse_json_arg)
@click.pass_context
def graph_set_config(ctx, name, config):
    c = _client(ctx)
    _output(c.set_graph_config(name, config).model_dump(), _fmt(ctx))


# ── vertex ─────────────────────────────────────────────────────────

@main.group()
def vertex():
    """Vertex CRUD."""


@vertex.command("create")
@click.option("--name", default="")
@click.option("--labels", default=None, callback=_parse_json_arg)
@click.option("--keywords", default=None, callback=_parse_json_arg)
@click.option("--properties", default=None, callback=_parse_json_arg)
@click.option("--graph", default=None)
@click.pass_context
def vertex_create(ctx, name, labels, keywords, properties, graph):
    c = _client(ctx)
    _output(c.create_vertex(name, labels, keywords, properties, graph).model_dump(), _fmt(ctx))


@vertex.command("update")
@click.argument("vid", type=int)
@click.option("--name")
@click.option("--labels", callback=_parse_json_arg)
@click.option("--keywords", callback=_parse_json_arg)
@click.option("--properties", callback=_parse_json_arg)
@click.option("--graph")
@click.pass_context
def vertex_update(ctx, vid, name, labels, keywords, properties, graph):
    c = _client(ctx)
    c.update_vertex(vid, name, labels, keywords, properties, graph)
    _output({"status": "ok"}, _fmt(ctx))


@vertex.command("delete")
@click.argument("vid", type=int)
@click.option("--force", is_flag=True)
@click.option("--graph")
@click.pass_context
def vertex_delete(ctx, vid, force, graph):
    c = _client(ctx)
    c.delete_vertex(vid, force, graph)
    _output({"status": "ok"}, _fmt(ctx))


@vertex.command("get-meta")
@click.argument("vid", type=int)
@click.option("--graph")
@click.pass_context
def vertex_get_meta(ctx, vid, graph):
    c = _client(ctx)
    _output(c.get_vertex_meta(vid, graph).model_dump(), _fmt(ctx))


@vertex.command("update-meta")
@click.argument("vid", type=int)
@click.option("--rank", type=int)
@click.option("--graph")
@click.pass_context
def vertex_update_meta(ctx, vid, rank, graph):
    c = _client(ctx)
    c.update_vertex_meta(vid, {"rank": rank} if rank else {})
    _output({"status": "ok"}, _fmt(ctx))


# ── edge ───────────────────────────────────────────────────────────

@main.group()
def edge():
    """Edge CRUD."""


@edge.command("create")
@click.option("--source", required=True, type=int)
@click.option("--target", required=True, type=int)
@click.option("--name", default="")
@click.option("--labels", callback=_parse_json_arg)
@click.option("--keywords", callback=_parse_json_arg)
@click.option("--strength", default=1.0, type=float)
@click.option("--properties", callback=_parse_json_arg)
@click.option("--graph")
@click.pass_context
def edge_create(ctx, source, target, name, labels, keywords, strength, properties, graph):
    c = _client(ctx)
    _output(c.create_edge(source, target, name, labels, keywords, strength, properties, graph).model_dump(), _fmt(ctx))


@edge.command("update")
@click.argument("eid", type=int)
@click.option("--name")
@click.option("--labels", callback=_parse_json_arg)
@click.option("--keywords", callback=_parse_json_arg)
@click.option("--strength", type=float)
@click.option("--properties", callback=_parse_json_arg)
@click.option("--graph")
@click.pass_context
def edge_update(ctx, eid, name, labels, keywords, strength, properties, graph):
    c = _client(ctx)
    c.update_edge(eid, name, labels, keywords, strength, properties, graph)
    _output({"status": "ok"}, _fmt(ctx))


@edge.command("delete")
@click.argument("eid", type=int)
@click.option("--force", is_flag=True)
@click.option("--graph")
@click.pass_context
def edge_delete(ctx, eid, force, graph):
    c = _client(ctx)
    c.delete_edge(eid, force, graph)
    _output({"status": "ok"}, _fmt(ctx))


@edge.command("get-meta")
@click.argument("eid", type=int)
@click.option("--graph")
@click.pass_context
def edge_get_meta(ctx, eid, graph):
    c = _client(ctx)
    _output(c.get_edge_meta(eid, graph).model_dump(), _fmt(ctx))


@edge.command("update-meta")
@click.argument("eid", type=int)
@click.option("--rank", type=int)
@click.option("--graph")
@click.pass_context
def edge_update_meta(ctx, eid, rank, graph):
    c = _client(ctx)
    c.update_edge_meta(eid, {"rank": rank} if rank else {})
    _output({"status": "ok"}, _fmt(ctx))


# ── gremlin ────────────────────────────────────────────────────────

@main.group()
def gremlin():
    """Gremlin queries and search."""


@gremlin.command("execute")
@click.option("--steps", required=True, callback=_parse_json_arg, help="JSON array of step objects")
@click.option("--graph")
@click.pass_context
def gremlin_execute(ctx, steps, graph):
    c = _client(ctx)
    resp = c.execute_gremlin(steps, graph)
    _output(resp.model_dump(), _fmt(ctx))


@gremlin.command("search")
@click.option("--text", required=True)
@click.option("--mode", default="greedy", show_default=True)
@click.option("--limit", default=20, type=int)
@click.option("--graph")
@click.pass_context
def gremlin_search(ctx, text, mode, limit, graph):
    c = _client(ctx)
    resp = c.search(text, mode, limit, graph=graph)
    _output(resp.model_dump(), _fmt(ctx))


# ── document ───────────────────────────────────────────────────────

@main.group()
def document():
    """Document management."""


@document.command("list")
@click.option("--graph")
@click.pass_context
def document_list(ctx, graph):
    c = _client(ctx)
    resp = c.list_documents(graph)
    _output(resp.model_dump(), _fmt(ctx))


@document.command("create")
@click.option("--title", required=True)
@click.option("--content", required=True)
@click.option("--tags", callback=_parse_json_arg)
@click.option("--graph")
@click.pass_context
def document_create(ctx, title, content, tags, graph):
    c = _client(ctx)
    _output(c.create_document(title, content, tags, graph), _fmt(ctx))


@document.command("get")
@click.argument("doc_id")
@click.pass_context
def document_get(ctx, doc_id):
    c = _client(ctx)
    _output(c.get_document(doc_id).model_dump(), _fmt(ctx))


@document.command("update")
@click.argument("doc_id")
@click.option("--title")
@click.option("--tags", callback=_parse_json_arg)
@click.pass_context
def document_update(ctx, doc_id, title, tags):
    c = _client(ctx)
    _output(c.update_document(doc_id, title, tags), _fmt(ctx))


@document.command("delete")
@click.argument("doc_id")
@click.pass_context
def document_delete(ctx, doc_id):
    c = _client(ctx)
    c.delete_document(doc_id)
    _output({"status": "ok"}, _fmt(ctx))


@document.command("get-content")
@click.argument("doc_id")
@click.pass_context
def document_get_content(ctx, doc_id):
    c = _client(ctx)
    content = c.get_document_content(doc_id)
    if _fmt(ctx) == "json":
        _output({"content": content}, "json")
    else:
        click.echo(content)


# ── extract ────────────────────────────────────────────────────────

@main.group()
def extract():
    """Extraction task management."""


@extract.command("submit")
@click.option("--document-id", required=True)
@click.option("--graph")
@click.option("--model")
@click.pass_context
def extract_submit(ctx, document_id, graph, model):
    c = _client(ctx)
    _output(c.submit_extraction(document_id, graph, model).model_dump(), _fmt(ctx))


@extract.command("get-task")
@click.option("--task-id", required=True)
@click.pass_context
def extract_get_task(ctx, task_id):
    c = _client(ctx)
    _output(c.get_extraction_task(task_id).model_dump(), _fmt(ctx))


@extract.command("list-tasks")
@click.pass_context
def extract_list_tasks(ctx):
    c = _client(ctx)
    tasks = c.list_extraction_tasks()
    _output([t.model_dump() for t in tasks], _fmt(ctx))


@extract.command("wait")
@click.option("--task-id", required=True)
@click.option("--poll-interval", default=1.0, type=float)
@click.option("--timeout", default=300.0, type=float)
@click.pass_context
def extract_wait(ctx, task_id, poll_interval, timeout):
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
    c = _client(ctx)
    _output(c.get_search_settings().model_dump(), _fmt(ctx))


@settings.command("set-search")
@click.option("--config", required=True, callback=_parse_json_arg)
@click.pass_context
def settings_set_search(ctx, config):
    c = _client(ctx)
    _output(c.set_search_settings(config).model_dump(), _fmt(ctx))


@settings.command("get-llm")
@click.pass_context
def settings_get_llm(ctx):
    c = _client(ctx)
    _output(c.get_llm_settings(), _fmt(ctx))


@settings.command("set-llm")
@click.option("--providers", callback=_parse_json_arg)
@click.option("--default-model")
@click.pass_context
def settings_set_llm(ctx, providers, default_model):
    c = _client(ctx)
    _output(c.set_llm_settings(providers, default_model).model_dump(), _fmt(ctx))


@settings.command("get-rank")
@click.pass_context
def settings_get_rank(ctx):
    c = _client(ctx)
    _output(c.get_rank_settings().model_dump(), _fmt(ctx))


@settings.command("set-rank")
@click.option("--config", required=True, callback=_parse_json_arg)
@click.pass_context
def settings_set_rank(ctx, config):
    c = _client(ctx)
    _output(c.set_rank_settings(config).model_dump(), _fmt(ctx))


@settings.command("get-web-search")
@click.pass_context
def settings_get_web_search(ctx):
    c = _client(ctx)
    _output(c.get_web_search_settings().model_dump(), _fmt(ctx))


@settings.command("set-web-search")
@click.option("--config", required=True, callback=_parse_json_arg)
@click.pass_context
def settings_set_web_search(ctx, config):
    c = _client(ctx)
    _output(c.set_web_search_settings(config).model_dump(), _fmt(ctx))


@settings.command("proxy")
@click.option("--query", required=True)
@click.option("--provider-id")
@click.pass_context
def settings_proxy(ctx, query, provider_id):
    c = _client(ctx)
    data = c.web_search_proxy(query, provider_id)
    if _fmt(ctx) == "json":
        _output({"data": data[:500]}, "json")
    else:
        click.echo(data[:2000])


@settings.command("get-tokenizer")
@click.pass_context
def settings_get_tokenizer(ctx):
    c = _client(ctx)
    _output(c.get_tokenizer_words().model_dump(), _fmt(ctx))


@settings.command("add-tokenizer-words")
@click.option("--words", required=True, callback=_parse_json_arg)
@click.pass_context
def settings_add_tokenizer_words(ctx, words):
    c = _client(ctx)
    _output(c.add_tokenizer_words(words).model_dump(), _fmt(ctx))


@settings.command("remove-tokenizer-words")
@click.option("--words", required=True, callback=_parse_json_arg)
@click.pass_context
def settings_remove_tokenizer_words(ctx, words):
    c = _client(ctx)
    _output(c.remove_tokenizer_words(words).model_dump(), _fmt(ctx))


# ── maas ───────────────────────────────────────────────────────────

@main.group()
def maas():
    """MaaS proxy (OpenAI-compatible)."""


@maas.command("list-models")
@click.pass_context
def maas_list_models(ctx):
    c = _client(ctx)
    _output(c.list_models().model_dump(), _fmt(ctx))


@maas.command("chat")
@click.option("--messages", required=True, callback=_parse_json_arg)
@click.option("--model")
@click.option("--stream/--no-stream", default=False)
@click.pass_context
def maas_chat(ctx, messages, model, stream):
    c = _client(ctx)
    resp = c.chat_completion(messages, model, stream)
    _output(resp, _fmt(ctx))


# ── chat ───────────────────────────────────────────────────────────

@main.command()
@click.option("--web-search/--no-web-search", default=True, help="Enable web search")
@click.option("--graph-search/--no-graph-search", default=True, help="Enable graph search")
@click.option("--extract-keywords/--no-extract-keywords", default=True, help="Extract keywords via LLM")
@click.option("--graph", default="graph0", help="Graph name for graph search")
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

        # ── Web search ──
        if web_search:
            try:
                ws_config = c.get_web_search_settings()
                provider_id = ws_config.default_provider
                search_query = user_input

                # Extract keywords
                if extract_keywords:
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

                # Execute search
                click.echo(f"  🌐 Searching web... ({provider_id})", err=True)
                raw_html = c.web_search_proxy(search_query, provider_id)
                search_context = raw_html[:32000]
            except Exception as e:
                click.echo(f"  ⚠️ Web search error: {e}", err=True)

        # ── Graph search ──
        if graph_search:
            try:
                graph_query = user_input

                if extract_keywords:
                    click.echo("  🔑 Extracting keywords...", err=True)
                    kw_msgs = conversation[-6:]
                    kw_msgs.insert(0, {
                        "role": "system",
                        "content": "Extract 2-5 concise search keywords. Return ONLY keywords separated by spaces.",
                    })
                    kw_resp = c.chat_completion(kw_msgs, model=model, stream=False)
                    kw_content = (kw_resp.get("choices") or [{}])[0].get("message", {}).get("content", "").strip()
                    if kw_content and len(kw_content.split()) <= 8:
                        graph_query = kw_content

                click.echo(f"  🔍 Searching graph...", err=True)
                graph_resp = c.search(graph_query, mode=search_mode, graph=graph)
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

        # ── Call LLM ──
        try:
            click.echo("  🤖 Thinking...", err=True)
            resp = c.chat_completion(llm_msgs, model=model, stream=False)
            content = (resp.get("choices") or [{}])[0].get("message", {}).get("content", "")
            if content:
                click.echo(content)
                conversation.append({"role": "assistant", "content": content})
            else:
                click.echo("(empty response)")
        except Exception as e:
            click.echo(f"⚠️ Error: {e}", err=True)

        click.echo("─" * 60)


if __name__ == "__main__":
    main()
