# 集群转发/广播统一改造计划

## 1. 目标

将当前分散在各 handler 中的转发（`try_forward_json`/`try_forward_status`）和广播（`broadcast_request_to_workers`/`broadcast_write_result`/独立 HTTP 调用）逻辑抽取为统一的 `ClusterGateway` 网关，每个 handler 只声明一次请求，网关自动处理转发和广播。

## 2. 现状分析

### 2.1 当前模式（每个 handler 重复）

```
┌─────────────────────────────────────────────────────┐
│ handler(request)                                      │
│   1. 提取 graph_name / query_str / body_str          │
│   2. try_forward_json/status(...)  → 转发            │
│   3. 如果转发成功 → 返回                              │
│   4. 本地执行（handler 特有的 CRUD 逻辑）             │
│   5. broadcast_xxx(...)  → 广播                      │
│   6. 返回响应                                         │
└─────────────────────────────────────────────────────┘
```

### 2.2 涉及的模块和文件

| 文件 | handler 数量 | 转发函数 | 广播函数 |
|------|-------------|---------|---------|
| `src/gremlin/mod.rs` | 26 个 | `try_forward_json`/`try_forward_status` | `broadcast_request_to_workers` |
| `src/gremlin/settings.rs` | 4 个 | `try_forward_json` | `broadcast_request_to_workers` |
| `src/gremlin/indices.rs` | 6 个 | `try_forward_json` | `broadcast_request_to_workers` |
| `src/gremlin/tokenizer_settings.rs` | 2 个 | `try_forward_json` | 独立的 tokenizer-sync HTTP 调用 |

### 2.3 转发和广播的差异

| 维度 | 转发 | 广播 |
|------|------|------|
| 方向 | Worker → Master | Master → Workers |
| 方式 | HTTP POST `/cluster/forward` | HTTP POST `/cluster/execute` |
| 是否阻塞 | 阻塞等待 master 响应 | fire-and-forget |
| 请求体 | `ForwardedRequest { method, path, query, body, graph }` | 同 `ForwardedRequest` |
| 响应 | `ForwardedResponse { success, status_code, body }` | 忽略响应 |

### 2.4 问题

1. **重复提取**：每个 handler 都重复提取 `graph_name`、构造 `query_str`、序列化 `body_str`
2. **转发和广播可能不一致**：转发时用的 `body` 和广播时用的 `body` 可能不同（如 create_vertex 曾用不同的 body）
3. **新 handler 容易遗漏**：添加新 handler 时容易忘记转发或广播
4. **三种不同的转发返回类型**：`try_forward_json` 返回 JSON，`try_forward_status` 返回 StatusCode，`try_forward_read_json` 返回 JSON

## 3. 设计方案

### 3.1 ClusterRequest — 统一请求模型

一个请求只构造一次，同时用于转发和广播。

```rust
// src/cluster/mod.rs 或 src/cluster/request.rs

#[derive(Clone, Serialize, Deserialize)]
pub struct ClusterRequest {
    /// HTTP method (GET, POST, PUT, DELETE)
    pub method: String,
    /// Request path (e.g. "/vertices", "/vertices/1?force=true")
    pub path: String,
    /// Request body (JSON string)
    pub body: Option<String>,
    /// Graph name (None = default graph)
    pub graph: Option<String>,
}

impl ClusterRequest {
    pub fn new(method: &str, path: &str) -> Self { ... }
    pub fn with_body(mut self, body: &str) -> Self { ... }
    pub fn with_graph(mut self, graph: &str) -> Self { ... }
    pub fn with_query(mut self, key: &str, val: &str) -> Self { ... }
}
```

### 3.2 ClusterGateway — 统一网关

```rust
// src/cluster/gateway.rs

pub struct ClusterGateway {
    /// 是否为 worker 节点（决定是否转发）
    is_worker: bool,
    /// master 的集群地址（用于转发）
    master_addr: Option<String>,
    /// 集群注册表（用于广播）
    registry: Option<Arc<NodeRegistry>>,
}

impl ClusterGateway {
    pub fn from_state(state: &AppState) -> Self;

    /// 转发到 master。返回 master 的完整响应。
    /// - master 节点: 返回 Ok(None)（不转发，由 handler 本地执行）
    /// - worker 节点 + REPLAYING: 返回 Ok(None)（防止递归）
    /// - worker 节点 + 正常: 转发并等待响应
    pub async fn forward<R: DeserializeOwned>(
        &self, req: &ClusterRequest
    ) -> Result<Option<R>, StatusCode>;

    /// 广播到所有 worker（fire-and-forget）
    pub fn broadcast(&self, req: &ClusterRequest);
}
```

**为什么 `forward` 能同时取代 `try_forward_json` 和 `try_forward_status`？**

当前两个函数的区别仅在于返回值：

| 函数 | 返回类型 | 调用方 |
|------|---------|--------|
| `try_forward_json` | `Option<Result<Json<Value>, StatusCode>>` | 需要 master 返回 JSON 数据的 handler（create/update 等） |
| `try_forward_status` | `Option<StatusCode>` | 只需要状态码的 handler（delete 等） |

统一后 `forward` 使用泛型返回，两种场景都能覆盖：

```rust
// 场景 1：需要 JSON 响应（原 try_forward_json）
if let Some(resp) = cluster.forward::<CreateVertexResponse>(&req).await? {
    return Ok(Json(resp));
}

// 场景 2：只需要状态码（原 try_forward_status）
if cluster.forward::<serde_json::Value>(&req).await?.is_some() {
    return StatusCode::OK;
} else {
    // 本地执行
}
```

对于返回 JSON 的 handler，`forward::<T>()` 自动反序列化 master 的响应。
对于只返回状态码的 handler，`forward::<Value>()` 忽略 body 内容，仅判断 `is_some()`。

### 3.3 改造后的 handler 模式

```rust
// 改造前
pub async fn create_vertex(...) -> ... {
    let graph_name = headers.get("X-Graph-Name").and_then(|v| v.to_str().ok());
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    if let Some(resp) = try_forward_json(&state, "POST", "/vertices", None, graph_name, Some(&body_str)).await {
        return ...;
    }
    // ... 本地执行 ...
    broadcast_request_to_workers(&state.cluster_registry, "POST", "/vertices", ...);
}

// 改造后
pub async fn create_vertex(...) -> ... {
    let req = ClusterRequest::new("POST", "/vertices")
        .with_graph(graph_name)
        .with_body(&body);
    
    if let Some(resp) = cluster.forward::<CreateVertexResponse>(&req).await? {
        return Ok(Json(resp));
    }
    // ... 本地执行 ...
    cluster.broadcast(&req);
}
```

### 3.4 改动范围

| 文件 | 改动 |
|------|------|
| **新增** `src/cluster/request.rs` | `ClusterRequest` 结构体 + impl |
| **新增** `src/cluster/gateway.rs` | `ClusterGateway` 结构体 + forward/broadcast 方法 |
| **修改** `src/cluster/mod.rs` | 添加 `pub mod request; pub mod gateway;` |
| **修改** `src/gremlin/mod.rs` | 修改 26 个 handler，移除 `try_forward_*`/`broadcast_*` 调用，改为 `cluster.forward/broadcast` |
| **修改** `src/gremlin/settings.rs` | 修改 4 个 handler |
| **修改** `src/gremlin/indices.rs` | 修改 6 个 handler |
| **修改** `src/gremlin/tokenizer_settings.rs` | 修改 2 个 handler，tokenizer-sync 也通过统一的 broadcast 接口（内部判断是否用 `/cluster/tokenizer-sync`） |
| **删除** `src/gremlin/mod.rs` | `try_forward_json`, `try_forward_status`, `try_forward_read_json`, `broadcast_request_to_workers` 函数 |
| **保留** | `broadcast_write_result`（作为 WAL 复制的底层函数，被 gateway 内部调用或直接移除） |

### 3.5 `ClusterRequest` → `ForwardedRequest` / 广播请求 的映射

```
ClusterRequest
  ├─ .method + .path + .body + .graph
  │    └─ 直接映射到 ForwardedRequest { method, path, query, body, graph }
  │         ├─ 转发时 POST 到 /cluster/forward
  │         └─ 广播时 POST 到 /cluster/execute
  └─ tokenizer-sync 特殊处理
       └─ 如果 path 是 /settings/tokenizer/words，广播到 /cluster/tokenizer-sync
```

### 3.6 反向响应处理

当前 `try_forward_json` 返回 `Option<Result<Json<Value>, StatusCode>>`，各 handler 按需解析。

改造后 `cluster.forward::<T>()` 返回 `Result<Option<T>, StatusCode>`：
- `Ok(Some(data))` → 转发成功，直接返回数据
- `Ok(None)` → 本地执行（master 节点或 replay 中）
- `Err(status)` → 转发失败

### 3.7 tokenizer-sync 统一到 broadcast 接口

当前 `tokenizer_settings.rs` 中 `add_tokenizer_words` 和 `remove_tokenizer_words` 的广播使用独立的 HTTP 调用（直接 `reqwest::Client::new().post(...)` 到 `/cluster/tokenizer-sync`），不走统一的 `broadcast_request_to_workers` 通路。

改造后，`ClusterGateway::broadcast()` 内部自动识别 `/settings/tokenizer/words` 请求，发送到 `/cluster/tokenizer-sync` 而非 `/cluster/execute`：

```rust
// src/cluster/gateway.rs
impl ClusterGateway {
    pub fn broadcast(&self, req: &ClusterRequest) {
        ...
        let endpoint = if req.path.starts_with("/settings/tokenizer") {
            "/cluster/tokenizer-sync"
        } else {
            "/cluster/execute"
        };
        ...
    }
}
```

### 3.8 查询参数忠实传递

查询参数（`?force=true/false`、`?clean=true/false` 等）在转发和广播中必须**忠实于原始请求**传递，不做任何默认值推断。

`ClusterRequest` 的 `path` 字段包含完整路径和查询字符串：

```rust
// handler 中构造请求
let path = format!("/vertices/{}", id);
let req = ClusterRequest::new(method, &path)
    .with_query_str(query_str)  // "force=true" / "force=false" / None
    .with_graph(graph_name)
    .with_body(&body_str);
```

`with_query_str` 方法将查询参数拼接到 path 末尾：

```rust
impl ClusterRequest {
    pub fn with_query_str(mut self, qs: Option<&str>) -> Self {
        if let Some(q) = qs {
            self.path.push('?');
            self.path.push_str(q);
        }
        self
    }
}
```

转发和广播使用同一 path，保证查询参数一致：

| 原始请求 | ClusterRequest.path | 转发 path | 广播 path |
|---------|-------------------|-----------|----------|
| `DELETE /vertices/2` | `/vertices/2` | `/vertices/2` | `/vertices/2` |
| `DELETE /vertices/2?force=true` | `/vertices/2?force=true` | `/vertices/2?force=true` | `/vertices/2?force=true` |
| `DELETE /vertices/2?force=false` | `/vertices/2?force=false` | `/vertices/2?force=false` | `/vertices/2?force=false` |
| `DELETE /documents/x?clean=true` | `/documents/x?clean=true` | `/documents/x?clean=true` | `/documents/x?clean=true` |

所有 handler 的查询参数统一处理：

```rust
// delete_vertex / delete_edge
let query_str = params.force.map(|f| format!("force={}", f));
let req = ClusterRequest::new("DELETE", &format!("/vertices/{}", id))
    .with_query_str(query_str.as_deref())
    .with_graph(graph_name);

// delete_document
let query_str = params.clean.map(|c| format!("clean={}", c));
let req = ClusterRequest::new("DELETE", &format!("/documents/{}", id))
    .with_query_str(query_str.as_deref());
```

### 3.10 非转发场景处理

- **任务查询**：`get_task_handler` / `list_tasks_handler` 使用 `try_forward_read_json`（不检查 REPLAYING）
  → 改造后 `ClusterGateway` 需要提供 `forward_read` 方法，或通过参数控制是否检查 REPLAYING
- **删除操作**：`delete_vertex` / `delete_edge` 的 `?force=true` 参数在 path 中携带
  → `ClusterRequest::with_query()` 统一处理

### 3.11 ID 一致性保证（关键）

顶点、边、文档的**创建操作**在广播时，body 中必须携带 master 分配的唯一 ID，确保所有 workers 使用相同 ID 创建：

```rust
// 改造后的 create_vertex 广播示例
let vid = create_vertex_locked(&graph, &body.name, ...);  // master 分配 ID

// 广播 body = 原创建 body + "id": vid
let broadcast_body = ClusterRequest::new("POST", "/vertices")
    .with_graph(&graph_name)
    .with_body(&serde_json::json!({
        "name": body.name,
        "labels": body.labels,
        "keywords": body.keywords,
        "properties": body.properties,
        "id": vid,                        // ← master 的 ID
    }).to_string());

cluster.broadcast(&broadcast_body);
```

对应 handler 在 replay 模式下（`is_broadcast_replay() == true`）需要：
- **顶点创建**：body.id 有值时，使用该 ID 创建（跳过自动 alloc_vertex_id）
- **边创建**：body.id 有值时，使用该 ID 创建（跳过自动 alloc_edge_id）
- **文档创建**：body.id 有值时，使用该 ID 创建（`create_document` 已实现此逻辑）

```rust
// 在 create_vertex handler 中的 replay 分支
if crate::graph::graph::is_broadcast_replay() {
    if let Some(replica_id) = body.id {
        // 使用 master 分配的同 ID 创建
        create_vertex_with_id_locked(&graph, replica_id, &body.name, ...)?;
    } else {
        create_vertex_locked(&graph, &body.name, ...)?;
    }
}
```

当前状态：
- ✅ `CreateVertexBody` 已有 `id: Option<u32>` 字段
- ✅ `CreateEdgeBody` 已有 `id: Option<u32>` 字段
- ✅ `CreateDocumentBody` 已有 `id: Option<String>` 字段
- ❌ `crud::create_vertex` 尚不支持指定 ID 创建（需要新增 `create_vertex_with_id` 函数）
- ❌ `crud::create_edge` 尚不支持指定 ID 创建（需要新增 `create_edge_with_id` 函数）

### 3.12 ID 自增器和重复 ID 检查

创建 `create_vertex_with_id` / `create_edge_with_id` 函数时，需处理：

#### 顶点指定 ID 创建

```rust
pub fn create_vertex_with_id(
    graph: &Graph,
    vid: u32,
    name: &str,
    labels: &[String],
    keywords: &[String],
    properties: &HashMap<String, PropertyValue>,
) -> StorageResult<u32> {
    // 1. 检查是否已存在同名 vertex（避免重复写入）
    let existing = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.vertex_id.get(vid).copied()
    };
    if let Some(ptr) = existing {
        log::error!(
            "ID collision: vertex {} already exists at block={} chunk={}. \
             This should not happen during cluster replay.",
            vid, ptr.block_idx, ptr.chunk_offset
        );
        return Err(StorageError::Other(format!("vertex ID {} already exists", vid)));
    }

    // 2. 更新自增器，使其不会回头重复分配该 ID
    graph.ensure_vertex_id(vid);

    // 3. 创建 vertex 数据（与 create_vertex 相同，使用指定 vid）
    let payload = VertexPayload { ... };
    ...
}
```

#### 边指定 ID 创建

```rust
pub fn create_edge_with_id(
    graph: &Graph,
    eid: u32,
    source: u32,
    target: u32,
    name: &str,
    labels: &[String],
    keywords: &[String],
    strength: f32,
    properties: &HashMap<String, PropertyValue>,
) -> StorageResult<u32> {
    // 1. 检查边 ID 是否重复
    let existing = {
        let mi = graph.memory_index.read().unwrap_or_else(|e| e.into_inner());
        mi.edge_id.get(eid).copied()
    };
    if let Some(ptr) = existing {
        log::error!(
            "ID collision: edge {} already exists at block={} chunk={}. \
             This should not happen during cluster replay.",
            eid, ptr.block_idx, ptr.chunk_offset
        );
        return Err(StorageError::Other(format!("edge ID {} already exists", eid)));
    }

    // 2. 更新自增器
    graph.ensure_edge_id(eid);

    // 3. 创建 edge 数据（与 create_edge 相同，使用指定 eid）
    let payload = EdgePayload { ... };
    ...
}
```

#### 自增器同步方法

```rust
// graph.graph.rs 中新增
pub fn ensure_vertex_id(&self, vid: u32) {
    let max = self.next_vertex_id.fetch_max(vid + 1, Ordering::Relaxed);
    if max > vid {
        log::warn!(
            "vertex ID {} is behind allocator (max={}). \
             This may indicate out-of-order replay.", vid, max - 1
        );
    }
}

pub fn ensure_edge_id(&self, eid: u32) {
    let max = self.next_edge_id.fetch_max(eid + 1, Ordering::Relaxed);
    if max > eid {
        log::warn!(
            "edge ID {} is behind allocator (max={}). \
             This may indicate out-of-order replay.", eid, max - 1
        );
    }
}
```

## 4. 实施步骤

### 步骤 1：创建 ClusterRequest（1天）

```
- 新建 src/cluster/request.rs
- 定义 ClusterRequest + builder 方法
- 添加 from 到 ForwardedRequest 的转换
- 单元测试
```

### 步骤 2：创建 ClusterGateway（1天）

```
- 新建 src/cluster/gateway.rs
- forward() 方法：封装 try_forward_json 逻辑
- broadcast() 方法：封装 broadcast_request_to_workers + tokenizer-sync
- 处理 REPLAYING 标志
- 单元测试
```

### 步骤 3：逐个 handler 迁移（2-3天）

按优先级分组：

第一组（6 个，最常用）：
- `create_vertex` / `update_vertex` / `delete_vertex`
- `create_edge` / `update_edge` / `delete_edge`

第二组（7 个）：
- `handle_update_vertex_meta` / `handle_update_edge_meta`
- `create_graph` / `delete_graph` / `set_default_graph`
- `handle_batch_import` / `handle_batch_delete`

第三组（6 个）：
- `create_document` / `update_document` / `delete_document`
- `submit_extraction` / `extract_document_handler`
- `put_graph_config_handler`

第四组（6 个）：
- `update_search_settings` / `update_llm_settings`
- `update_web_search_settings` / `update_rank_settings`
- `add_tokenizer_words` / `remove_tokenizer_words`

第五组（5 个）：
- `create_vertex_property_index` / `delete_vertex_property_index` / ...
- `get_task_handler` / `list_tasks_handler`（仅 forward_read）

### 步骤 4：清理和验证（1天）

```
- 删除旧的 try_forward_json / try_forward_status / try_forward_read_json / broadcast_request_to_workers
- 全量编译
- 集群覆盖测试（REASONIX.md 中的 32 个测试用例）
```

### 步骤 5：文档更新（0.5天）

```
- 更新 src/cluster/ 的模块注释
- 更新 REASONIX.md
```

## 5. 风险和缓解

| 风险 | 缓解 |
|------|------|
| 改造范围大，回归风险高 | 分组逐个 handler 迁移，每组合并一次 |
| tokenizer-sync 广播机制特殊 | gateway.broadcast 内部判断 path，特殊处理 |
| `try_forward_read_json` 跳过 REPLAYING 检查 | gateway.forward_read() 作为一个独立方法 |
| 请求 body 在 handler 中可能已被 consume/修改 | ClusterRequest 的 body 在 handler 本地执行前构造 |

## 6. 总工作量估算

| 阶段 | 工作日 | 代码变更 |
|------|--------|---------|
| ClusterRequest 设计 | 1 | +80 行 |
| ClusterGateway 设计 | 1 | +120 行 |
| 第一组 handler 迁移 | 1 | -60 行 |
| 第二组 handler 迁移 | 0.5 | -50 行 |
| 第三组 handler 迁移 | 0.5 | -50 行 |
| 第四组 handler 迁移 | 0.5 | -60 行 |
| 第五组 handler 迁移 | 0.5 | -40 行 |
| 清理 + 验证 | 1 | -200 行 |
| **总计** | **6 天** | **+200 / -460 = 净减约 260 行** |
