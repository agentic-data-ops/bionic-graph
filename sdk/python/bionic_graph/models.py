from typing import Any, Optional
from pydantic import BaseModel


class HealthResponse(BaseModel):
    status: str
    version: str
    uptime_secs: int
    graphs: int
    cluster_enabled: bool


class GraphMetadata(BaseModel):
    name: str
    description: str = ""
    time_travel: bool = False


class GraphListResponse(BaseModel):
    default: str
    graphs: list[GraphMetadata]


class VertexResult(BaseModel):
    type: str = "vertex"
    id: int
    name: str = ""
    labels: list[str] = []
    keywords: list[str] = []
    properties: dict[str, Any] = {}
    score: Optional[float] = None
    rank: Optional[int] = None


class EdgeResult(BaseModel):
    type: str = "edge"
    id: int
    name: str = ""
    labels: list[str] = []
    keywords: list[str] = []
    source: int
    target: int
    strength: float = 1.0
    properties: dict[str, Any] = {}
    score: Optional[float] = None
    rank: Optional[int] = None


class GremlinResponse(BaseModel):
    success: bool
    data: list[VertexResult | EdgeResult | dict] = []
    error: Optional[str] = None


class IdResponse(BaseModel):
    id: int


class MetaResponse(BaseModel):
    id: int
    rank: int
    atime: str
    status: str


class Document(BaseModel):
    id: str
    title: str
    tags: list[str] = []
    created_at: str = ""
    updated_at: str = ""
    graph_name: str = ""


class DocumentListResponse(BaseModel):
    documents: list[Document] = []


class ExtractionTaskStep(BaseModel):
    icon: str = ""
    name: str = ""
    status: str = ""
    llmOutput: str = ""


class ExtractionTask(BaseModel):
    task_id: str
    status: str
    steps: list[ExtractionTaskStep] = []
    overall_pct: float = 0.0


class ExtractionSubmitResponse(BaseModel):
    task_id: str
    status: str


class SearchConfig(BaseModel):
    greedy: dict[str, Any] = {}
    exact: dict[str, Any] = {}


class RankConfig(BaseModel):
    auto_inc_rank_when_update: bool = True
    auto_inc_rank_when_read: bool = True
    auto_dec_rank_when_inactive: bool = True
    inactive_after_accessed_secs: int = 1_296_000
    inactive_rank_update_period: int = 86_400


class WebSearchProvider(BaseModel):
    id: str
    name: str
    search_url: str
    method: str = "GET"
    body_template: Optional[str] = None
    params: dict[str, str] = {}
    headers: dict[str, str] = {}


class WebSearchConfig(BaseModel):
    providers: list[WebSearchProvider] = []
    default_provider: str = ""


class TokenizerConfig(BaseModel):
    custom_words: list[str] = []


class LlmProvider(BaseModel):
    name: str
    api_base_url: str
    api_key: str = ""
    models: list[str] = []
    default_model: str = ""
    id: str = ""


class ModelListResponse(BaseModel):
    data: list[dict] = []
    defaultModel: str = ""


class StatusResponse(BaseModel):
    status: str
