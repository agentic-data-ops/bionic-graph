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

一个请求只构造一次，同时用于转发和广播，包含完整的 HTTP 请求信息：method、path、headers、query、body。

```rust
// src/cluster/mod.rs 或 src/cluster/request.rs

/// 统一的集群请求模型，包含原始 HTTP 请求的完整信息。
#[derive(Clone, Serialize, Deserialize)]
pub struct ClusterRequest {
    /// HTTP method (GET, POST, PUT, DELETE)
    pub method: String,
    /// Request path + query string (e.g. "/vertices/1?force=true")
    pub path: String,
    /// 原始请求 headers（如 X-Graph-Name、X-Time-Travel 等）
    pub headers: HashMap<String, String>,
    /// Request body (JSON string)
    pub body: Option<String>,
}
```

`ForwardedRequest` 结构体同步简化，移除冗余的 `graph` 字段，该信息已由 `headers["X-Graph-Name"]` 携带：

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForwardedRequest {
    pub method: String,
    pub path: String,                       // 含查询参数
    pub body: Option<String>,
    pub headers: HashMap<String, String>,   // 含 X-Graph-Name、X-Time-Travel 等
}
```

Builder 方法——不再需要 `with_graph`，直接通过 header 指定：

```rust
impl ClusterRequest {
    pub fn new(method: &str, path: &str) -> Self { ... }

    /// 设置请求体
    pub fn with_body(mut self, body: &str) -> Self { ... }

    /// 设置 X-Graph-Name header
    pub fn with_graph(mut self, graph: &str) -> Self {
        self.headers.insert("X-Graph-Name".to_string(), graph.to_string());
        self
    }

    /// 设置 X-Time-Travel header
    pub fn with_time_travel(mut self, tt: u64) -> Self {
        self.headers.insert("X-Time-Travel".to_string(), tt.to_string());
        self
    }

    /// 添加任意 header
    pub fn with_header(mut self, key: &str, val: &str) -> Self { ... }

    /// 设置查询字符串，追加到 path 末尾
    pub fn with_query_str(mut self, qs: Option<&str>) -> Self { ... }
}
```

**handler 中构造请求示例**：

```rust
// create_vertex: 转发 body + graph name
let req = ClusterRequest::new("POST", "/vertices")
    .with_graph(graph_name)              // → X-Graph-Name header
    .with_body(&serde_json::to_string(&body)?);

// delete_vertex: 转发 query param + graph name
let query_str = params.force.map(|f| format!("force={}", f));
let req = ClusterRequest::new("DELETE", &format!("/vertices/{}", id))
    .with_graph(graph_name)
    .with_query_str(query_str.as_deref());

// search with time travel: 转发 time travel header
let req = ClusterRequest::new("POST", "/gremlin")
    .with_graph(graph_name)
    .with_time_travel(tt_micros)
    .with_body(&serde_json::to_string(&query)?);
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

    /// 转发到 master（写操作）。检查 REPLAYING 标志，replay 中不转发。
    /// 返回 master 的完整响应。
    /// - master 节点: 返回 Ok(None)（不转发，由 handler 本地执行）
    /// - worker 节点 + REPLAYING: 返回 Ok(None)（防止递归）
    /// - worker 节点 + 正常: 转发并等待响应
    pub async fn forward<R: DeserializeOwned>(
        &self, req: &ClusterRequest
    ) -> Result<Option<R>, StatusCode>;

    /// 转发到 master（读操作）。**跳过** REPLAYING 检查。
    /// 用于任务查询等只读请求，即使正在写 replay 也能正常转发。
    pub async fn forward_read<R: DeserializeOwned>(
        &self, req: &ClusterRequest
    ) -> Result<Option<R>, StatusCode>;

    /// 广播到所有 worker（fire-and-forget）
    pub fn broadcast(&self, req: &ClusterRequest);
}
```

**`forward` vs `forward_read` 行为对比**：

| 场景 | `forward`（写） | `forward_read`（读） |
|------|----------------|---------------------|
| master 节点 | 不转发 → 本地执行 | 不转发 → 本地执行 |
| worker + REPLAYING | **不转发** → 跳过（防递归） | **转发** → 正常查询 master |
| worker + 正常 | 转发 | 转发 |

**handler 使用示例**：

```rust
// 写操作：create_vertex（使用 forward）
let req = ClusterRequest::new("POST", "/vertices").with_graph(graph_name).with_body(&body_str);
if let Some(resp) = cluster.forward::<CreateVertexResponse>(&req).await? {
    return Ok(Json(resp));
}

// 读操作：get_task（使用 forward_read）
let req = ClusterRequest::new("GET", &format!("/tasks/{}", task_id));
if let Some(resp) = cluster.forward_read::<TaskResponse>(&req).await? {
    return Ok(Json(resp));
}
```
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

## 5. 审计发现 — 计划遗漏和风险点

### 5.1 遗漏的 handler

| Handler | 转发 | 广播 | 说明 |
|---------|------|------|------|
| `update_graph_meta` (`PUT /graphs/:name`) | ✅ `try_forward_json` | ✅ `broadcast_request_to_workers` | 修改图库描述/time_travel，列入 Group 2 |

### 5.2 计划未覆盖的风险点

#### 风险 1：并发广播的 REPLAYING 标志干扰

当前 `IS_BROADCAST_REPLAY` 通过 `tokio::task_local!` 实现（`mod.rs:486`），每个任务独立。但多个写入操作同时广播时，worker 的 `handle_execute` 会并发处理多个 replay 请求。Gateway 的 `broadcast()` 方法内部仍使用 `is_broadcast_replay()` 检查，**不会互相干扰**（task-local 隔离）。但需确保 `forward()` 方法在 REPLAYING 检查上也是基于 task-local。

#### 风险 2：广播失败无重试（at-most-once）

当前广播是 fire-and-forget：`tokio::spawn` 后日志即丢弃。改造后的 `ClusterGateway::broadcast()` 继续保持此语义（与现有行为一致）。如需 at-least-once 保证，需在计划范围外引入持久化队列+重试机制。

#### 风险 3：tokenizer 双重广播

`handle_forward`（`server.rs:108-140`）在收到 tokenizer 的转发请求时，会再次广播 `/cluster/tokenizer-sync`。而 `add_tokenizer_words`/`remove_tokenizer_words`（Master 本地执行后）也会广播。改造后由 `ClusterGateway::broadcast()` 统一处理，**仅广播一次**（Master 本地执行后 → broadcast）。`handle_forward` 中的 tokenizer 分支应移除。

#### 风险 4：`X-Request-Id` 的传递

当前通过 `proxy_to_api`（`server.rs:314-318`）设置此 header，用于 middleware 识别重放。改造后的 `ClusterGateway::forward()` 必须在 `ForwardedRequest` 中携带此 header，或由 `proxy_to_api` 内部自动注入（推荐：`proxy_to_api` 始终注入 `X-Request-Id`，gateway 不关心此细节）。

#### 风险 5：`proxy_to_api` 需遍历所有 headers

当前 `proxy_to_api`（`server.rs:306-318`）仅设置 `X-Graph-Name` 和 `X-Request-Id`。改造后 `ForwardedRequest.headers` 包含了所有原始请求 headers，`proxy_to_api` 必须改为**遍历 `headers` 逐项设置**：

```rust
for (k, v) in &req.headers {
    if k.eq_ignore_ascii_case("host") || k.eq_ignore_ascii_case("content-length") { continue; }
    request = request.header(k.as_str(), v.as_str());
}
```

#### 风险 6：`submit_extraction` 后台任务中的广播

`submit_extraction` 的后台任务（`mod.rs:2039-2042`, `2056-2059`）在 `tokio::spawn` 的闭包中调用 `broadcast_request_to_workers`。改造后需确保 `ClusterGateway` 可以跨线程安全调用（`Send + Sync`）。

### 5.3 死代码

`broadcast_write_result`（`mod.rs:254`）已无任何调用点，可安全删除。

## 6. 集群可靠性增强

### 6.1 Master 节点持久化与启动等待

**文件**：`<data_dir>/cluster/nodes.json`

Master 在每次 worker 注册/心跳时，将可用节点信息持久化到 `cluster/nodes.json`：

```json
{
  "master": {
    "node_id": "master@127.0.0.1:9090",
    "api_addr": "127.0.0.1:8080",
    "cluster_addr": "127.0.0.1:9090",
    "last_seen": 1785411000000000
  },
  "workers": [
    {
      "node_id": "worker@127.0.0.1:9091",
      "api_addr": "127.0.0.1:8081",
      "cluster_addr": "127.0.0.1:9091",
      "last_seen": 1785411000000000,
      "status": "alive"
    }
  ],
  "version": 1
}
```

**启动流程**：

```
master 启动 → 读取 cluster/nodes.json
  ├─ 等待所有已知 worker 通过心跳注册（超时 N 秒）
  │    └─ 超时后，已注册 worker ≥ 1 → 继续启动
  │    └─ 超时后，尚无任何 worker → 继续启动（允许单节点降级）
  ├─ 开始接受 API 请求
  │    └─ 未就绪的 worker 请求 → 503 Service Unavailable
  └─ 定期持久化当前节点状态
```

**API 可用性检查**：Master 在所有已知 worker 未全部注册前，对写请求返回 `503 Service Unavailable`，读请求正常处理。

### 6.2 Worker 首次连接时图库配置同步

Worker 连接到 Master 时，在心跳消息中携带本地图库列表。Master 对比后返回差异，Worker 同步缺失/不一致的图库。

**心跳扩展**：

```rust
pub struct Heartbeat {
    pub node_id: String,
    pub api_addr: String,
    pub cluster_addr: String,
    pub last_acked_seq: u64,
    pub graphs: Vec<GraphMeta>,  // Worker 的本地图库列表
}
```

**同步流程**：

```
Worker 首次心跳
  ├─ Worker: 发送 Heartbeat { graphs: [graph0: {time_travel:true}, ...] }
  ├─ Master: 对比 master 的图库列表与 worker 的列表
  │    ├─ 缺失图库 → 返回 CreateGraph 指令
  │    └─ 配置不一致 → 返回 UpdateGraphConfig 指令
  └─ Worker: 执行指令，创建/更新本地图库
       └─ 完成后，发送确认心跳
```

图库配置同步后，Master 才开始向该 Worker 广播写入请求。

### 6.3 基于持久化 FIFO 队列的广播机制（重构）

**文件**：`<data_dir>/cluster/broadcast-<node>-<timestamp>.bin`

将广播从 fire-and-forget 重构为 **持久化 FIFO 消息队列 + 异步消费线程**，保证 at-least-once 投递：

**队列文件格式**（bincode 或 JSON 序列化）：

```rust
#[derive(Serialize, Deserialize)]
pub struct QueuedBroadcast {
    pub req: ForwardedRequest,
    pub target_node: String,   // worker 的 node_id（如 "worker@127.0.0.1:9091"）
    pub created_at: u64,
}
```

**滚动策略**：
- 每个队列文件最多记录 **1000 个请求**，写满后滚动创建新文件 `broadcast-<node>-<timestamp>.bin`（时间戳保证文件名唯一）
- 文件名中的 `<node>` 区分不同目标节点，每个 worker 有独立的队列
- 处理完一个队列文件（全部成功投递）后执行 **删除**

**异步消费流程**：

```
broadcast() 调用（Master handler）
  └─ 将 QueuedBroadcast 追加到 <node> 的当前 FIFO 队列文件（同步写盘）

异步消费线程（每个 worker 一个，或统一调度）
  ├─ 扫描 broadcast-<node>-*.bin（按时间戳排序）
  ├─ 顺序读取队首请求 → POST /cluster/execute → 目标 worker
  │    ├─ 成功 → 从队列移除，继续下一条
  │    └─ 失败（网络错误）→ 保留在队首，持续重试，直到该节点成功
  │         └─ 重试间隔可退避（如 1s → 2s → 4s → ... 上限 30s）
  └─ 队列文件全部投递成功 → 删除该文件 → 处理下一个文件
```

**Master 启动就绪检查**：

```
master 启动 → 读取 cluster/nodes.json（已知节点）
  ├─ 等待所有已知 worker 通过心跳注册（超时 N 秒）
  │    └─ 超时后，已注册 worker ≥ 1 → 继续
  │    └─ 超时后，尚无任何 worker → 允许单节点降级继续
  ├─ 扫描并执行所有遗留 broadcast-<node>-*.bin 队列（重启前未投递的）
  │    └─ 队列全部执行完毕后，才开始对外服务
  └─ 开始接受 API 请求
       └─ 未就绪的 worker 广播请求 → 先入队，由消费线程投递
```

**失败语义**：
- 与旧方案的 `retry_count > 10 放弃` 不同，新机制 **无限重试**，直到节点成功——因为队列文件持久化在磁盘，进程重启也不丢失
- 网络恢复后，消费线程自动继续投递队首请求
- worker 永久离线时，队列持续堆积（符合 at-least-once 语义）；可通过运维人工清理

### 6.4 实施优先级

| 功能 | 优先级 | 工作量 | 说明 |
|------|--------|--------|------|
| 6.1 节点持久化 | P0 | 1天 | 启动就绪检查，影响集群可用性 |
| 6.2 图库同步 | P1 | 1天 | 数据一致性保障 |
| 6.3 持久化 FIFO 队列广播 | P2 | 2-3天 | 数据可靠性保障：重构广播为持久化队列 + 异步消费线程 + 启动就绪检查 |

## 7. 总工作量估算
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
