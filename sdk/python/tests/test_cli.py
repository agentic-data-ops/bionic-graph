"""Tests for the Bionic-Graph CLI using Click's CliRunner + respx mock."""
import json
import pytest
from click.testing import CliRunner
import respx
from httpx import Response

from bionic_graph.cli import main


BASE_URL = "http://127.0.0.1:8080"


@pytest.fixture
def runner():
    return CliRunner()


@pytest.fixture
def mock():
    with respx.mock(base_url=BASE_URL, assert_all_called=False) as m:
        yield m


# ── Health ─────────────────────────────────────────────────────────


def test_health(runner, mock):
    mock.get("/health").respond(json={"status": "ok", "version": "0.1.0", "uptime_secs": 42, "graphs": 1, "cluster_enabled": False})
    result = runner.invoke(main, ["--base-url", BASE_URL, "health", "check"])
    assert result.exit_code == 0
    assert "ok" in result.output


# ── Graph ──────────────────────────────────────────────────────────


def test_graph_list(runner, mock):
    mock.get("/graphs").respond(json={"default": "g0", "graphs": [{"name": "g0", "description": "", "time_travel": True}]})
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "list"])
    assert result.exit_code == 0
    assert "g0" in result.output


def test_graph_create(runner, mock):
    mock.post("/graphs").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "create", "test-graph", "--description", "desc"])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_graph_delete(runner, mock):
    mock.delete("/graphs/test-graph").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "delete", "test-graph"])
    assert result.exit_code == 0


def test_graph_get_config(runner, mock):
    mock.get("/graphs/g0/config").respond(json={"cache_capacity": 4096})
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "get-config", "g0"])
    assert result.exit_code == 0
    assert "4096" in result.output


# ── Vertex ─────────────────────────────────────────────────────────


def test_vertex_create(runner, mock):
    mock.post("/vertices").respond(json={"id": 1})
    result = runner.invoke(main, ["--base-url", BASE_URL, "vertex", "create", "--name", "Eddard Stark", "--labels", '["person"]'])
    assert result.exit_code == 0
    assert '"id"' in result.output or "1" in result.output


def test_vertex_delete(runner, mock):
    mock.delete("/vertices/1").respond(json={})
    result = runner.invoke(main, ["--base-url", BASE_URL, "vertex", "delete", "1"])
    assert result.exit_code == 0


def test_vertex_get_meta(runner, mock):
    mock.get("/vertices/1/meta").respond(json={"id": 1, "rank": 5, "atime": "2026-01-01", "status": "active"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "vertex", "get-meta", "1"])
    assert result.exit_code == 0
    assert "5" in result.output


# ── Edge ───────────────────────────────────────────────────────────


def test_edge_create(runner, mock):
    mock.post("/edges").respond(json={"id": 1})
    result = runner.invoke(main, ["--base-url", BASE_URL, "edge", "create", "--source", "1", "--target", "2", "--name", "married_to"])
    assert result.exit_code == 0


def test_edge_delete(runner, mock):
    mock.delete("/edges/1").respond(json={})
    result = runner.invoke(main, ["--base-url", BASE_URL, "edge", "delete", "1"])
    assert result.exit_code == 0


# ── Gremlin ────────────────────────────────────────────────────────


def test_gremlin_execute(runner, mock):
    mock.post("/gremlin").respond(json={"success": True, "data": [], "error": None})
    result = runner.invoke(main, ["--base-url", BASE_URL, "gremlin", "execute", "--steps", '[{"step":"V","ids":[1]}]'])
    assert result.exit_code == 0
    assert "true" in result.output.lower() or "success" in result.output.lower()


def test_gremlin_search(runner, mock):
    mock.get("/search?text=cat&mode=greedy&limit=20").respond(json={"success": True, "data": [], "error": None})
    result = runner.invoke(main, ["--base-url", BASE_URL, "gremlin", "search", "--text", "cat"])
    assert result.exit_code == 0


# ── Document ───────────────────────────────────────────────────────


def test_document_create(runner, mock):
    mock.post("/documents").respond(json={"id": "d1"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "document", "create", "--title", "T", "--content", "C"])
    assert result.exit_code == 0


def test_document_get_content(runner, mock):
    mock.get("/documents/d1/content").respond(text="Hello World")
    result = runner.invoke(main, ["--base-url", BASE_URL, "document", "get-content", "d1"])
    assert result.exit_code == 0
    assert "Hello World" in result.output


# ── Settings ───────────────────────────────────────────────────────


def test_settings_get_search(runner, mock):
    mock.get("/settings/graph/search").respond(json={"greedy": {}, "exact": {}})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "get-search"])
    assert result.exit_code == 0


def test_settings_proxy(runner, mock):
    mock.post("/web-search/proxy").respond(json={"success": True, "data": "<html>results</html>"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "proxy", "--query", "test"])
    assert result.exit_code == 0
    assert "results" in result.output


def test_settings_get_tokenizer(runner, mock):
    mock.get("/settings/tokenizer").respond(json={"custom_words": ["word1"]})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "get-tokenizer"])
    assert result.exit_code == 0
    assert "word1" in result.output


# ── MaaS ───────────────────────────────────────────────────────────


def test_maas_list_models(runner, mock):
    mock.get("/maas/openai/v1/models").respond(json={"data": [{"id": "m1"}], "defaultModel": "m1"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "maas", "list-models"])
    assert result.exit_code == 0
    assert "m1" in result.output


def test_maas_chat(runner, mock):
    mock.post("/maas/openai/v1/chat/completions").respond(json={"choices": [{"message": {"content": "Hello"}}]})
    result = runner.invoke(main, ["--base-url", BASE_URL, "maas", "chat", "--messages", '[{"role":"user","content":"Hi"}]'])
    assert result.exit_code == 0
    assert "Hello" in result.output


# ── JSON output ────────────────────────────────────────────────────


def test_json_output(runner, mock):
    mock.get("/health").respond(json={"status": "ok", "version": "0.1.0", "uptime_secs": 0, "graphs": 1, "cluster_enabled": False})
    result = runner.invoke(main, ["--base-url", BASE_URL, "--output", "json", "health", "check"])
    assert result.exit_code == 0
    data = json.loads(result.output)
    assert data["status"] == "ok"


# ── Error handling ─────────────────────────────────────────────────


def test_not_found(runner, mock):
    mock.get("/graphs/unknown").respond(status_code=404, text="not found")
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "get-config", "unknown"])
    assert result.exit_code != 0
