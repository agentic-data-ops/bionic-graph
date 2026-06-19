# Bionic-Graph 架构设计文档

> 归档所有已完成和待完成的计划

---

## Plan 1: 初始架构 (7 步迭代)

**状态: ✅ 已完成**

### 概述

用 Rust 从零实现一个知识图谱 + 生物神经元扩散激活索引层 + Gremlin 兼容接口。

### 架构

```
┌──────────────────────────────────────────────┐
│            Gremlin 接口层（REST API）           │
│  V()/E()/has()/out()/in()/both()/limit()/     │
│  neuralSearch(keywords...)                     │
├──────────────────────────────────────────────┤
│          Neural Index 层（扩散激活网络）         │
│  关键词 → 神经元激活 → 激活扩散 → 顶点发现      │
│  Hebbian 学习 | 自动持久化                       │
├──────────────────────────────────────────────┤
│          Knowledge Graph 层（邻接表）            │
│  Vertex / Edge / Property                      │
│  BFS / DFS 遍历引擎                             │
└──────────────────────────────────────────────┘
```

### 神经元模型

不是权重矩阵，而是扩散激活网络：

| 概念 | 生物对应 | 作用 |
|------|----------|------|
| 激活值 | 膜电位 | 兴奋程度 0.0~1.0 |
| 阈值 | 动作电位阈值 | 超过则点火 |
| 不应期 | 不应期 | 点火后休息 N tick |
| 衰减率 | 漏电流 | 每 tick 激活值衰减 |
| 突触强度 | 突触权重 | 传递激活比例 |
| 可塑性 | Hebbian 学习率 | 共激活增强连接 |

### 步骤

#### Step 1: 脚手架 + Graph 核心数据结构 ✅
- Cargo 项目结构
- Vertex/Edge/PropertyValue 类型
- 双向邻接表 Graph
- 标签索引
- 单元测试

**文件**: `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/graph/vertex.rs`, `src/graph/edge.rs`, `src/graph/graph.rs`, `src/graph/mod.rs`

#### Step 2: 图遍历引擎 (BFS/DFS) ✅
- BFS/DFS 惰性迭代器
- 深度限制、标签过滤、多起点
- TraversalStep

**文件**: `src/graph/traversal.rs`

#### Step 3: 生物神经元网络 (扩散激活) ✅
- Neuron/Synapse 结构
- 网络编排 (tick 循环)
- 扩散激活算法
- Hebbian 学习 + FiringHistory
- 关键词触发

**文件**: `src/neuron/neuron.rs`, `src/neuron/network.rs`, `src/neuron/activation.rs`, `src/neuron/learning.rs`, `src/neuron/mod.rs`

#### Step 4: 持久化层 ✅
- bincode 序列化/反序列化
- graph_store / neuron_store
- auto_save 后台线程
- load_or_create

**文件**: `src/persistence/mod.rs`, `src/persistence/graph_store.rs`, `src/persistence/neuron_store.rs`, `src/persistence/auto_save.rs`

#### Step 5: Gremlin 最小子集接口 ✅
- JSON 管道查询 (V/E/has/hasLabel/out/in/both/values/limit/count/dedup)
- 自定义 neuralSearch 步骤
- REST API (axum)

**文件**: `src/gremlin/query.rs`, `src/gremlin/steps.rs`, `src/gremlin/server.rs`, `src/gremlin/mod.rs`

#### Step 6: MemorySystem API + main.rs ✅
- 统一三层的顶层 MemorySystem
- CLI 入口 (clap)
- HTTP 服务器启动

**文件**: `src/memory_system.rs`, `src/main.rs`

#### Step 7: 演示 + 文档 ✅
- examples/demo.rs 完整演示
- README.md 文档

**文件**: `examples/demo.rs`, `README.md`

---

## Plan 2: 磁盘存储补充架构 (4 步)

**状态: ✅ 已完成**

### 概述

将知识图谱从全量内存改为磁盘存储：子图分区 + 按需加载 + LRU 缓存 + Redo Log (WAL)。

### 数据目录结构

```
data/
├── subgraph/
│   ├── 00000001.bin       ← 子图数据块
│   └── ...
├── index.bundle           ← VertexIndex + SubgraphIndex + LabelIndex
├── redo.log               ← WAL (追加写)
└── neural.bin             ← 神经网络（独立持久化）
```

### 子图分区

- 默认按 BFS 聚类，深度 3
- 支持 AutoCluster / ByLabel / SingleSubgraph 策略
- 单块上限 64MB
- 跨子图边存为 CrossEdgeRef（含 target_subgraph + target_vertex）

### Redo Log (WAL)

```
写入: mutate → 1. log.append(op) [fsync] → 2. cache.apply(op)
Checkpoint: flush dirty subs → write CHECKPOINT marker → rotate log
恢复: scan log → find last CHECKPOINT → replay after it
```

日志格式: `[type(u8)][seq(u64)][data_len(u32)][data...][CRC32(u32)]`

### 步骤

#### Step 1: Subgraph 格式 + Index 数据结构 ✅
- Subgraph struct (vertices/edges/cross_edges)
- 序列化格式: Magic + Version + CRC32 + bincode
- VertexIndex / SubgraphIndex / LabelIndex / IndexBundle

**文件**: `src/storage/subgraph.rs`, `src/storage/index.rs`, `src/storage/mod.rs`

#### Step 2: SubgraphCache (LRU + 按需加载) ✅
- LRU 缓存，容量默认 1000
- get() → 缓存命中/按需加载
- get_mut() → 标记 dirty
- flush / flush_all / evict / discard
- 淘汰时 dirty 先写回

**文件**: `src/storage/subgraph_cache.rs`

#### Step 3: Redo Log + Checkpoint + 崩溃恢复 ✅
- WAL 追加写 + fsync
- CHECKPOINT marker + 日志轮转
- 恢复: 扫描→重放→truncate
- CRC 校验防损坏

**文件**: `src/storage/redo_log.rs`

#### Step 4: 替换 Graph 后端 + 分区算法 ✅
- DiskGraph: 通过 SubgraphCache + 索引 + WAL 操作
- BFS 聚类分区算法 (partition.rs)
- 保留旧 graph::Graph 作为内存构建器
- persistence/graph_store.rs 和 auto_save.rs 更新

**文件**: `src/storage/disk_graph.rs`, `src/storage/partition.rs`, `src/persistence/graph_store.rs`, `src/persistence/auto_save.rs`

---

## Plan 3: 文档知识提取层 (4 步)

**状态: ✅ 已完成**

### 概述

从 Markdown 文档中自动提取命名实体和关系，通过 LLM (OpenAI 格式) 解析后更新到图库。

### 流程

```
Markdown 文档
    │ 按章节分割
    ▼
章节列表 [(heading, content, depth)]
    │ 逐章送入 LLM (带上下文摘要)
    ▼
LLM (默认 deepseek-v4-flash)
    │ 结构化 JSON 输出
    ▼
解析 → 去重 → 创建 Vertex / Edge → 图库
```

### 上下文窗口约束

- `context_window` = 65536 (默认 deepseek-v4)
- `prompt_overhead_tokens` = 4096
- `max_output_tokens` = 8192
- 每节可用 ≈ 53K tokens
- 超长章节自动递归分割或截断

### 步骤

#### Step 1: config + document reader ✅
- ExtractionConfig (endpoint / model / token limits)
- Markdown 按 heading 分割为 Section
- heading_chain 跟踪层级
- ensure_fits_budget 约束 token 预算

**文件**: `src/extract/config.rs`, `src/extract/document.rs`

#### Step 2: LLM 客户端 ✅
- OpenAI 格式 chat/completions 调用
- 指数退避重试 (默认 3 次)
- Token 用量统计

**文件**: `src/extract/llm_client.rs`

#### Step 3: Prompt + 响应解析 ✅
- System prompt 指导 LLM 输出结构化 JSON
- build_user_message 构建含上下文的用户消息
- parse_response: 清理 markdown fence → JSON → ExtractedEntity + ExtractedRelation

**文件**: `src/extract/extraction.rs`

#### Step 4: Pipeline + 图更新 ✅
- extract_document() 编排器
- 逐章调 LLM → 解析 → 去重 → insert_entity / insert_relation
- MemorySystem 扩展 set_vertex_properties
- ExtractionStats 统计

**文件**: `src/extract/pipeline.rs`, `src/extract/mod.rs`

---

## Plan 4: HTTP 上传入口 + 配置文件系统

**状态: ✅ 已完成**

### 4.1 HTTP 文档上传接口

为文档知识提取新增 REST API 端点。

#### 端点设计

```
POST /extract
  Content-Type: multipart/form-data
  Body: file=<markdown 文件>

  或

POST /extract
  Content-Type: text/markdown
  Body: <原始 markdown 内容>
```

#### 响应

```json
{
  "success": true,
  "stats": {
    "total_sections": 12,
    "processed_sections": 12,
    "total_entities": 45,
    "total_relations": 23,
    "new_vertices": 45,
    "new_edges": 23,
    "total_prompt_tokens": 15800,
    "total_completion_tokens": 3200
  }
}
```

#### 实现要点

| 要点 | 说明 |
|------|------|
| 文件接收 | axum `Multipart` 或原始 body |
| 临时存储 | 写入临时文件后传给 `extract_document`，或直接传内存正文 |
| 异步执行 | 提取可能耗时较长，考虑 `tokio::spawn` + 任务 ID 轮询模式 |
| 配置复用 | 从 settings.json 读取 extraction 配置 |
| 认证 | 暂无（后续可加 API key header 验证） |

#### 影响文件

| 文件 | 改动 |
|------|------|
| `src/gremlin/server.rs` | 新增 `/extract` 路由 + handler |
| `src/extract/pipeline.rs` | 增加接受内存正文的入口（不依赖文件路径） |
| `Cargo.toml` | 可能需要 `axum-extra` 或 `multer` |

### 4.2 配置文件系统

从 `~/.config/bionic-graph/settings.json` 读取所有配置，文件不存在时生成默认配置。

#### 配置文件结构

```json
{
  "server": {
    "host": "127.0.0.1",
    "port": 8080
  },
  "extraction": {
    "api_base_url": "https://api.deepseek.com/v1",
    "model": "deepseek-v4-flash",
    "context_window": 65536,
    "max_output_tokens": 8192,
    "max_retries": 3,
    "concurrent_sections": 1,
    "pass_section_context": true
  },
  "storage": {
    "data_dir": "data",
    "cache_capacity": 1000,
    "checkpoint_interval_entries": 1000,
    "auto_save_interval_secs": 5
  },
  "graph": {
    "default_vertex_labels": ["entity"],
    "max_edges_per_vertex": 10000
  },
  "neural": {
    "default_threshold": 0.7,
    "default_decay_rate": 0.1,
    "default_refractory_ticks": 3,
    "learning_enabled": true,
    "co_fire_window": 5
  }
}
```

#### 加载优先级

```
1. 环境变量（最高优先级）
   BGRAPH_EXTRACT_API_KEY, BGRAPH_HOST, BGRAPH_PORT
2. ~/.config/bionic-graph/settings.json
3. 内置默认值（最低优先级）
```

#### 配置模块拆分

| 新文件 | 职责 |
|--------|------|
| `src/config/mod.rs` | 顶层配置结构 + 加载逻辑 |
| `src/config/settings.rs` | Settings 结构体 (serde Deserialize) + 默认值 |
| `src/config/loader.rs` | 路径解析 + 文件读取 + JSON 解析 + merge 环境变量 |

#### 迁移：现有配置项改为从 Settings 读取

| 现有代码 | 改为 |
|----------|------|
| `main.rs` 中 clap 的 host/port | 从 `settings.server.host/port` 读取，clap 作为覆盖 |
| `ExtractionConfig` 硬编码默认值 | 从 `settings.extraction.*` 读取 |
| `AutoSaveConfig` 硬编码默认值 | 从 `settings.storage.*` 读取 |
| `NeuralNetwork` 默认参数 | 从 `settings.neural.*` 读取 |

#### 实现步骤

| # | 内容 |
|---|------|
| 1 | 创建 `src/config/` 模块，定义 `Settings` 结构体，serde 反序列化 |
| 2 | 实现加载逻辑：先读文件，环境变量覆盖关键字段 |
| 3 | 文件不存在时创建默认配置并写入 |
| 4 | 修改 `main.rs` 使用 Settings 替代部分 clap 参数 |
| 5 | 修改 `ExtractionConfig` 默认值从 Settings 读取 |
| 6 | 修改 `server.rs` 从 Settings 读取 host/port |

---

---

## Plan 5: Gremlin 增强 — 深度遍历 / 模糊过滤 / repeat

**状态: ✅ 已完成**

### 概述

补齐 Gremlin 接口的三个关键缺失功能，使其支持更复杂的图遍历场景。

### 新增功能

#### 5.1 深度遍历 (depth-limited traversal)

给 `out` / `in` / `both` 步骤添加可选 `depth` 字段：

```json
{"step": "out", "label": "knows", "depth": 3}
```

- `depth = 1`（默认）— 当前行为，只走一层
- `depth = N` — 使用 BFS 迭代 N 层，返回所有可达顶点
- 内部调用 `graph::Bfs::with_max_depth(depth)`

**实现**: `src/gremlin/query.rs` — Out/In/Both 加 `depth: Option<usize>`; `src/gremlin/steps.rs` — 当 `depth > 1` 时走 BFS 路径

#### 5.2 文本模糊匹配 (hasText)

新增 `hasText` 步骤，支持子串匹配：

```json
{"step": "hasText", "key": "name", "pattern": "Ali"}
```

- 大小写不敏感的子串匹配
- 对当前流中的 VertexResult 和 EdgeResult 过滤
- 属性值转换为字符串后检查是否包含 pattern

**实现**: `src/gremlin/query.rs` — 新增 `HasText` 变体; `src/gremlin/steps.rs` — 实现 contains 逻辑

#### 5.3 Repeat / Times

新增 `Repeat` 步骤，重复执行一组子步骤 N 次：

```json
{"step": "repeat", "times": 3, "steps": [
  {"step": "out", "label": "knows"}
]}
```

- `times` = 重复次数
- `steps` = 每次迭代执行的子步骤管道
- 第 i 次迭代的输出作为第 i+1 次迭代的输入
- 支持嵌套（repeat 里可以再套 repeat）

**实现**: `src/gremlin/query.rs` — 新增 `Repeat` 变体，含 `times` + `steps`; `src/gremlin/steps.rs` — 递归执行子步骤

### 影响文件

| 文件 | 改动 |
|------|------|
| `src/gremlin/query.rs` | Out/In/Both 加 `depth`; 新增 `HasText`; 新增 `Repeat` |
| `src/gremlin/steps.rs` | 实现三个新步骤的执行逻辑; 递归处理 Repeat 的子步骤 |
| `README.md` | 更新 API 步骤表和示例 |

---

## 文件索引

### 已完成 (23 个模块文件)

```
src/
├── main.rs
├── lib.rs
├── memory_system.rs
├── graph/
│   ├── mod.rs
│   ├── vertex.rs
│   ├── edge.rs
│   ├── graph.rs
│   └── traversal.rs
├── neuron/
│   ├── mod.rs
│   ├── neuron.rs
│   ├── network.rs
│   ├── activation.rs
│   └── learning.rs
├── gremlin/
│   ├── mod.rs
│   ├── query.rs
│   ├── steps.rs
│   └── server.rs
├── persistence/
│   ├── mod.rs
│   ├── graph_store.rs
│   ├── neuron_store.rs
│   └── auto_save.rs
├── storage/
│   ├── mod.rs
│   ├── index.rs
│   ├── subgraph.rs
│   ├── subgraph_cache.rs
│   ├── redo_log.rs
│   ├── partition.rs
│   └── disk_graph.rs
└── extract/
    ├── mod.rs
    ├── config.rs
    ├── document.rs
    ├── llm_client.rs
    ├── extraction.rs
    └── pipeline.rs
```

---

## Plan 6: 多图支持 — 多个命名图，独立数据目录

**状态: ✅ 已完成**

### 概述

当前系统只支持单个知识图谱。需要扩展为支持多个命名图，每个图的数据独立持久化到 `data/{graph_name}/` 子目录下。

### 数据目录结构

```
data/
├── default/                  ← 默认图（向前兼容）
│   ├── graph.bin
│   ├── neural.bin
│   ├── subgraph/
│   └── redo.log
├── my_knowledge_base/        ← 用户创建的图
│   ├── graph.bin
│   ├── neural.bin
│   └── ...
└── ...
```

### API 设计

#### 图管理端点

```
GET    /graphs               — 列出所有图名
POST   /graphs               — 创建新图 {"name": "mygraph"}
DELETE /graphs/{name}        — 删除图及其数据
```

#### 现有端点 → 多图适配

所有数据操作端点（`/gremlin`、`/vertices`、`/edges`、`/search`、`/extract`）需要一个方式指定目标图：

**方式 A（推荐）**: HTTP Header `X-Graph-Name: mygraph`，缺省为 `default`
**方式 B**: JSON Body 中加 `"graph": "mygraph"` 字段

### 核心结构

```rust
pub struct GraphManager {
    graphs: HashMap<String, Arc<Mutex<MemorySystem>>>,
    data_root: PathBuf,
}

impl GraphManager {
    pub fn open(data_root: &Path) -> Self  // 扫描 data/ 下子目录
    pub fn create(&mut self, name: &str)   // 创建新图
    pub fn get(&self, name: &str)          // 获取指定图
    pub fn delete(&mut self, name: &str)   // 删除图
    pub fn list(&self) -> Vec<String>      // 列出所有图
}
```

### 实现步骤

| # | 内容 | 风险 |
|---|------|------|
| 1 | `GraphManager` 结构体 + 创建/打开/列表/删除 | low |
| 2 | `AppState` 改为持有 `GraphManager`，替换单 `Graph` | med — 波及所有 handler |
| 3 | 图管理 REST 端点 (`GET/POST/DELETE /graphs`) | low |
| 4 | 现有 handler 改为从 `X-Graph-Name` header 获取目标图 | med |
| 5 | `main.rs` 初始化 `GraphManager` 而非单 `MemorySystem` | low |

### Plan 4 已实现

```
src/
├── config/                    ← 新增
│   ├── mod.rs
│   ├── settings.rs            # Settings 结构体 (5 个子配置)
│   └── loader.rs              # 加载逻辑 + env 覆盖 + 默认生成
├── extract/
│   ├── config.rs              # 新增 from_settings()
│   └── pipeline.rs            # 新增 extract_content_raw()
├── gremlin/
│   └── server.rs              # AppState 加 extraction_config, 新增 /extract 端点
├── memory_system.rs           # 新增 into_router_with_settings()
└── main.rs                    # 改用 Settings 加载, clap 作为覆盖
```

---

## Plan 7: Time Travel — 版本化顶点/边/神经元，支持时间点查询

**状态: ✅ 已完成**

### 概述

为所有数据实体（顶点、边、神经元）增加 MVCC 能力：每次更新保留旧版本快照，支持软删除，查询时可指定时间点回退到历史状态。

### 新增内部属性

每个 Vertex / Edge / Neuron 新增三个内部字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `updated_at` | `i64` | 最后更新时间（Unix 微秒） |
| `version` | `u64` | 版本号，每次修改递增 |
| `is_deleted` | `bool` | 软删除标记 |

### 版本历史

```rust
pub struct VersionRecord {
    pub version: u64,
    pub updated_at: i64,
    pub properties: HashMap<String, PropertyValue>,
    pub labels: Vec<String>,
}
```

每个 Vertex / Edge 维护 `history: Vec<VersionRecord>`，记录每次修改前的属性快照。

### 更新语义

```
update_properties(vertex, new_props):
  1. snapshot = { version, updated_at, properties, labels }
  2. history.push(snapshot)
  3. version += 1
  4. updated_at = now()
  5. properties = new_props
```

### 删除语义

```
delete(vertex, force=false):
  if force: 物理删除（移除顶点及所有关联）
  if !force: is_deleted = true  （软删除，保留历史）
```

### 查询语义

```
get_vertex(id, query_time=None):
  v = find(id)
  if v.is_deleted and query_time is None:
    return None                          ← 默认不返回已删除
  if query_time is not None:
    snap = find_version_at(v, query_time) ← 回退到指定时间点的快照
    if snap.is_deleted_at(query_time): return None
    return snap
```

### Gremlin 接口

新增 `timeTravel` 步骤，设置查询时间点，影响后续所有步骤：

```json
{"step": "timeTravel", "at": 1718000000000}
```

or 使用 ISO 8601 时间字符串：

```json
{"step": "timeTravel", "at": "2024-06-10T12:00:00Z"}
```

实现方式：在 Gremlin pipeline 中维护一个 `query_time: Option<i64>` 上下文变量，传递给 Graph 的查询方法。

### 实现步骤

| # | 内容 | 风险 | 文件 |
|---|------|------|------|
| 1 | Vertex/Edge 增加 version/updated_at/is_deleted/history | low | `src/graph/vertex.rs`, `src/graph/edge.rs` |
| 2 | Graph 方法升级：add/update 自动管理版本，delete 支持软删除 | med | `src/graph/graph.rs` |
| 3 | Neuron 增加版本字段 | low | `src/neuron/neuron.rs` |
| 4 | Gremlin: timeTravel step + pipeline query_time 传递 | med | `src/gremlin/query.rs`, `src/gremlin/steps.rs` |
| 5 | Subgraph 序列化更新 + 时间点过滤查询 | low | `src/storage/subgraph.rs` |

---

## Plan 8: Compaction — 历史版本归档与裁剪

**状态: ✅ 已完成**

### 概述

当前 time travel 实现有两个问题：
1. **版本膨胀** — 每次更新追加 `VersionRecord`，无上限增长 Vertex/Edge 序列化大小
2. **无归档** — 旧版本永远留在主数据中，即使不再需要 time travel 到那些时间点

Plan 8 增加 compaction 能力：可配置 `max_history` 裁剪旧版本，并将历史 offload 到独立的版本日志文件。

### 设计

#### 方案 A: max_history 裁剪（简单）

```rust
// Vertex 新增配置
pub const MAX_HISTORY: usize = 100;

// Vertex::update_properties 中
pub fn update_properties(&mut self, new_props: ...) {
    self._history.push(VersionRecord { ... });
    // 超出上限时丢弃最旧的版本
    if self._history.len() > MAX_HISTORY {
        self._history.remove(0);  // 丢弃最旧的
    }
    self._version += 1;
}
```

| 优点 | 缺点 |
|------|------|
| 实现简单，O(1) 空间保证 | 旧版本永久丢失，无法 time travel 到裁剪前的时间点 |
| 零额外 I/O | |

#### 方案 B: 历史 offload 到版本日志（推荐）

类似 Iceberg 的 Manifest 设计，将旧版本从 Vertex/Edge 结构体中剥离，存入独立的版本日志文件。

```
数据目录结构:
data/
├── default/
│   ├── graph.bin                    ← 主数据（当前版本 + 最近 N 个历史）
│   ├── version_log/
│   │   ├── 0000000001.vlog          ← 版本日志片段 1
│   │   ├── 0000000002.vlog          ← 版本日志片段 2
│   │   └── ...
│   └── version_index.bin            ← VertexId → vlog 偏移索引
```

每个 `.vlog` 文件：

```
┌──────────────────────────────────────────────┐
│  HEADER                                       │
│  Magic:    "BGVL" (4 bytes)                  │
│  Version:  2 (u32 LE)                        │
│  Count:    条目总数 (u32 LE)                  │
│  Index interval: N (u32 LE)  ← 默认 64       │
│  Index count: M (u32 LE)                     │
├──────────────────────────────────────────────┤
│  SPARSE INDEX                                 │
│  [0]: entry_idx(u32) + offset(u64) +         │
│        first_vertex_id(u64)                   │
│  [1]: entry_idx(u32) + offset(u64) +         │
│        first_vertex_id(u64)                   │
│  ...                                          │
│  [M-1]: ...                                   │
│  每 N 个条目记录一个索引点                     │
├──────────────────────────────────────────────┤
│  ENTRIES                                      │
│  Entry 0:                                     │
│    vertex_id: u64                             │
│    version: u64                               │
│    payload_len: u32                           │
│    payload: [bincode VersionRecord]           │
│  Entry 1: ...                                 │
│  ...                                          │
└──────────────────────────────────────────────┘
```

**稀疏索引查找**:

```
lookup(vertex_id=42):
  1. 二分查找 sparse_index，找到 first_vertex_id ≤ 42 的最大索引点
  2. 跳到对应的 file_offset
  3. 从该位置开始线性扫描最多 N 个条目
  → O(log M + N) 而非 O(总条目数)
```

#### Compaction 策略

| 策略 | 说明 |
|------|------|
| **按时间** | 归档指定时间点之前的所有旧版本，如 `compact(before: 2024-01-01)` |
| **按数量** | 只保留每个 Vertex 最近的 N 个历史版本 |
| **按文件大小** | vlog 文件超过阈值时分裂 |

```rust
pub fn compact(before_timestamp: i64) -> CompactionStats {
    for each vertex:
        keep = [_history 中 updated_at > before_timestamp 的记录]
        archive = [_history 中 updated_at <= before_timestamp 的记录]
        if archive is not empty:
            write_to_vlog(vertex.id, archive)
            vertex._history = keep

    rebuild version_index.bin
    return CompactionStats { archived_records, freed_bytes }
}
```

### API 设计

```bash
# 通过 Gremlin 触发 compaction
curl -X POST localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[{"step":"compact","before":"2024-06-01T00:00:00Z"}]}'

# 或通过 HTTP 端点
curl -X POST localhost:8080/compact \
  -H 'Content-Type: application/json' \
  -d '{"before":"2024-06-01T00:00:00Z"}'
```

### 配置项

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `max_history_per_vertex` | `100` | 每个 Vertex 保留的最大历史记录数（方案 A/B 均适用） |
| `version_log_enabled` | `false` | 是否启用版本日志 offload（方案 B） |
| `auto_compact_interval_secs` | `86400` | 自动 compaction 间隔（秒，默认每天） |

### 实现步骤

| # | 内容 | 风险 | 文件 |
|---|------|------|------|
| 1 | Vertex/Edge 添加 `compact()` 方法：按时间/数量裁剪 `_history` | low | `src/graph/vertex.rs`, `src/graph/edge.rs` |
| 2 | 版本日志 vlog 格式 + 读写 | med | `src/storage/version_log.rs` |
| 3 | 全局 `compact()` 编排器：遍历所有 vertex → 裁剪 + offload | med | `src/storage/compaction.rs` |
| 4 | Gremlin `compact` step + REST 端点 `POST /compact` | low | `src/gremlin/query.rs`, `src/gremlin/server.rs` |
| 5 | 自动 compaction 后台线程 + `settings.json` 配置 | low | `src/main.rs`, `src/config/settings.rs` |

### vlog 稀疏索引增强

在 vlog header 中增加稀疏索引段，支持二分查找定位特定 vertex 的历史。

**文件格式 (v2)**:

```
HEADER: Magic(4) + Version(4) + Count(4) + IndexInterval(4) + IndexCount(4)
INDEX:  [entry_idx(4) + file_offset(8) + first_vertex_id(8)] × IndexCount
ENTRIES: [vertex_id(8) + version(8) + payload_len(4) + payload] × Count
```

**查找算法**:

```
lookup(vertex_id=42):
  1. 二分查找 sparse_index → 找到 first_vertex_id ≤ 42 的最大索引点
  2. 跳到 file_offset
  3. 线性扫描最多 IndexInterval 个条目
  → O(log N) + O(IndexInterval)
```

**向后兼容**: v1 格式（无索引）的读取代码保留，读取时自动降级为全扫描。

**文件**: `src/storage/version_log.rs`

---

## Plan 9: 可选 Time Travel — 创建图时可指定是否启用 time travel

**状态: ✅ 已完成**

### 概述

当前所有 Vertex/Edge 默认启用 time travel（强制携带 `_version`/`_updated_at`/`_is_deleted`/`_history`），带来额外的内存和序列化开销。对于不需要 time travel 的场景，应允许禁用此功能以节省资源。

### 设计

#### GraphConfig 配置

```rust
// src/config/settings.rs
pub struct GraphConfig {
    pub time_travel_enabled: bool,   // 默认 false
    pub max_history_per_vertex: usize, // 默认 100，仅在启用时生效
}
```

#### Graph 结构体增加标志位

```rust
pub struct Graph {
    pub time_travel_enabled: bool,
    // ... 原有字段
}
```

#### 条件版本管理

```
当 time_travel_enabled = false 时：
  - Vertex::update_properties()   → 直接覆盖，不 push history
  - Vertex::soft_delete()         → 物理删除
  - Vertex::at_time()             → 返回 None
  - Edge 同理
  - Graph::remove_vertex()        → 始终 force=true

当 time_travel_enabled = true 时：
  - 当前全部行为保持不变
```

#### GraphManager API 扩展

```rust
// 创建图时可选是否启用 time travel
POST /graphs {"name": "mygraph", "time_travel": true}

// 等效于:
gm.create("mygraph")?.set_time_travel(true);
```

#### 序列化兼容

- `_version`/`_updated_at`/`_is_deleted` 字段始终存在于 struct 中（简化代码），但 `_history` 在禁用时保持为空 Vec
- 反序列化旧数据（没有这些字段的）时填充默认值

#### 实现步骤

| # | 内容 | 风险 | 文件 |
|---|------|------|------|
| 1 | GraphConfig + Graph 增加 `time_travel_enabled` 标志 | low | `src/config/settings.rs`, `src/graph/graph.rs` |
| 2 | Vertex/Edge 方法根据标志决定是否记录 history | low | `src/graph/vertex.rs`, `src/graph/edge.rs` |
| 3 | Graph 根据标志决定 remove_vertex 行为 | low | `src/graph/graph.rs` |
| 4 | GraphManager.create() + API 接受 `time_travel` 参数 | low | `src/graph_manager.rs`, `src/gremlin/server.rs` |
