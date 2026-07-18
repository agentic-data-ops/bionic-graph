# Python SDK for Bionic-Graph

## 概述

构建一个 Python SDK，封装 bionic-graph 后端的所有 REST API，提供类型安全、使用便捷的客户端接口。

## 命名

- 包名: `bionic-graph-sdk`（发布到 PyPI）
- 顶级模块: `bionic_graph`

## 项目结构

```
sdk/python/
├── pyproject.toml          # 构建配置（hatchling / setuptools）
├── README.md               # 使用说明
├── bionic_graph/
│   ├── __init__.py         # 导出 Client 类 + 类型
│   ├── client.py           # 主 Client 类（HTTP 会话、认证、基础 CRUD）
│   ├── cli.py              # CLI 入口（click 子命令）
│   ├── models.py           # 数据类型（Pydantic）
│   ├── gremlin.py          # Gremlin 查询构建器
│   ├── documents.py        # 文档管理模块
│   ├── extraction.py       # 提取任务模块
│   ├── settings.py         # 设置管理模块
│   ├── maas.py             # MaaS 代理模块
│   └── exceptions.py       # 异常类
└── tests/
    ├── conftest.py         # 测试夹具（mock server）
    ├── test_client.py
    ├── test_gremlin.py
    └── test_documents.py
```

## 阶段划分

### Phase 1: 基础设施 + 核心 API（健康检查 + 图管理 + 顶点/边 CRUD）

**Step 1: 项目脚手架**
- 创建 `pyproject.toml`，依赖: `httpx`, `pydantic`
- `bionic_graph/__init__.py` 导出 `Client`
- `bionic_graph/exceptions.py`: `BionicGraphError`, `NotFoundError`, `ApiError`

**Step 2: HTTP 客户端基类**
- `Client` 类封装 `httpx.AsyncClient` / `httpx.Client`
- 支持 `base_url` 配置，可选 `api_key`
- 统一错误处理：将 HTTP 4xx/5xx 转为异常

**Step 3: 健康检查**
- `GET /health` → `client.health()` 返回 `HealthResponse`

**Step 4: 图生命周期管理**
- `GET /graphs` — 列出图
- `POST /graphs` — 创建图（name, description, time_travel）
- `PUT /graphs` — 设置默认图
- `DELETE /graphs/:name` — 删除图
- `PUT /graphs/:name` — 更新图元数据
- `GET /graphs/:name/config` — 获取图配置
- `PUT /graphs/:name/config` — 更新图配置

**Step 5: 顶点 CRUD**
- `POST /vertices` — 创建顶点（name, labels, keywords, properties）
- `PUT /vertices/:id` — 更新顶点
- `DELETE /vertices/:id?force=true` — 删除顶点
- `GET /vertices/:id/meta` — 获取顶点元数据
- `PUT /vertices/:id/meta` — 更新顶点元数据

**Step 6: 边 CRUD**
- `POST /edges` — 创建边（source, target, name, labels, keywords, strength, properties）
- `PUT /edges/:id` — 更新边
- `DELETE /edges/:id?force=true` — 删除边
- `GET /edges/:id/meta` — 获取边元数据
- `PUT /edges/:id/meta` — 更新边元数据

### Phase 2: Gremlin 查询 + 搜索

**Step 7: Gremlin 查询模块**
- `POST /gremlin` — 提交 Gremlin 查询
- 提供 Pythonic 的 Gremlin 查询构建器（链式调用）
- 支持全部 25 个步骤：search, V, E, has, hasNot, hasKey, hasValue, hasLabel, hasText, out, in, both, outE, inE, bothE, values, limit, count, dedup, repeat, timeTravel, compact, expand, traverse, rank

**Step 8: 搜索快捷方式**
- `GET /search?text=xxx&mode=greedy&limit=20`

### Phase 3: 文档管理

**Step 9: 文档 CRUD**
- GET/POST/PUT/DELETE `/documents`
- GET `/documents/:id/content`

### Phase 4: 知识提取

**Step 10: 提取任务**
- POST `/extract`, POST `/documents/:id/extract`
- GET `/extract/task/:task_id`, GET `/extract/tasks`
- 支持异步任务轮询：`client.wait_for_extraction()`

### Phase 5: 设置管理

**Step 11-15: 全部设置端点**
- 搜索设置: GET/PUT `/settings/graph/search`
- LLM 设置: GET/PUT `/settings/llm`
- Rank 设置: GET/PUT `/settings/graph/rank`
- Web Search: GET/PUT `/settings/web-search`, POST `/web-search/proxy`
- Tokenizer: GET `/settings/tokenizer`, POST/DELETE `/settings/tokenizer/words`

### Phase 6: MaaS 代理

**Step 16: OpenAI 兼容代理**
- GET `/maas/openai/v1/models` — 列出模型
- POST `/maas/openai/v1/chat/completions` — 聊天补全（支持 SSE 流式）

### Phase 7: CLI 入口

**Step 17: CLI 脚手架**
- 在 `pyproject.toml` 中添加 `[project.scripts]` entry point: `bionic-graph = bionic_graph.cli:main`
- 依赖 `click>=8.0`（或使用标准库 `argparse`）
- 文件 `bionic_graph/cli.py` — 主入口，解析参数并分发

**Step 18: 位置参数 + 命名参数设计**

```
用法:
  bionic-graph <topic> <action> [OPTIONS]

主题 (topic) 和 动作 (action):
  health    check                 健康检查
  
  graph     list                  列出图
            create                创建图 (--name, --description, --time-travel)
            set-default           设置默认图 (--name)
            delete                删除图 (--name, --force)
            update-meta           更新元数据 (--name, --description, --time-travel)
            get-config            获取图配置 (--name)
            set-config            设置图配置 (--name, --config)

  vertex    create                创建顶点 (--name, --labels, --keywords, --properties, --graph)
            update                更新顶点 (--id, --name, --labels, --keywords, --properties, --graph)
            delete                删除顶点 (--id, --force, --graph)
            get-meta              获取顶点元数据 (--id, --graph)
            update-meta           更新顶点元数据 (--id, --rank, --graph)

  edge      create                创建边 (--source, --target, --name, --labels, --strength, --properties, --graph)
            update                更新边 (--id, ...)
            delete                删除边 (--id, --force, --graph)
            get-meta              获取边元数据 (--id, --graph)
            update-meta           更新边元数据 (--id, --rank, --graph)

  gremlin   execute               执行 Gremlin 查询 (--steps JSON, --graph)
            search                搜索 (--text, --mode, --limit, --graph)

  document  list                  列出文档 (--graph)
            create                创建文档 (--title, --content, --tags, --graph)
            get                   获取文档 (--id)
            update                更新文档 (--id, --title, --tags)
            delete                删除文档 (--id)
            get-content           获取文档内容 (--id)

  extract   submit                提交提取任务 (--document-id, --graph, --model)
            get-task              获取任务状态 (--task-id)
            list-tasks            列出所有任务
            wait                  等待任务完成 (--task-id, --poll-interval, --timeout)

  settings  get-search            获取搜索配置
            set-search            设置搜索配置 (--config JSON)
            get-llm               获取 LLM 配置
            set-llm               设置 LLM 配置 (--providers JSON, --default-model)
            get-rank              获取排序配置
            set-rank              设置排序配置 (--config JSON)
            get-web-search        获取联网搜索配置
            set-web-search        设置联网搜索配置 (--config JSON)
            proxy                 执行联网搜索 (--query, --provider-id)
            get-tokenizer         获取分词器配置
            add-tokenizer-words   添加分词词 (--words JSON)
            remove-tokenizer-words 移除分词词 (--words JSON)

  maas      list-models           列出模型
            chat                  聊天补全 (--messages JSON, --model, --stream)

  chat       interactive          交互式聊天 (默认动作)
```

**Step 19: 全局选项**

```
选项:
  --base-url TEXT    后端地址 (默认 http://127.0.0.1:8080)
  --api-key TEXT     API 密钥 (可选)
  --timeout FLOAT    请求超时秒数 (默认 30.0)
  --help             显示帮助
```

**Step 20: 交互式聊天 (chat 主题)**

`bionic-graph chat` 进入交互式 REPL 模式，实现类似前端的聊天体验：

```
$ bionic-graph chat
Chat session started. Type /help for commands, /exit to quit.
────────────────────────────────────────────────────────
You > 介绍下电影《功夫女足》
🤖 周星驰执导的《功夫女足》是一部...
────────────────────────────────────────────────────────
You > 演员都有谁？
```

命令行参数控制聊天行为：

```
bionic-graph chat --web-search --graph-search --extract-keywords --graph graph0 --search-mode greedy
```

| 参数 | 默认 | 说明 |
|------|------|------|
| `--web-search` / `--no-web-search` | `--web-search` | 启用联网搜索 |
| `--graph-search` / `--no-graph-search` | `--graph-search` | 启用图谱搜索 |
| `--extract-keywords` / `--no-extract-keywords` | `--extract-keywords` | 启用 LLM 关键词提取 |
| `--graph TEXT` | `graph0` | 指定搜索图谱 |
| `--search-mode TEXT` | `greedy` | 搜索模式 (greedy/exact) |
| `--model TEXT` | `DeepSeek/deepseek-v4-flash` | 聊天模型 |

实现要点:
- 使用 `click.prompt()` 或标准 `input()` 读取用户输入
- 维护对话历史列表，调用 MaaS `/v1/chat/completions`
- 联网搜索：调用 `client.web_search_proxy()`，搜索结果拼接为 system message
- 图谱搜索：调用 `client.search()`，结果拼接为 system message
- 关键词提取：在搜索前调用 LLM 提取关键词
- 支持 `/help`、`/exit`、`/clear`、`/graph <name>` 等内部命令

**Step 21: CLI 输出格式**

- 默认：彩色可读输出（支持 JSON/YAML 选项）
- `--output json` — 输出原始 JSON，便于管道处理
- 错误时非零退出码

## 项目结构（更新后）

```
sdk/python/
├── pyproject.toml
├── README.md
├── bionic_graph/
│   ├── __init__.py
│   ├── client.py
│   ├── models.py
│   ├── cli.py               # 新增: CLI 入口 + 所有子命令
│   ├── gremlin.py
│   ├── documents.py
│   ├── extraction.py
│   ├── settings.py
│   ├── maas.py
│   └── exceptions.py
└── tests/
    ├── test_client.py
    ├── test_cli.py           # 新增: CLI 测试
    └── ...
```

## 数据模型（models.py）

主要 dataclass:

- `PropertyValue` — 属性值（String/Integer/Float/Boolean/List/Null）
- `VertexResult` — 顶点查询结果（id, name, labels, keywords, properties, score, rank）
- `EdgeResult` — 边查询结果（id, name, labels, keywords, source, target, strength, properties, score, rank）
- `GremlinResponse` — Gremlin 响应（success, data, error）
- `GraphMetadata` — 图元数据（name, description, time_travel）
- `Document` — 文档元数据（id, title, tags, created_at, updated_at, graph_name）
- `HealthResponse` — 健康检查响应
- `ExtractionTask` — 提取任务（task_id, status, steps, overall_pct）

## Gremlin 查询构建器（gremlin.py）

链式调用接口，覆盖全部 25 个 step：

```python
query = GremlinQuery()
query.search("机器学习", mode="greedy").traverse(decay=0.9, max_depth=3).rank(limit=10)
results = query.execute(client, graph="graph0")

# 快捷方式
results = client.gremlin().search("AI").both(labels=["related_to"]).limit(20).execute()
```

## Client API 总览

```python
class Client:
    def __init__(self, base_url="http://127.0.0.1:8080", api_key=None, timeout=30.0)

    # 健康检查
    def health() -> HealthResponse

    # 图管理
    def list_graphs(), create_graph(), set_default_graph(), delete_graph()
    def update_graph_meta(), get_graph_config(), set_graph_config()

    # 顶点
    def create_vertex(), update_vertex(), delete_vertex()
    def get_vertex_meta(), update_vertex_meta()

    # 边
    def create_edge(), update_edge(), delete_edge()
    def get_edge_meta(), update_edge_meta()

    # Gremlin
    def gremlin() -> GremlinQuery
    def execute_gremlin(steps, graph=None)
    def search(text, mode="greedy", ...)

    # 文档
    def list_documents(), create_document(), get_document()
    def update_document(), delete_document(), get_document_content()

    # 提取
    def submit_extraction(), extract_document()
    def get_extraction_task(), list_extraction_tasks()
    def wait_for_extraction(task_id, poll_interval=1.0, timeout=300.0)

    # 设置
    def get_search_settings(), set_search_settings()
    def get_llm_settings(), set_llm_settings()
    def get_rank_settings(), set_rank_settings()
    def get_web_search_settings(), set_web_search_settings()
    def web_search_proxy(query, provider_id=None)
    def get_tokenizer_words(), add_tokenizer_words(), remove_tokenizer_words()

    # MaaS
    def list_models(), chat_completion(messages, model=None, stream=False)
```

## 测试

- 使用 pytest + respx（httpx mock）模拟后端
- 每个端点至少一个测试用例
- docker-compose 启动真实后端做集成测试

## 发布

```
pip install build twine
python -m build
twine upload dist/*
```
