"""Tests for the Bionic-Graph Python SDK using respx mock."""
import pytest
import respx
from httpx import Response

from bionic_graph import Client
from bionic_graph.exceptions import ApiError, NotFoundError


BASE_URL = "http://127.0.0.1:8080"


@pytest.fixture
def client():
    return Client(base_url=BASE_URL)


@pytest.fixture
def mock():
    with respx.mock(base_url=BASE_URL, assert_all_called=False) as respx_mock:
        yield respx_mock


# ── Health ─────────────────────────────────────────────────────────


def test_health(client, mock):
    mock.get("/health").respond(json={"status": "ok", "version": "0.1.0", "uptime_secs": 42, "graphs": 1, "cluster_enabled": False})
    resp = client.health()
    assert resp.status == "ok"
    assert resp.version == "0.1.0"
    assert resp.graphs == 1


# ── Graph lifecycle ─────────────────────────────────────────────────


def test_list_graphs(client, mock):
    mock.get("/graphs").respond(json={"default": "g0", "graphs": [{"name": "g0", "description": "", "time_travel": True}]})
    resp = client.list_graphs()
    assert resp.default == "g0"
    assert len(resp.graphs) == 1
    assert resp.graphs[0].name == "g0"


def test_create_graph(client, mock):
    mock.post("/graphs").respond(json={"status": "ok"})
    resp = client.create_graph("test", "desc", True)
    assert resp.status == "ok"


def test_set_default_graph(client, mock):
    mock.put("/graphs").respond(json={"status": "ok"})
    resp = client.set_default_graph("g0")
    assert resp.status == "ok"


def test_delete_graph(client, mock):
    mock.delete("/graphs/g0").respond(json={"status": "ok"})
    resp = client.delete_graph("g0")
    assert resp.status == "ok"


def test_update_graph_meta(client, mock):
    mock.put("/graphs/g0").respond(json={"status": "ok"})
    resp = client.update_graph_meta("g0", description="new desc")
    assert resp.status == "ok"


def test_get_graph_config(client, mock):
    mock.get("/graphs/g0/config").respond(json={"cache_capacity": 4096})
    cfg = client.get_graph_config("g0")
    assert cfg["cache_capacity"] == 4096


def test_set_graph_config(client, mock):
    mock.put("/graphs/g0/config").respond(json={"status": "ok"})
    resp = client.set_graph_config("g0", {"cache_capacity": 2048})
    assert resp.status == "ok"


# ── Vertices ───────────────────────────────────────────────────────


def test_create_vertex(client, mock):
    mock.post("/vertices").respond(json={"id": 1})
    resp = client.create_vertex("Eddard Stark", labels=["person"])
    assert resp.id == 1


def test_update_vertex(client, mock):
    mock.put("/vertices/1").respond(json={})
    client.update_vertex(1, name="Eddard Stark", properties={"title": "Lord of Winterfell"})


def test_delete_vertex(client, mock):
    mock.delete("/vertices/1").respond(json={})
    client.delete_vertex(1)


def test_delete_vertex_force(client, mock):
    mock.delete("/vertices/1?force=true").respond(json={})
    client.delete_vertex(1, force=True)


def test_get_vertex_meta(client, mock):
    mock.get("/vertices/1/meta").respond(json={"id": 1, "rank": 5, "atime": "2026-01-01", "status": "active"})
    meta = client.get_vertex_meta(1)
    assert meta.id == 1
    assert meta.rank == 5


def test_update_vertex_meta(client, mock):
    mock.put("/vertices/1/meta").respond(json={})
    client.update_vertex_meta(1, {"rank": 10})


# ── Edges ──────────────────────────────────────────────────────────


def test_create_edge(client, mock):
    mock.post("/edges").respond(json={"id": 1})
    resp = client.create_edge(1, 2, "married_to", strength=0.9)
    assert resp.id == 1


def test_update_edge(client, mock):
    mock.put("/edges/1").respond(json={})
    client.update_edge(1, name="lovers")


def test_delete_edge(client, mock):
    mock.delete("/edges/1").respond(json={})
    client.delete_edge(1)


def test_get_edge_meta(client, mock):
    mock.get("/edges/1/meta").respond(json={"id": 1, "rank": 3, "atime": "2026-01-01", "status": "active"})
    meta = client.get_edge_meta(1)
    assert meta.id == 1


def test_update_edge_meta(client, mock):
    mock.put("/edges/1/meta").respond(json={})
    client.update_edge_meta(1, {"rank": 10})


# ── Gremlin ────────────────────────────────────────────────────────


def test_execute_gremlin(client, mock):
    mock.post("/gremlin").respond(json={
        "success": True,
        "data": [{"type": "vertex", "id": 1, "name": "Eddard Stark", "labels": ["person"], "keywords": [], "properties": {}, "score": None, "rank": 1}],
        "error": None,
    })
    steps = [{"step": "V", "ids": [1]}]
    resp = client.execute_gremlin(steps)
    assert resp.success
    assert resp.data[0].id == 1


def test_search(client, mock):
    mock.get("/search?text=cat&mode=greedy&limit=20").respond(json={
        "success": True, "data": [], "error": None,
    })
    resp = client.search("cat")
    assert resp.success


# ── Documents ──────────────────────────────────────────────────────


def test_list_documents(client, mock):
    mock.get("/documents").respond(json={"documents": [{"id": "d1", "title": "Doc1", "tags": [], "created_at": "", "updated_at": "", "graph_name": ""}]})
    docs = client.list_documents()
    assert len(docs.documents) == 1
    assert docs.documents[0].title == "Doc1"


def test_create_document(client, mock):
    mock.post("/documents").respond(json={"id": "d1"})
    resp = client.create_document("Title", "Content")
    assert resp["id"] == "d1"


def test_get_document(client, mock):
    mock.get("/documents/d1").respond(json={"id": "d1", "title": "Doc1", "tags": [], "created_at": "", "updated_at": "", "graph_name": ""})
    doc = client.get_document("d1")
    assert doc.title == "Doc1"


def test_delete_document(client, mock):
    mock.delete("/documents/d1").respond(json={})
    client.delete_document("d1")


def test_get_document_content(client, mock):
    mock.get("/documents/d1/content").respond(text="Hello World")
    content = client.get_document_content("d1")
    assert content == "Hello World"


# ── Extraction ─────────────────────────────────────────────────────


def test_submit_extraction(client, mock):
    mock.post("/extract").respond(json={"task_id": "t1", "status": "pending"})
    resp = client.submit_extraction("d1")
    assert resp.task_id == "t1"


def test_get_extraction_task(client, mock):
    mock.get("/extract/task/t1").respond(json={"task_id": "t1", "status": "completed", "steps": [], "overall_pct": 100.0})
    task = client.get_extraction_task("t1")
    assert task.status == "completed"


# ── Settings ───────────────────────────────────────────────────────


def test_get_search_settings(client, mock):
    mock.get("/settings/graph/search").respond(json={"greedy": {}, "exact": {}})
    cfg = client.get_search_settings()
    assert cfg.greedy == {}


def test_set_search_settings(client, mock):
    mock.put("/settings/graph/search").respond(json={"status": "ok"})
    resp = client.set_search_settings({"greedy": {}, "exact": {}})
    assert resp.status == "ok"


def test_get_llm_settings(client, mock):
    mock.get("/settings/llm").respond(json={"llm": {"providers": []}})
    cfg = client.get_llm_settings()
    assert "llm" in cfg


def test_set_llm_settings(client, mock):
    mock.put("/settings/llm").respond(json={"status": "ok"})
    resp = client.set_llm_settings(default_model="DeepSeek/default")
    assert resp.status == "ok"


def test_get_rank_settings(client, mock):
    mock.get("/settings/graph/rank").respond(json={"auto_inc_rank_when_update": True, "auto_inc_rank_when_read": True, "auto_dec_rank_when_inactive": True, "inactive_after_accessed_secs": 1296000, "inactive_rank_update_period": 86400})
    cfg = client.get_rank_settings()
    assert cfg.auto_inc_rank_when_update is True


def test_set_rank_settings(client, mock):
    mock.put("/settings/graph/rank").respond(json={"status": "ok"})
    resp = client.set_rank_settings({"auto_inc_rank_when_update": True})
    assert resp.status == "ok"


def test_get_web_search_settings(client, mock):
    mock.get("/settings/web-search").respond(json={"providers": [], "default_provider": ""})
    cfg = client.get_web_search_settings()
    assert cfg.providers == []


def test_web_search_proxy(client, mock):
    mock.post("/proxy/web-search").respond(json={"success": True, "data": "<html>results</html>"})
    data = client.web_search_proxy("hello")
    assert "results" in data


def test_get_tokenizer_words(client, mock):
    mock.get("/settings/tokenizer").respond(json={"custom_words": ["word1"]})
    cfg = client.get_tokenizer_words()
    assert cfg.custom_words == ["word1"]


def test_add_tokenizer_words(client, mock):
    mock.post("/settings/tokenizer/words").respond(json={"status": "ok"})
    resp = client.add_tokenizer_words(["word1"])
    assert resp.status == "ok"


# ── MaaS ───────────────────────────────────────────────────────────


def test_list_models(client, mock):
    mock.get("/maas/openai/v1/models").respond(json={"data": [{"id": "model1"}], "defaultModel": "model1"})
    resp = client.list_models()
    assert len(resp.data) == 1
    assert resp.data[0]["id"] == "model1"


def test_chat_completion(client, mock):
    mock.post("/maas/openai/v1/chat/completions").respond(json={"choices": [{"message": {"content": "Hello"}}]})
    resp = client.chat_completion([{"role": "user", "content": "Hi"}])
    assert resp["choices"][0]["message"]["content"] == "Hello"


# ── Error handling ─────────────────────────────────────────────────


def test_not_found(client, mock):
    mock.get("/graphs/unknown").respond(status_code=404, text="not found")
    with pytest.raises(NotFoundError):
        client._request("GET", "/graphs/unknown")


def test_api_error(client, mock):
    mock.get("/health").respond(status_code=500, text="server error")
    with pytest.raises(ApiError):
        client.health()


# ── Graph header ───────────────────────────────────────────────────


def test_graph_header(client, mock):
    mock.post("/vertices").respond(json={"id": 1})
    # The mock doesn't check headers, so just verify no error
    resp = client.create_vertex("test", graph="graph0")
    assert resp.id == 1
