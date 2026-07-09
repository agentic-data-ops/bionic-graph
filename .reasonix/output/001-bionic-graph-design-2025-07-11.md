# Bionic-Graph：神经元图谱系统设计文档

> 适用于论文与专利的技术设计说明
> 生成日期：2025-07-11

---

## 目录

1. [图核心（Graph Core）](#1-图核心graph-core)
2. [神经网络（Neural Network）](#2-神经网络neural-network)
3. [搜索逻辑（Search Logic）](#3-搜索逻辑search-logic)
4. [Gremlin 管线引擎（Pipeline Engine）](#4-gremlin-管线引擎pipeline-engine)
5. [存储层（Storage Layer）](#5-存储层storage-layer)
6. [系统集成（System Integration）](#6-系统集成system-integration)
7. [前端搜索与可视化（Frontend）](#7-前端搜索与可视化frontend)

---

## 1. 图核心（Graph Core）

### 1.1 数据结构

**Vertex（顶点）** — 知识图谱的基本实体单元：

```rust
pub struct Vertex {
    pub id: VertexId,                    // u64 — 全局唯一标识符
    pub name: String,                    // 实体名称（必填）
    pub keywords: Vec<String>,           // 额外搜索关键词
    pub document: String,                // 来源文档 ID
    pub labels: Vec<String>,             // 类型标签（如 "人物"、"组织"）
    pub properties: HashMap<String, PropertyValue>,  // 自定义属性

    // MVCC 版本字段
    pub _version: u64,                   // 单调递增版本号
    pub _updated_at: i64,                // Unix 微秒时间戳
    pub _is_deleted: bool,               // 软删除标记
    pub _history: Vec<VersionRecord>,    // 历史快照链
}
```

**Edge（边）** — 实体间的关系：

```rust
pub struct Edge {
    pub id: EdgeId,                      // u64
    pub label: String,                   // 关系标签（如 "任职于"）
    pub source: VertexId,                // 源顶点
    pub target: VertexId,                // 目标顶点
    pub document: String,
    pub properties: HashMap<String, PropertyValue>,
    // 同 Vertex 的 MVCC 字段...
}
```

**PropertyValue（属性值）** — 多态值枚举，支持字符串、整数、浮点数、布尔、列表和空值六种类型。

**VersionRecord（版本记录）** — 时间快照单元，记录顶点某一历史时刻的完整状态快照（name、keywords、labels、properties 等全部字段）。

### 1.2 Graph 容器

```rust
pub struct Graph {
    vertices: HashMap<VertexId, Vertex>,               // O(1) 顶点查找
    edges: HashMap<EdgeId, Edge>,                       // O(1) 边查找
    forward: HashMap<VertexId, Vec<EdgeId>>,            // 出边邻接表
    backward: HashMap<VertexId, Vec<EdgeId>>,           // 入边邻接表
    vertex_labels: HashMap<String, HashSet<VertexId>>,  // 标签→顶点集合（反向索引）
    next_vertex_id: VertexId,
    next_edge_id: EdgeId,
    pub time_travel_enabled: bool,                      // MVCC 开关（默认关闭）
}
```

**设计意图**：
- 双邻接表（`forward`/`backward`）实现 O(1) 邻居遍历，避免全表扫描
- `vertex_labels` 反向索引支持按标签快速过滤，无需遍历所有顶点
- `time_travel_enabled` 标志控制 MVCC 的开销——启用时才记录历史

### 1.3 MVCC 版本化机制

**写入时复制（Copy-on-Write）**：当 `time_travel_enabled = true` 时：

1. `update_properties()`、`update_labels()` 被调用时，先在 `_history` 中 push 当前状态的完整快照（`VersionRecord`）
2. 然后 `_version += 1`，`_updated_at = now_micros()`
3. `remove_vertex(id, force=false)` 转为软删除：`_is_deleted = true`
4. `get_vertex()` 默认过滤 `_is_deleted` 的顶点

**时间回溯（Time Travel）**：
```rust
// 在指定时间点查询顶点的历史快照
fn at_time(&self, timestamp_us: i64) -> Option<Vertex>
```
从 `_history` 中反向扫描（最新→最旧），返回时间戳对应的最近快照。Edge 提供相同方法。

**历史压缩（Compact）**：
```rust
// 将早于 before_timestamp 的历史记录从内存移出并返回
fn compact(before_timestamp: i64) -> Vec<VersionRecord>
// 限制历史记录条数上限
fn compact_max(max_count: usize) -> Vec<VersionRecord>
```

### 1.4 图遍历（Traversal）

BFS 和 DFS 均实现为**惰性迭代器**（lazy iterator），使用 `VecDeque`（FIFO）和 `Vec`（LIFO）实现：

```rust
Bfs::new(graph, start: VertexId) -> Self
Bfs::from_many(graph, starts: Vec<VertexId>) -> Self   // 多起点
  .with_edge_label(label) -> Self                      // 按边标签过滤
  .with_max_depth(depth) -> Self                       // 限制深度

// 同上的 Dfs 迭代器

// 一步邻居查询（利用邻接表）
fn out_neighbors(&self, vertex_id, edge_label: Option<&str>) -> Vec<VertexId>
fn in_neighbors(&self, vertex_id, edge_label: Option<&str>) -> Vec<VertexId>
fn both_neighbors(&self, vertex_id, edge_label: Option<&str>) -> Vec<VertexId>
```

通过 `visited: HashSet<VertexId>` 去重，`out_neighbors()` 内部已过滤软删除顶点。

---

## 2. 神经网络（Neural Network）

### 2.1 Neuron 结构体

每个 Neuron（神经元）对应图谱中的一个**概念**，可以关联一个 Vertex（实体）或 Edge（关系）：

```rust
pub struct Neuron {
    pub id: NeuronId,                    // u64
    pub label: String,                   // 概念名称
    pub keywords: Vec<String>,           // 触发关键词（小写，用于语义匹配）
    pub activation: f32,                 // 当前激活值 [0.0, 1.0]
    pub threshold: f32,                  // 触发阈值（默认 0.7）
    pub decay_rate: f32,                 // 每 tick 衰减率（默认 0.1）
    pub refractory_ticks: usize,         // 不应期长度（默认 3）
    pub refractory_remaining: usize,     // 剩余不应期计数
    pub vertex_refs: Vec<VertexId>,      // 关联的图顶点 ID
    pub entity_type: Option<EntityType>,  // 类型标记：Vertex(id) 或 Edge(id)
    pub synapses: Vec<Synapse>,          // 传出突触连接列表
    // MVCC 版本字段...
}
```

**Synapse（突触）**：
```rust
pub struct Synapse {
    pub post_neuron_id: NeuronId,        // 突触后神经元 ID
    pub strength: f32,                   // 连接强度 [0.0, 1.0]
    pub plasticity: f32,                 // Hebbian 学习率
}
```

**EntityType（实体类型）**：`enum EntityType { Vertex(VertexId), Edge(EdgeId) }`

### 2.2 NeuralNetwork（神经网）

```rust
pub struct NeuralNetwork {
    neurons: HashMap<NeuronId, Neuron>,
    synapses: HashMap<NeuronId, Vec<Synapse>>,  // 预计算的突触查找表
    activation_config: ActivationConfig,
    learning_config: LearningConfig,
    total_ticks: u64,
    dirty: bool,               // 是否有未持久化的变更
}
```

**ActivationConfig（激活配置）**：
| 参数 | 默认值 | 说明 |
|------|--------|------|
| `max_ticks` | 20 | 每次查询的最大 tick 数 |
| `hot_threshold` | 0.3 | 热点神经元阈值 |
| `search_mode` | Greedy | 搜索模式：Greedy / Exact |
| `min_synapse_strength` | 0.01 | 最小突触强度过滤 |
| `auto_stabilize` | true | 无触发时自动停止 |
| `greedy_exact_score` | 1.0 | 精确匹配得分 |
| `greedy_partial_score` | 0.8 | 部分匹配得分 |
| `exact_min_score` | 0.5 | 精确模式最低得分 |
| `fuzzy_match_enabled` | false | 是否启用模糊匹配（Levenshtein） |
| `fuzzy_match_threshold` | 0.6 | 模糊匹配相似度阈值 |

### 2.3 扩散激活算法（Spreading Activation）

这是系统最核心的搜索算法，模拟生物神经网络的激活传播。

```
search(neurons, synapses, config, tokens):
│
├── Phase 1: 关键词匹配
│   for each neuron:
│       score = neuron.match_keywords(tokens, config.search_mode)
│       if score > 0: neuron.activation = score
│
├── Phase 2: Tick 循环（最多 config.max_ticks 次）
│   for tick in 0..max_ticks:
│       tick(neurons, synapses)
│       if auto_stabilize && no_new_firings: break
│
└── Phase 3: 结果收集
    统计触发神经元的 vertex_refs
    按引用计数降序排列
    返回 (ranked_vertices, ranked_edges, fired_ids, hot_ids, ticks_run)
```

**单次 tick 执行**：

```
tick(neurons, synapses):
  Phase A — 检查触发：
    for each neuron:
      if neuron.refractory_remaining > 0:
        neuron.refractory_remaining -= 1; continue
      if neuron.activation >= neuron.threshold:
        neuron.fire()  → activation = 1.0, refractory = refractory_ticks

  Phase B — 衰减：
    for each un-fired neuron:
      neuron.activation *= (1.0 - neuron.decay_rate)

  Phase C — 传播：
    for each fired neuron:
      for each synapse in neuron.synapses:
        post_neuron.receive_activation(synapse.strength)
```

**设计意图**：
- 激活从"种子"神经元（直接匹配关键词）出发，沿着突触传播到语义相关的概念
- 衰减机制防止激活无限扩散
- 不应期（refractory period）防止同一神经元在短时间内被反复触发
- 传播路径越长，激活强度呈指数级衰减，天然形成相关性排序

### 2.4 关键词匹配算法

```rust
fn match_keywords(&self, tokens: &[String], mode: SearchMode) -> f32
```

**Greedy 模式**：
- 任一 token 与神经元 label 或 keywords 完全匹配 → 返回 `greedy_exact_score`（1.0）
- 子串匹配 → 返回 `greedy_partial_score`（0.8）
- 若 `fuzzy_match_enabled` → Levenshtein 模糊回退 → 返回 0.8 * 相似度
- 匹配即停：取第一个匹配的 token 的分数

**Exact 模式**：
- 所有 token 都必须匹配（精确/子串/模糊均可）
- 返回 `匹配的 token 数 / 总 token 数`
- 需 ≥ `exact_min_score`（0.5）才视为匹配

**Levenshtein 相似度**：`similarity = 1.0 - edit_distance / max(len1, len2)`，基于两行 DP 实现。

### 2.5 Hebbian 学习

遵循 Hebb 定律——**"一起触发的神经元，连接也会增强"**（Fire together, wire together）：

```rust
fn hebbian_update(neurons, synapses, firing_history):
    for each co-firing pair (pre, post) in recent tick:
        if synapse exists:
            strength += plasticity * (1.0 - strength)  // 增强
        else:
            create new synapse with initial strength
    for each non-co-firing pair:
        strength -= plasticity * strength               // 衰减
```

- `FiringHistory` 维护一个环形缓冲区，记录最近 N 个 tick 的触发情况
- 学习率由 `plasticity` 参数控制
- 长期不共激活的突触逐渐衰减至消失
- 激活搜索完成后自动触发学习（在 `network.search()` 末尾调用）

### 2.6 Neuron 自动创建与 Synapse 自动化

**Neuron 创建链路**：
```
POST /vertices 或 POST /edges
  → HTTP handler 调用 Neuron::for_vertex(vertex) 或 Neuron::for_edge(edge)
    → 提取 name + labels + keywords 作为 Neuron.label 和 Neuron.keywords
    → 设置 entity_type = EntityType::Vertex(id) 或 EntityType::Edge(id)
    → vertex_refs.push(vertex_id)
  → auto_synapse(&mut nn, &graph)
    → 遍历所有已存在的神经元，与新神经元共享的关键词
    → 关键词重叠数 ≥ 1 则创建突触（strength = overlap / max_keywords）
```

`auto_synapse` 实现了**自动突触形成**：每当新增一个神经元，系统自动扫描已有神经元，根据关键词重叠度建立初始突触连接。这是知识图谱自发形成语义网络的基础。

---

## 3. 搜索逻辑（Search Logic）

### 3.1 搜索架构

系统提供三个搜索入口：

```
用户输入
    │
    ├── Gremlin search step ──→ 后端 REST API
    │       POST /gremlin { steps: [ { step: "search", keywords: [...], mode: "greedy" } ] }
    │
    ├── POST /search API ──→ 简化的关键词搜索端点
    │       POST /search { keywords: [...], mode: "greedy" }
    │
    └── 前端语义搜索 ──→ LLM 提取关键词 → graphSearch → LLM 过滤
```

### 3.2 Gremlin Search Step 完整执行流程

```
用户请求: { step: "search", keywords: ["人工智能", "机器学习"], mode: "greedy" }
    │
    ├─ 1. SearchHandler (steps.rs)
    │     提取 keywords 和 mode，调用 nn.search()
    │
    ├─ 2. NeuralNetwork::search() (network.rs)
    │     a) reset() → 所有神经元 activation = 0
    │     b) 分词 → 按空白 + 非字母数字分割
    │     c) activation::search(neurons, synapses, config, tokens)
    │
    ├─ 3. activation::search() (activation.rs)
    │     Phase 1: 关键词匹配
    │       遍历所有神经元 → neuron.match_keywords(tokens, mode)
    │     Phase 2: 扩散传播（多 tick）
    │       直到自动稳定或达到 max_ticks
    │     Phase 3: 收集结果
    │       聚合触发神经元的 vertex_refs → 排序 → 返回
    │
    ├─ 4. 结果映射 (steps.rs)
    │     取前 100 个顶点 + 所有排名的边
    │     将 Vertex/Edge 转换为 TraversalResult
    │     包装到 QueryResponse { data, ticks_used, neurons_fired }
    │
    └─ 5. Hebbian 学习 (network.rs)
         hebbian_update() 根据本次搜索的触发记录调整突触强度
```

### 3.3 Greedy vs Exact 模式对比

| 特性 | Greedy（贪婪） | Exact（精确） |
|------|----------------|---------------|
| 匹配策略 | 任意关键词匹配即返回 | 所有关键词必须匹配 |
| 适用场景 | 宽泛搜索、发现关联 | 精确检索、筛选 |
| 匹配速度 | 更快（找到即停） | 略慢（需检查全部 token） |
| 结果数量 | 较多 | 较少但更精准 |
| 前端默认 | ✅ 默认模式 | — |

### 3.4 前端搜索流程

#### 关键词模式（Keyword Search）

```
用户输入 → 前端分词 → POST /gremlin (search step, mode=greedy/exact)
         → 后端返回匹配的顶点和边 → 直接展示图谱结果
```

#### 语义模式（Semantic Search）— 三阶段管线

```
Step 1 — LLM 关键词提取
  系统提示:
    "Select 3-5 key search keywords from the user's query below.
     ONLY pick words/phrases that actually appear in the query —
     do NOT generate, infer, or translate any new words.
     Return ONLY a JSON array of strings."
  ↓
Step 2 — 图谱搜索
  将 LLM 提取的关键词传给 graphSearch()（始终使用 greedy 模式）
  → POST /gremlin { step: "search", keywords, mode: "greedy" }
  ↓
Step 3 — LLM 语义过滤
  系统提示:
    "You are a semantic relevance filter. Given a user query and
     search results (vertices + edges), identify which results are
     semantically relevant..."
  选择规则:
    1. 选择与查询实体匹配的顶点
    2. 选择与查询关系匹配的边
    3. 闭包规则：若选中一条边，自动包含其 source 和 target 顶点
  输出: 逗号分隔的 1-based 索引列表，或 "NONE"
```

**设计意图**：语义模式利用 LLM 的理解能力进行两步增强——先用 LLM 提取核心概念（而非粗暴分词），再用 LLM 对搜索结果进行语义相关性重排序。闭包规则确保图谱的连通性不被破坏。

---

## 4. Gremlin 管线引擎（Pipeline Engine）

### 4.1 管线架构

Gremlin 查询以**步骤管线（step pipeline）**的形式执行，前一步的输出作为下一步的输入：

```rust
// 输入格式
POST /gremlin {
  "steps": [
    { "step": "search", "keywords": ["AI"], "mode": "greedy" },
    { "step": "out", "depth": 1 },
    { "step": "hasLabel", "label": "人物" },
    { "step": "values", "key": "name" },
    { "step": "limit", "count": 10 }
  ]
}

// 内部执行: traverse steps → intermediate Vec<TraversalResult> → next step
```

### 4.2 全部 15 个步骤

| 步骤 | JSON 标签 | 输入 | 输出 | 说明 |
|------|-----------|------|------|------|
| `Search` | `"search"` | — | 顶点+边 | 神经索引语义/关键词搜索 |
| `V` | `"V"` | — | 顶点 | 按 ID 获取或全部顶点 |
| `E` | `"E"` | — | 边 | 按 ID 获取或全部边 |
| `Has` | `"has"` | 任意 | 过滤后 | 按属性 key=value 过滤 |
| `HasNot` | `"hasNot"` | 任意 | 过滤后 | 属性不匹配 |
| `HasKey` | `"hasKey"` | 任意 | 过滤后 | 属性键存在 |
| `HasValue` | `"hasValue"` | 任意 | 过滤后 | 任意属性有此值 |
| `HasLabel` | `"hasLabel"` | 顶点/边 | 过滤后 | 按标签过滤 |
| `HasText` | `"hasText"` | 顶点/边 | 过滤后 | 属性值不区分大小写子串匹配 |
| `Out/In/Both` | `"out"`/`"in"`/`"both"` | 顶点 | 邻居顶点 | 遍历邻居（支持深度 N 级 BFS） |
| `OutE/InE/BothE` | `"outE"`/`"inE"`/`"bothE"` | 顶点 | 边 | 遍历邻居并返回边 |
| `Values` | `"values"` | 顶点/边 | 属性值 | 提取指定属性值 |
| `Limit` | `"limit"` | 任意 | 前 N 个 | 取前 N 条结果 |
| `Count` | `"count"` | 任意 | 计数 | 返回结果总数 |
| `Dedup` | `"dedup"` | 任意 | 去重 | 按 ID 去重 |
| `Repeat` | `"repeat"` | 任意 | 管线后 | 循环执行子管线 N 次 |
| `TimeTravel` | `"timeTravel"` | 顶点 | 历史快照 | 将顶点替换为历史时间点快照 |
| `Compact` | `"compact"` | — | — | 归档历史记录到版本日志 |

### 4.3 TimeTravel 步骤

```rust
TraversalStep::TimeTravel { at } => {
    // at 支持: Unix 微秒整数 / ISO 8601 字符串 / "2024-06-10" 纯日期
    let timestamp = parse_time_value(at)?;
    // 将当前管线中的每个顶点替换为其在指定时间点的历史快照
    input.into_iter().filter_map(|r| match r {
        TraversalResult::VertexResult(v) => {
            let original = g.get_vertex_including_deleted(v.id)?;  // 包括已删除的
            let snapshot = original.at_time(timestamp)?;           // 历史回溯
            Some(vertex_from_snapshot(&snapshot))
        }
        _ => Some(r),
    }).collect()
}
```

**设计意图**：TimeTravel 不是查询整个历史数据库，而是作用于**当前管线结果集**上——先通过搜索得到相关顶点，再将这些顶点"快照"到指定时间点。这使得用户可以在保持查询意图的同时查看历史状态。

### 4.4 Repeat 步骤

```rust
TraversalStep::Repeat { times, steps } => {
    let mut current = input;
    for _ in 0..*times {
        current = run_steps(&current, steps, ...)?;
        if current.is_empty() { break; }  // 早期终止
    }
    Ok(current)
}
```

`run_steps()` 是主执行器的精简版，支持 `V/Out/In/Has/Limit/Count/Dedup` 等子步骤，但不包括 `Search/TimeTravel/Compact`。

---

## 5. 存储层（Storage Layer）

### 5.1 整体架构

```
                    ┌─────────────────────────┐
                    │     GraphManager         │
                    │  HashMap<String, Handle>  │
                    └──────┬──────────────────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
      ┌───────────┐ ┌──────────┐ ┌──────────┐
      │  Graph    │ │ NeuralNet│ │RedologWal│
      │ (in-mem)  │ │(in-mem)  │ │(crash-R) │
      └─────┬─────┘ └────┬─────┘ └────┬─────┘
            │            │            │
            ▼            ▼            ▼
      ┌───────────┐ ┌──────────┐ ┌──────────┐
      │ graph.bin │ │neural.bin│ │redo.wal  │
      │ (bincode) │ │(bincode) │ │(binary)  │
      └───────────┘ └──────────┘ └──────────┘
            │                          │
            ▼                          ▼
      ┌───────────────────────────────────────┐
      │       data/graphs/<name>/              │
      │         (磁盘目录)                       │
      │  + version_log/ (vlog 归档)             │
      └───────────────────────────────────────┘
```

### 5.2 SubgraphPartition（子图分区）

将大图划分为多个子图（Subgraph），每个子图独立序列化：

**Partition Key 分配策略**：

| 策略 | 说明 |
|------|------|
| `AutoCluster`（默认） | BFS 聚类：以种子顶点为起点的 BFS 集群作为同一子图，最大深度 3 |
| `ByLabel` | 按标签分组（当前简化实现为任意有容量的子图） |
| `SingleSubgraph` | 所有顶点放入单个子图 |

**每个 Subgraph 包含**：
- `vertices: Vec<Vertex>` — 该子图拥有的顶点
- `edges: Vec<Edge>` — 两端同子图的边
- `cross_edges: Vec<CrossEdgeRef>` — 跨子图边引用
- 单调递增的 ID 计数器（按 `id * 1_000_000 + 1` 初始化确保全局唯一）

单个子图序列化上限：**64 MiB**（`MAX_SUBGRAPH_BYTES`）。

### 5.3 SubgraphCache（LRU 缓存）

```rust
struct CachedEntry {
    subgraph: Subgraph,
    dirty: bool,           // 被修改但尚未写回磁盘
    size_bytes: u64,
}

pub struct SubgraphCache {
    entries: HashMap<SubgraphId, CachedEntry>,  // O(1) 随机访问
    order: VecDeque<SubgraphId>,                // front=MRU, back=LRU
    capacity: usize,                            // 默认 1000
    data_dir: PathBuf,
}
```

**Eviction 策略**：
- 基于条目数而非字节数
- 淘汰 LRU 端条目时，若 dirty 则先 flush 再移除
- `discard()` 用于删除场景，不做写回

**与分区的协作**：`DiskGraph` 通过 `VertexIndex`（vertex_id → subgraph_id + offset）实现 O(1) 定位，`AutoCluster` 策略将相邻顶点聚入同一子图以提升缓存命中率。

### 5.4 RedologWal（WAL — 预写日志）

**文件格式**（单文件 `redolog.wal`）：

```
[type: 1B] [data_len: 4LE] [data: data_len B] [crc32: 4LE]
```

**操作类型码**：

| 码 | 操作 | 范围 |
|----|------|------|
| `0x01-0x06` | 图操作（add/remove/update vertex/edge） | Graph |
| `0x11-0x15` | 神经元操作（add/remove/update neuron/synapse） | Neuron |
| `0xFF` | Checkpoint 标记 | Control |

**写入策略**：
```rust
fn write_batch(&mut self, entries: &[(u8, Vec<u8>)]) {
    // 1. 所有条目串联到一个缓冲区
    // 2. 单次 write_all
    // 3. 单次 sync_all (fsync)
}
```

**原子性保证**：一批突变在一次 `write_all` + `sync_all` 中完成，崩溃要么写入全部，要么什么都没写入。Graph 和 Neuron 的变更可以混合在同一批中。

**Crash Recovery（崩溃恢复）**：
1. 读取 WAL 文件中所有条目，逐条 CRC 校验
2. 定位最后一个 `OP_CHECKPOINT` 条目
3. 回放 checkpoint 之后的全部条目到 Graph + NeuralNetwork
4. CRC 不匹配时停止（检测截断/损坏）

**Checkpoint 机制**：
```rust
wal.checkpoint();                    // 写入 OP_CHECKPOINT 标记
wal.truncate_after_checkpoint();     // 裁剪 checkpoint 之前的旧条目
```

### 5.5 VersionLog（vlog — 版本日志）

**文件结构**（v2 格式，带稀疏索引）：

```
HEADER: [Magic: "BGVL"] [Version: 2] [Count] [IndexInterval: 64] [IndexCount]

INDEX SECTION: [IndexEntry × IndexCount]
   每项: { entry_idx, file_offset, first_vertex_id }

DATA ENTRIES: [Entry × Count]
   每项: { vertex_id, version, payload_len, payload(bincode: VersionRecord) }
```

**文件名**：`{data_dir}/version_log/{timestamp_us}_{sequence:04}.vlog`

**稀疏索引查找**：在索引区做二分查找定位目标 vertex_id 所在的偏移范围，然后线性扫描该范围。

### 5.6 Compaction（归档）

**触发**：手动触发（通过 Gremlin `compact` step 或 REST API 调用）

**执行流程**：
```
compact_graph(graph, data_dir, before_timestamp, sequence):
  1. 遍历所有顶点
  2. 对每个顶点调用 compact(before_timestamp)
     → 将 _history 中 updated_at < before_timestamp 的记录移除并返回
  3. 同样调用 compact_max(100) → 限制历史最大 100 条
  4. 收集所有移除的记录 → 批量写入 vlog 文件
```

**完整数据链**：
```
Vertex._history (RAM)
    → compact() 移出
    → write_vlog() 写入 vlog 文件
    → [vlog 文件] 作为长期归档存储
    → [可选] 通过 lookup_vertex_in_vlog() 还原历史查询
```

---

## 6. 系统集成（System Integration）

### 6.1 GraphManager（多图管理器）

```rust
pub struct GraphManager {
    graphs: HashMap<String, GraphHandle>,
    data_root: PathBuf,
    neural_config: NeuralConfig,
}

pub struct GraphHandle {
    pub name: String,
    pub graph: Arc<Mutex<Graph>>,
    pub neural_network: Arc<Mutex<NeuralNetwork>>,
    pub redolog_wal: Arc<Mutex<RedologWal>>,
    pub data_dir: PathBuf,       // data/graphs/{name}/
}
```

**启动加载流程**：
1. 扫描 `data_root/graphs/` 下所有子目录
2. 对每个含 `graph.bin` 或 `neural.bin` 的目录：反序列化 → 打开 WAL → 回放未持久化的变更
3. 无任何图时自动创建 `"default"` 图

### 6.2 Auto-Save（自动保存）

两种模式：

| 模式 | 触发间隔 | 保存条件 |
|------|---------|---------|
| Legacy in-memory | 5 秒 | graph：无条件；neural：仅 dirty 时 |
| DiskGraph checkpoint | 5 秒 | WAL 条目数超阈值时 |

**写路径**：
```
内存修改 → RedologWal 追加 → (每5秒) → 全量 bincode 序列化
→ 写入 graph.bin / neural.bin → WAL checkpoint + truncate
```

### 6.3 MaaS 代理（Model as a Service）

**架构**：前端 → 后端 MaaS 代理 → 第三方 LLM 提供商

```
前端（浏览器）
    │ POST /maas/openai/v1/chat/completions { model: "DeepSeek/deepseek-v4-flash", ... }
    ▼
后端 MaaS 代理（src/maas/openai.rs）
    │ 1. 解析 model = "Provider/Model" → (provider_name, model_name)
    │ 2. 从 settings 查找 api_key + api_base_url
    │ 3. 替换 model 为纯 model_name
    │ 4. 添加 Authorization: Bearer {api_key}
    ▼
第三方 LLM API（OpenAI-compatible）
```

**安全设计**：
- API Key 仅存储在后端 `settings.json`，永不暴露给前端
- `GET /settings` 序列化时隐去 `api_key` 字段
- `PUT /settings` 传入空 api_key 时保留旧值，防止无意清空
- 前端始终调用 `/maas/openai/v1/...` 代理端点

### 6.4 完整 API 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/health` | 系统健康检查 + 聚合统计 |
| GET/POST/DELETE | `/graphs` | 图谱列表/创建/删除 |
| POST | `/gremlin` | Gremlin 管线查询 |
| POST | `/search` | 神经关键词搜索 |
| POST | `/vertices` | 添加顶点（自动创建神经元+突触） |
| POST | `/edges` | 添加边（同上） |
| PUT | `/vertices/:id` | 更新顶点属性/名称/关键词/标签 |
| DELETE | `/vertices/:id` | 删除顶点（级联删除关联边） |
| POST | `/neurons` | 神经网络管理 |
| POST | `/neurons/:id/link` | 关联顶点到神经元 |
| POST | `/neurons/:id/synapse` | 手动创建突触 |
| POST | `/compact` | 历史归档 |
| POST | `/reindex` | 重建边到神经元的索引 |
| GET/PUT | `/settings` | LLM 提供商配置 |
| GET/PUT | `/settings/neural` | 神经/搜索/学习参数配置 |
| POST | `/documents/:id/extract` | 异步文档提取 |
| GET | `/maas/openai/v1/models` | 模型列表（含 `x-default-model` 头部） |
| POST | `/maas/openai/v1/chat/completions` | OpenAI 兼容的聊天代理 |

---

## 7. 前端搜索与可视化（Frontend）

### 7.1 架构

```
App.jsx
├── Sidebar          — 对话列表、知识库、图库、设置入口
├── ChatArea         — 聊天主区域
│   ├── MessageList  — 消息列表（支持图搜索结果内嵌视图）
│   └── ChatInput    — 输入框、模型选择、模式控制
├── KnowledgeBase    — 文档管理 + LLM 提取
├── GraphManagerDialog — 图库管理
└── SettingsDialog   — 提供商/搜索参数配置
```

### 7.2 数据流

```
用户输入文本
    │
    ├─ 纯聊天模式 (useGraph=false)
    │   ├─ 构建 LLM 消息列表
    │   ├─ chatCompletionProxy() → 后端 MaaS → 第三方 LLM
    │   └─ 流式渲染（SSE → parseSSEStream → 逐 token 更新）
    │
    └─ 图谱模式 (useGraph=true)
        ├─ 关键词搜索 (mode=keyword)
        │   ├─ 前端分词 → POST /gremlin (search)
        │   └─ 直接渲染图谱结果
        └─ 语义搜索 (mode=semantic)
            ├─ Step 1: LLM 提取关键词（流式）
            ├─ Step 2: POST /gremlin (search)
            └─ Step 3: LLM 过滤相关性（流式）→ 渲染
```

### 7.3 图谱可视化（vis-network）

**配置双主题**：暗色/亮色两套完整的 node/edge 样式配置，通过 CSS 变量和 `useTheme` 切换。

**物理引擎**：`forceAtlas2Based`，重力 `-40`，弹簧长度 `180`，`randomSeed 42` 确保布局可复现。

**交互**：悬停提示（tooltip）、缩放/拖拽、节点选中高亮。

### 7.4 文档提取流程

```
Markdown 文件
    → LLM 生成标题/标签
    → LLM 提取实体（实体名+类型+属性）和关系（源实体+目标实体+关系标签）
    → POST /vertices 逐个创建顶点（自动创建神经元）
    → POST /edges 逐个创建边（自动创建神经元+auto_synapse）
    → 前端实时轮询任务进度
```

---

## 附录：关键设计决策

1. **为什么扩散激活优于向量嵌入搜索？**
   - 扩散激活天然支持多跳语义关联（如"苹果"→"库克"→"发布会"），而无需预先训练向量映射
   - 神经元之间的突触权重可以随使用动态调整（Hebbian 学习），实现知识图谱的自适应演化
   - 适用于冷启动场景——新添加的实体立即可以被搜索到，无需重新训练模型

2. **为什么使用 WAL + 全量快照双写？**
   - WAL 提供崩溃恢复的原子性保证（write + fsync）
   - 全量快照（bincode）提供高效的读取路径——无需 replay 所有日志即可启动
   - Checkpoint 机制裁剪 WAL，防止日志无限增长

3. **为什么 Greedy 是默认搜索模式？**
   - 知识图谱搜索更注重"发现"而非"精确匹配"
   - Greedy 模式返回更丰富的结果集，有助于用户发现非预期的关联
   - Exact 模式作为精确检索的可选补充

4. **语义搜索为什么分为两阶段（提取+过滤）？**
   - 避免将大量图谱数据直接塞入 LLM 上下文窗口
   - 第一阶段用 LLM 提取精炼的关键词，减少噪声
   - 第二阶段对搜索结果做细粒度过滤，可处理前 30 条结果
   - 闭包规则确保关系完整性
