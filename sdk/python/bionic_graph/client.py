from __future__ import annotations

from typing import Any, Optional

import httpx

from .exceptions import ApiError, NotFoundError
from .models import *


class Client:
    """Python client for the Bionic-Graph REST API."""

    def __init__(
        self,
        base_url: str = "http://127.0.0.1:8080",
        api_key: Optional[str] = None,
        timeout: float = 30.0,
    ):
        self.base_url = base_url.rstrip("/")
        headers = {}
        if api_key:
            headers["Authorization"] = f"Bearer {api_key}"
        self._http = httpx.Client(base_url=self.base_url, headers=headers, timeout=timeout)

    def close(self):
        self._http.close()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    # ── Internal ────────────────────────────────────────────────────

    def _request(self, method: str, path: str, **kwargs) -> Any:
        url = f"{self.base_url}{path}" if path.startswith("/") else path
        resp = self._http.request(method, url, **kwargs)
        if resp.status_code == 404:
            raise NotFoundError(body=resp.text)
        if not resp.is_success:
            raise ApiError(resp.status_code, resp.reason_phrase or "error", resp.text)
        if resp.content:
            return resp.json()
        return {"status": "ok"}

    def _graph_header(self, graph: Optional[str]) -> dict:
        if graph:
            return {"X-Graph-Name": graph}
        return {}

    # ── 3. Health ───────────────────────────────────────────────────

    def health(self) -> HealthResponse:
        return HealthResponse.model_validate(self._request("GET", "/health"))

    # ── 4. Graph lifecycle ──────────────────────────────────────────

    def list_graphs(self) -> GraphListResponse:
        return GraphListResponse.model_validate(self._request("GET", "/graphs"))

    def create_graph(self, name: str, description: str = "", time_travel: bool = True) -> GraphCreateResponse:
        return GraphCreateResponse.model_validate(
            self._request("POST", "/graphs", json={"name": name, "description": description, "time_travel": time_travel})
        )

    def set_default_graph(self, name: str) -> StatusResponse:
        return StatusResponse.model_validate(self._request("PUT", "/graphs", json={"name": name}))

    def delete_graph(self, name: str, force: bool = False) -> StatusResponse:
        url = f"/graphs/{name}"
        if force:
            url += "?force=true"
        return StatusResponse.model_validate(self._request("DELETE", url))

    def update_graph_meta(self, name: str, description: Optional[str] = None, time_travel: Optional[bool] = None) -> StatusResponse:
        body: dict = {}
        if description is not None:
            body["description"] = description
        if time_travel is not None:
            body["time_travel"] = time_travel
        return StatusResponse.model_validate(self._request("PUT", f"/graphs/{name}", json=body))

    def get_graph_config(self, name: str) -> dict:
        return self._request("GET", f"/graphs/{name}/config")

    def set_graph_config(self, name: str, config: dict) -> StatusResponse:
        return StatusResponse.model_validate(self._request("PUT", f"/graphs/{name}/config", json=config))

    # ── 5. Vertices ─────────────────────────────────────────────────

    def create_vertex(
        self,
        name: str = "",
        labels: Optional[list[str]] = None,
        keywords: Optional[list[str]] = None,
        properties: Optional[dict] = None,
        graph: Optional[str] = None,
    ) -> IdResponse:
        body: dict = {"name": name}
        if labels:
            body["labels"] = labels
        if keywords:
            body["keywords"] = keywords
        if properties:
            body["properties"] = properties
        return IdResponse.model_validate(
            self._request("POST", "/vertices", json=body, headers=self._graph_header(graph))
        )

    def update_vertex(
        self,
        vid: int,
        name: Optional[str] = None,
        labels: Optional[list[str]] = None,
        keywords: Optional[list[str]] = None,
        properties: Optional[dict] = None,
        graph: Optional[str] = None,
    ) -> None:
        body: dict = {}
        if name is not None:
            body["name"] = name
        if labels is not None:
            body["labels"] = labels
        if keywords is not None:
            body["keywords"] = keywords
        if properties is not None:
            body["properties"] = properties
        self._request("PUT", f"/vertices/{vid}", json=body, headers=self._graph_header(graph))

    def delete_vertex(self, vid: int, force: bool = False, graph: Optional[str] = None) -> None:
        url = f"/vertices/{vid}"
        if force:
            url += "?force=true"
        self._request("DELETE", url, headers=self._graph_header(graph))

    def get_vertex_meta(self, vid: int, graph: Optional[str] = None) -> MetaResponse:
        return MetaResponse.model_validate(
            self._request("GET", f"/vertices/{vid}/meta", headers=self._graph_header(graph))
        )

    def update_vertex_meta(self, vid: int, meta: dict, graph: Optional[str] = None) -> None:
        self._request("PUT", f"/vertices/{vid}/meta", json=meta, headers=self._graph_header(graph))

    # ── 6. Edges ────────────────────────────────────────────────────

    def create_edge(
        self,
        source: int,
        target: int,
        name: str = "",
        labels: Optional[list[str]] = None,
        keywords: Optional[list[str]] = None,
        strength: float = 1.0,
        properties: Optional[dict] = None,
        graph: Optional[str] = None,
    ) -> IdResponse:
        body: dict = {"source": source, "target": target, "name": name, "strength": strength}
        if labels:
            body["labels"] = labels
        if keywords:
            body["keywords"] = keywords
        if properties:
            body["properties"] = properties
        return IdResponse.model_validate(
            self._request("POST", "/edges", json=body, headers=self._graph_header(graph))
        )

    def update_edge(
        self,
        eid: int,
        name: Optional[str] = None,
        labels: Optional[list[str]] = None,
        keywords: Optional[list[str]] = None,
        strength: Optional[float] = None,
        properties: Optional[dict] = None,
        graph: Optional[str] = None,
    ) -> None:
        body: dict = {}
        if name is not None:
            body["name"] = name
        if labels is not None:
            body["labels"] = labels
        if keywords is not None:
            body["keywords"] = keywords
        if strength is not None:
            body["strength"] = strength
        if properties is not None:
            body["properties"] = properties
        self._request("PUT", f"/edges/{eid}", json=body, headers=self._graph_header(graph))

    def delete_edge(self, eid: int, force: bool = False, graph: Optional[str] = None) -> None:
        url = f"/edges/{eid}"
        if force:
            url += "?force=true"
        self._request("DELETE", url, headers=self._graph_header(graph))

    def get_edge_meta(self, eid: int, graph: Optional[str] = None) -> MetaResponse:
        return MetaResponse.model_validate(
            self._request("GET", f"/edges/{eid}/meta", headers=self._graph_header(graph))
        )

    def update_edge_meta(self, eid: int, meta: dict, graph: Optional[str] = None) -> None:
        self._request("PUT", f"/edges/{eid}/meta", json=meta, headers=self._graph_header(graph))

    # ── 6b. Batch load ──────────────────────────────────────────────

    def batch_load(
        self,
        entities: list[dict],
        relations: list[dict],
        graph: Optional[str] = None,
        update_existing: bool = True,
    ) -> dict:
        """Batch import vertices and edges.

        Vertices are upserted by 'name'. Edges are upserted by
        (source_name, target_name, name). Edges reference vertices
        by string name, not numeric ID.

        Args:
            entities: List of {name, labels?, keywords?, properties?}
            relations: List of {source, target, name, labels?, keywords?, strength?, properties?}
            graph: Target graph name (via X-Graph-Name header).
            update_existing: If True (default), update existing vertices/edges.
                             If False, skip existing and only create new ones.

        Returns:
            Dict with vertices_created, vertices_updated, vertices_skipped,
            edges_created, edges_updated, edges_skipped.
        """
        body = {"entities": entities, "relations": relations, "update_existing": update_existing}
        return self._request("POST", "/batch/load", json=body, headers=self._graph_header(graph))

    def batch_delete(
        self,
        vertices: Optional[list[str]] = None,
        edges: Optional[list[dict]] = None,
        graph: Optional[str] = None,
    ) -> dict:
        """Batch delete vertices and edges by name.

        Vertices are identified by 'name'. All edges connected to deleted
        vertices (both incoming and outgoing) are also deleted.

        Edges are identified by {source, target, name}.

        Args:
            vertices: List of vertex names to delete.
            edges: List of {source, target, name} for edges to delete.
            graph: Target graph name (via X-Graph-Name header).

        Returns:
            Dict with vertices_deleted, edges_deleted.
        """
        body = {
            "vertices": vertices or [],
            "edges": edges or [],
        }
        return self._request("POST", "/batch/delete", json=body, headers=self._graph_header(graph))

    # ── 7. Gremlin ──────────────────────────────────────────────────

    def execute_gremlin(self, steps: list[dict], graph: Optional[str] = None) -> GremlinResponse:
        return GremlinResponse.model_validate(
            self._request("POST", "/gremlin", json={"steps": steps}, headers=self._graph_header(graph))
        )

    def search(
        self,
        text: str,
        mode: str = "greedy",
        limit: int = 20,
        min_rank: Optional[int] = None,
        graph: Optional[str] = None,
    ) -> GremlinResponse:
        params: dict = {"text": text, "mode": mode}
        if limit:
            params["limit"] = str(limit)
        if min_rank is not None:
            params["min_rank"] = str(min_rank)
        return GremlinResponse.model_validate(
            self._request("GET", "/search", params=params, headers=self._graph_header(graph))
        )

    # ── 9. Documents ────────────────────────────────────────────────

    def list_documents(self, graph: Optional[str] = None) -> list[Document]:
        """List documents. Backend returns a JSON array directly, not a wrapped object."""
        data = self._request("GET", "/documents", headers=self._graph_header(graph))
        if isinstance(data, list):
            return [Document.model_validate(d) for d in data]
        return DocumentListResponse.model_validate(data).documents

    def create_document(self, title: str, content: str, tags: Optional[list[str]] = None) -> dict:
        body: dict = {"title": title, "content": content}
        if tags:
            body["tags"] = tags
        return self._request("POST", "/documents", json=body)

    def get_document(self, doc_id: str) -> Document:
        return Document.model_validate(self._request("GET", f"/documents/{doc_id}"))

    def update_document(self, doc_id: str, title: Optional[str] = None, tags: Optional[list[str]] = None) -> dict:
        body: dict = {}
        if title is not None:
            body["title"] = title
        if tags is not None:
            body["tags"] = tags
        return self._request("PUT", f"/documents/{doc_id}", json=body)

    def delete_document(self, doc_id: str) -> None:
        self._request("DELETE", f"/documents/{doc_id}")

    def get_document_content(self, doc_id: str) -> str:
        url = f"{self.base_url}/documents/{doc_id}/content"
        resp = self._http.get(url)
        if resp.status_code == 404:
            raise NotFoundError(body=resp.text)
        if not resp.is_success:
            raise ApiError(resp.status_code, resp.reason_phrase or "error", resp.text)
        return resp.text

    # ── 10. Extraction ──────────────────────────────────────────────

    def submit_extraction(self, document_id: str, graph: Optional[str] = None, model: Optional[str] = None) -> ExtractionSubmitResponse:
        url = f"/extract"
        body: dict = {"document_id": document_id}
        if model:
            body["model"] = model
        return ExtractionSubmitResponse.model_validate(
            self._request("POST", url, json=body, headers=self._graph_header(graph))
        )

    def extract_document(self, doc_id: str, graph: Optional[str] = None, model: Optional[str] = None) -> ExtractionSubmitResponse:
        url = f"/documents/{doc_id}/extract"
        params = {}
        if model:
            params["model"] = model
        headers = self._graph_header(graph)
        return ExtractionSubmitResponse.model_validate(
            self._request("POST", url, params=params, headers=headers)
        )

    def get_extraction_task(self, task_id: str) -> ExtractionTask:
        """Get extraction task by ID. Deprecated: use get_task()."""
        return ExtractionTask.model_validate(self._request("GET", f"/tasks/{task_id}"))

    def get_task(self, task_id: str) -> ExtractionTask:
        """Get task by ID (generic task endpoint)."""
        return ExtractionTask.model_validate(self._request("GET", f"/tasks/{task_id}"))

    def list_extraction_tasks(self) -> list[ExtractionTask]:
        """List all tasks. Deprecated: use list_tasks()."""
        data = self._request("GET", "/tasks")
        return [ExtractionTask.model_validate(t) for t in data]

    def list_tasks(self) -> list[ExtractionTask]:
        """List all tasks (newest first)."""
        data = self._request("GET", "/tasks")
        return [ExtractionTask.model_validate(t) for t in data]

    def wait_for_extraction(
        self, task_id: str, poll_interval: float = 1.0, timeout: float = 300.0
    ) -> ExtractionTask:
        import time
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            task = self.get_extraction_task(task_id)
            if task.status in ("completed", "failed"):
                return task
            time.sleep(poll_interval)
        raise TimeoutError(f"Extraction task {task_id} did not complete within {timeout}s")

    # ── 11-15. Settings ─────────────────────────────────────────────

    def get_search_settings(self) -> SearchConfig:
        return SearchConfig.model_validate(self._request("GET", "/settings/graph/search"))

    def set_search_settings(self, config: dict) -> StatusResponse:
        return StatusResponse.model_validate(self._request("PUT", "/settings/graph/search", json=config))

    def get_llm_settings(self) -> dict:
        return self._request("GET", "/settings/llm")

    def set_llm_settings(self, providers: Optional[list[dict]] = None, default_model: Optional[str] = None) -> StatusResponse:
        body: dict = {}
        if providers is not None:
            body["providers"] = providers
        if default_model is not None:
            body["default_model"] = default_model
        return StatusResponse.model_validate(self._request("PUT", "/settings/llm", json=body))

    def get_rank_settings(self) -> RankConfig:
        return RankConfig.model_validate(self._request("GET", "/settings/graph/rank"))

    def set_rank_settings(self, config: dict) -> StatusResponse:
        return StatusResponse.model_validate(self._request("PUT", "/settings/graph/rank", json=config))

    def get_web_search_settings(self) -> WebSearchConfig:
        return WebSearchConfig.model_validate(self._request("GET", "/settings/web-search"))

    def set_web_search_settings(self, config: dict) -> StatusResponse:
        return StatusResponse.model_validate(self._request("PUT", "/settings/web-search", json=config))

    def web_search_proxy(self, query: str, provider_id: Optional[str] = None) -> str:
        body: dict = {"query": query}
        if provider_id:
            body["provider_id"] = provider_id
        data = self._request("POST", "/proxy/web-search", json=body)
        if data.get("success"):
            return data["data"]
        raise ApiError(502, data.get("error", "proxy search failed"))

    def get_tokenizer_words(self) -> TokenizerConfig:
        return TokenizerConfig.model_validate(self._request("GET", "/settings/tokenizer"))

    def add_tokenizer_words(self, words: list[str]) -> StatusResponse:
        return StatusResponse.model_validate(
            self._request("POST", "/settings/tokenizer/words", json={"words": words})
        )

    def remove_tokenizer_words(self, words: list[str]) -> StatusResponse:
        return StatusResponse.model_validate(
            self._request("DELETE", "/settings/tokenizer/words", json={"words": words})
        )

    # ── 16. MaaS ────────────────────────────────────────────────────

    def list_models(self) -> ModelListResponse:
        return ModelListResponse.model_validate(self._request("GET", "/proxy/openai/v1/models"))

    def chat_completion(
        self,
        messages: list[dict],
        model: Optional[str] = None,
        stream: bool = False,
        on_chunk: Optional[callable] = None,
    ) -> dict:
        body: dict = {"messages": messages, "stream": stream}
        if model:
            body["model"] = model
        if stream:
            return self._chat_completion_stream(body, on_chunk)
        return self._request("POST", "/proxy/openai/v1/chat/completions", json=body)

    def _chat_completion_stream(self, body: dict, on_chunk: Optional[callable] = None) -> dict:
        """Send a streaming chat completion request and accumulate SSE response.
        If on_chunk is provided, it is called with each content fragment as it arrives."""
        import json
        import httpx
        url = f"{self.base_url}/proxy/openai/v1/chat/completions"
        headers = dict(self._http.headers)
        headers["Accept"] = "text/event-stream"
        full_content = ""
        with httpx.Client(base_url=self.base_url, headers=headers, timeout=self._http.timeout) as client:
            with client.stream("POST", "/proxy/openai/v1/chat/completions", json=body) as resp:
                if resp.status_code == 404:
                    raise NotFoundError(body="not found")
                if not resp.is_success:
                    raise ApiError(resp.status_code, "error", "stream request failed")
                for line in resp.iter_lines():
                    if line.startswith("data: "):
                        data = line[6:]
                        if data == "[DONE]":
                            break
                        try:
                            chunk = json.loads(data)
                            delta = chunk.get("choices", [{}])[0].get("delta", {})
                            content = delta.get("content", "")
                            if content:
                                full_content += content
                                if on_chunk:
                                    on_chunk(content)
                        except json.JSONDecodeError:
                            pass
        return {"choices": [{"message": {"content": full_content}}]}
