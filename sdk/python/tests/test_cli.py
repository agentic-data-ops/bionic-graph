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
    mock.post("/graphs").respond(json={"name": "test-graph", "description": "desc", "time_travel": False, "created": True})
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "create", "test-graph", "--description", "desc"])
    assert result.exit_code == 0
    assert "test-graph" in result.output


def test_graph_delete(runner, mock):
    mock.delete("/graphs/test-graph").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "delete", "test-graph"])
    assert result.exit_code == 0


def test_graph_get_config(runner, mock):
    mock.get("/graphs/g0/config").respond(json={"cache_capacity": 4096})
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "get-config", "g0"])
    assert result.exit_code == 0
    assert "4096" in result.output


def test_graph_set_default(runner, mock):
    mock.put("/graphs").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "set-default", "g1"])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_graph_update_meta(runner, mock):
    mock.put("/graphs/test-graph").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "update-meta", "test-graph", "--description", "new desc"])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_graph_set_config(runner, mock):
    mock.put("/graphs/g0/config").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "set-config", "g0", "--config", '{"cache_capacity": 8192}'])
    assert result.exit_code == 0
    assert "ok" in result.output


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
    mock.get("/vertices/1/meta").respond(json={"atime": 1784464671, "ctime": 1784464671, "mtime": 1784464671, "rank": 5, "status": 0, "success": True, "version": 1})
    result = runner.invoke(main, ["--base-url", BASE_URL, "vertex", "get-meta", "1"])
    assert result.exit_code == 0
    assert "5" in result.output


def test_vertex_update(runner, mock):
    mock.put("/vertices/1").respond(json={})
    result = runner.invoke(main, ["--base-url", BASE_URL, "vertex", "update", "1", "--name", "New Name"])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_vertex_update_meta(runner, mock):
    mock.put("/vertices/1/meta").respond(json={})
    result = runner.invoke(main, ["--base-url", BASE_URL, "vertex", "update-meta", "1", "--rank", "10"])
    assert result.exit_code == 0
    assert "ok" in result.output


# ── Edge ───────────────────────────────────────────────────────────


def test_edge_create(runner, mock):
    mock.post("/edges").respond(json={"id": 1})
    result = runner.invoke(main, ["--base-url", BASE_URL, "edge", "create", "--source", "1", "--target", "2", "--name", "married_to"])
    assert result.exit_code == 0


def test_edge_delete(runner, mock):
    mock.delete("/edges/1").respond(json={})
    result = runner.invoke(main, ["--base-url", BASE_URL, "edge", "delete", "1"])
    assert result.exit_code == 0


def test_edge_update(runner, mock):
    mock.put("/edges/1").respond(json={})
    result = runner.invoke(main, ["--base-url", BASE_URL, "edge", "update", "1", "--name", "new_name"])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_edge_get_meta(runner, mock):
    mock.get("/edges/1/meta").respond(json={"atime": 1784464672, "ctime": 1784464672, "mtime": 1784464672, "rank": 3, "status": 0, "success": True, "version": 1})
    result = runner.invoke(main, ["--base-url", BASE_URL, "edge", "get-meta", "1"])
    assert result.exit_code == 0
    assert "3" in result.output


def test_edge_update_meta(runner, mock):
    mock.put("/edges/1/meta").respond(json={})
    result = runner.invoke(main, ["--base-url", BASE_URL, "edge", "update-meta", "1", "--rank", "8"])
    assert result.exit_code == 0
    assert "ok" in result.output


# ── Gremlin ────────────────────────────────────────────────────────


def test_gremlin_execute(runner, mock):
    mock.post("/gremlin").respond(json={"success": True, "data": [], "error": None})
    result = runner.invoke(main, ["--base-url", BASE_URL, "gremlin", "execute", "--steps", '[{"step":"V","ids":[1]}]'])
    assert result.exit_code == 0
    assert "true" in result.output.lower() or "success" in result.output.lower()


def test_search(runner, mock):
    """Top-level search command."""
    mock.get("/search?text=cat&mode=greedy&limit=20").respond(json={"success": True, "data": [], "error": None})
    result = runner.invoke(main, ["--base-url", BASE_URL, "search", "--text", "cat"])
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


def test_document_list(runner, mock):
    mock.get("/documents").respond(json={"documents": [{"id": "d1", "title": "Doc 1", "tags": []}]})
    result = runner.invoke(main, ["--base-url", BASE_URL, "document", "list"])
    assert result.exit_code == 0
    assert "d1" in result.output


def test_document_get(runner, mock):
    mock.get("/documents/d1").respond(json={"id": "d1", "title": "Doc 1", "tags": [], "created_at": "", "updated_at": "", "graph_name": ""})
    result = runner.invoke(main, ["--base-url", BASE_URL, "document", "get", "d1"])
    assert result.exit_code == 0
    assert "Doc 1" in result.output


def test_document_update(runner, mock):
    mock.put("/documents/d1").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "document", "update", "d1", "--title", "Updated"])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_document_delete(runner, mock):
    mock.delete("/documents/d1").respond(json={})
    result = runner.invoke(main, ["--base-url", BASE_URL, "document", "delete", "d1"])
    assert result.exit_code == 0
    assert "ok" in result.output


# ── Extract ────────────────────────────────────────────────────────


def test_document_extract(runner, mock):
    """document extract submits a document for background extraction."""
    mock.post("/documents/d1/extract").respond(json={"task_id": "t1", "status": "pending"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "document", "extract", "d1"])
    assert result.exit_code == 0
    assert "t1" in result.output


def test_task_get(runner, mock):
    mock.get("/tasks/t1").respond(json={"task_id": "t1", "task_type": "extraction", "status": "completed", "steps": [], "overall_pct": 100.0})
    result = runner.invoke(main, ["--base-url", BASE_URL, "task", "get", "--task-id", "t1"])
    assert result.exit_code == 0
    assert "completed" in result.output


def test_task_list(runner, mock):
    mock.get("/tasks").respond(json=[{"task_id": "t1", "task_type": "extraction", "status": "pending", "steps": [], "overall_pct": 0.0}])
    result = runner.invoke(main, ["--base-url", BASE_URL, "task", "list"])
    assert result.exit_code == 0
    assert "t1" in result.output


def test_task_wait(runner, mock):
    """task wait polls until completion."""
    mock.get("/tasks/t1").respond(json={"task_id": "t1", "task_type": "extraction", "status": "completed", "steps": [], "overall_pct": 100.0})
    result = runner.invoke(main, ["--base-url", BASE_URL, "task", "wait", "--task-id", "t1", "--poll-interval", "0.1", "--timeout", "5"])
    assert result.exit_code == 0
    assert "completed" in result.output


# ── Settings ───────────────────────────────────────────────────────


def test_settings_get_search(runner, mock):
    mock.get("/settings/graph/search").respond(json={"greedy": {}, "exact": {}})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "get-search"])
    assert result.exit_code == 0


def test_proxy_web_search(runner, mock):
    mock.post("/proxy/web-search").respond(json={"success": True, "data": "<html>results</html>"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "proxy", "web-search", "--query", "test"])
    assert result.exit_code == 0
    assert "results" in result.output


def test_settings_get_tokenizer(runner, mock):
    mock.get("/settings/tokenizer").respond(json={"custom_words": ["word1"]})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "get-tokenizer"])
    assert result.exit_code == 0
    assert "word1" in result.output


def test_settings_set_search(runner, mock):
    mock.put("/settings/graph/search").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "set-search", "--config", '{"greedy": {}, "exact": {}}'])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_settings_get_llm(runner, mock):
    mock.get("/settings/llm").respond(json={"llm": {"default_model": "test/model"}})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "get-llm"])
    assert result.exit_code == 0
    assert "test/model" in result.output


def test_settings_set_llm(runner, mock):
    mock.put("/settings/llm").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "set-llm", "--default-model", "gpt-4"])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_settings_get_rank(runner, mock):
    mock.get("/settings/graph/rank").respond(json={"auto_inc_rank_when_update": True, "auto_inc_rank_when_read": True, "auto_dec_rank_when_inactive": True, "inactive_after_accessed_secs": 1296000, "inactive_rank_update_period": 86400})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "get-rank"])
    assert result.exit_code == 0
    assert "1296000" in result.output


def test_settings_set_rank(runner, mock):
    mock.put("/settings/graph/rank").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "set-rank", "--config", '{"auto_inc_rank_when_update": false}'])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_settings_get_web_search(runner, mock):
    mock.get("/settings/web-search").respond(json={"providers": [], "default_provider": ""})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "get-web-search"])
    assert result.exit_code == 0
    assert "default_provider" in result.output


def test_settings_set_web_search(runner, mock):
    mock.put("/settings/web-search").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "set-web-search", "--config", '{"default_provider": "bing"}'])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_settings_add_tokenizer_words(runner, mock):
    mock.post("/settings/tokenizer/words").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "add-tokenizer-words", "--words", '["word1","word2"]'])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_settings_remove_tokenizer_words(runner, mock):
    mock.delete("/settings/tokenizer/words").respond(json={"status": "ok"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "settings", "remove-tokenizer-words", "--words", '["word1"]'])
    assert result.exit_code == 0
    assert "ok" in result.output


# ── MaaS ───────────────────────────────────────────────────────────


def test_proxy_list_models(runner, mock):
    mock.get("/proxy/openai/v1/models").respond(json={"data": [{"id": "m1"}], "defaultModel": "m1"})
    result = runner.invoke(main, ["--base-url", BASE_URL, "proxy", "openai-models"])
    assert result.exit_code == 0
    assert "m1" in result.output


def test_proxy_chat(runner, mock):
    """proxy openai-chat with --model."""
    mock.post("/proxy/openai/v1/chat/completions").respond(json={"choices": [{"message": {"content": "Hello"}}]})
    result = runner.invoke(main, ["--base-url", BASE_URL, "proxy", "openai-chat",
                                   "--messages", '[{"role":"user","content":"Hi"}]',
                                   "--model", "test/model"])
    assert result.exit_code == 0
    assert "Hello" in result.output


def test_proxy_chat_with_model(runner, mock):
    """proxy openai-chat with --model specified."""
    mock.post("/proxy/openai/v1/chat/completions").respond(
        json={"choices": [{"message": {"content": "Model response"}}]}
    )
    result = runner.invoke(main, ["--base-url", BASE_URL, "proxy", "openai-chat",
                                   "--messages", '[{"role":"user","content":"Hi"}]',
                                   "--model", "test/model"])
    assert result.exit_code == 0
    assert "Model response" in result.output


def test_proxy_chat_stream(runner, mock):
    """proxy openai-chat with --stream flag."""
    mock.post("/proxy/openai/v1/chat/completions").respond(
        content_type="text/event-stream",
        text="data: {\"choices\":[{\"delta\":{\"content\":\"Stream\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" reply\"}}]}\n\ndata: [DONE]\n\n",
    )
    result = runner.invoke(main, ["--base-url", BASE_URL, "proxy", "openai-chat",
                                   "--messages", '[{"role":"user","content":"Hi"}]',
                                   "--model", "test/model",
                                   "--stream"])
    assert result.exit_code == 0
    assert "Stream reply" in result.output


def test_proxy_chat_auto_model(runner, mock):
    """proxy openai-chat auto-fetches default model from settings."""
    mock.get("/settings/llm").respond(json={"llm": {"default_model": "auto/model"}})
    mock.post("/proxy/openai/v1/chat/completions").respond(
        json={"choices": [{"message": {"content": "Auto model reply"}}]}
    )
    result = runner.invoke(main, ["--base-url", BASE_URL, "proxy", "openai-chat",
                                   "--messages", '[{"role":"user","content":"Hi"}]'])
    assert result.exit_code == 0
    assert "Auto model reply" in result.output


def test_proxy_chat_no_model_error(runner, mock):
    """proxy openai-chat without --model and no default model should error."""
    mock.get("/settings/llm").respond(json={"llm": {}})
    result = runner.invoke(main, ["--base-url", BASE_URL, "proxy", "openai-chat",
                                   "--messages", '[{"role":"user","content":"Hi"}]'])
    assert result.exit_code != 0
    assert "No model specified" in result.output


# ── Chat ────────────────────────────────────────────────────────────


def test_chat_basic(runner, mock):
    """Chat with --model specified and all search disabled, then /exit."""
    mock.post("/proxy/openai/v1/chat/completions").respond(
        content_type="text/event-stream",
        text="data: {\"choices\":[{\"delta\":{\"content\":\"Hello from LLM\"}}]}\n\ndata: [DONE]\n\n",
    )
    result = runner.invoke(main, ["--base-url", BASE_URL, "chat", "--model", "test/model",
                                   "--no-web-search", "--no-graph-search"],
                           input="hello\n/exit\n")
    assert result.exit_code == 0
    assert "Hello from LLM" in result.output


def test_chat_with_web_search(runner, mock):
    """Chat with web search enabled."""
    mock.get("/settings/web-search").respond(json={
        "providers": [{"id": "web", "name": "Web", "search_url": "http://example.com", "method": "GET"}],
        "default_provider": "web",
    })
    mock.post("/proxy/web-search").respond(json={"success": True, "data": "<html>search result</html>"})
    # Keyword extraction (non-stream) and final response (stream) share same URL
    # Use side_effect to match on request body
    def chat_handler(request):
        body = json.loads(request.content)
        if body.get("stream"):
            return Response(200, text="data: {\"choices\":[{\"delta\":{\"content\":\"Answer from LLM\"}}]}\n\ndata: [DONE]\n\n",
                            headers={"content-type": "text/event-stream"})
        else:
            return Response(200, json={"choices": [{"message": {"content": "key words"}}]})
    mock.post("/proxy/openai/v1/chat/completions").side_effect = chat_handler
    result = runner.invoke(main, ["--base-url", BASE_URL, "chat", "--model", "test/model",
                                   "--no-graph-search"],
                           input="hello\n/exit\n")
    assert result.exit_code == 0
    assert "Answer from LLM" in result.output


def test_chat_no_model_error(runner, mock):
    """Chat without --model and no default model configured should error."""
    mock.get("/settings/llm").respond(json={"llm": {}})
    result = runner.invoke(main, ["--base-url", BASE_URL, "chat", "--no-web-search", "--no-graph-search"],
                           input="/exit\n")
    assert result.exit_code != 0
    assert "No model specified" in result.output


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


def test_invalid_json_arg(runner, mock):
    """Pass invalid JSON to --config should fail."""
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "set-config", "g0", "--config", "not-json"])
    assert result.exit_code != 0
    assert "Invalid JSON" in result.output


def test_server_error(runner, mock):
    """Backend returns 500 should result in non-zero exit."""
    mock.post("/graphs").respond(status_code=500, text="Internal Server Error")
    result = runner.invoke(main, ["--base-url", BASE_URL, "graph", "create", "fail-graph"])
    assert result.exit_code != 0


# ── Global options ─────────────────────────────────────────────────


def test_global_timeout(runner, mock):
    """--timeout should be accepted and passed through."""
    mock.get("/health").respond(json={"status": "ok", "version": "0.1.0", "uptime_secs": 0, "graphs": 1, "cluster_enabled": False})
    result = runner.invoke(main, ["--base-url", BASE_URL, "--timeout", "15", "health", "check"])
    assert result.exit_code == 0
    assert "ok" in result.output


def test_global_api_key(runner, mock):
    """--api-key should be accepted."""
    mock.get("/health").respond(json={"status": "ok", "version": "0.1.0", "uptime_secs": 0, "graphs": 1, "cluster_enabled": False})
    result = runner.invoke(main, ["--base-url", BASE_URL, "--api-key", "sk-test123", "health", "check"])
    assert result.exit_code == 0
    assert "ok" in result.output
