# Plan 104 — 自定义属性索引 (vertex_properties / edge_properties)

## 目标

允许用户基于顶点和边的属性（properties）创建自定义索引，通过 API 提供索引的创建、删除和查询能力。索引在数据库关闭时持久化到文件，启动时从文件加载或从 data file 扫描重建。

## 存储设计

### MemoryIndex 新增字段

```rust
// 自定义顶点属性索引: property_key → (property_value → [MetaPointer])
pub vertex_properties: HashMap<String, BTreeMap<String, Vec<MetaPointer>>>,
// 自定义边属性索引: property_key → (property_value → [MetaPointer])
pub edge_properties: HashMap<String, BTreeMap<String, Vec<MetaPointer>>>,
```

两层 map：
- 第一层 key = 属性名（如 `"city"`、`"occupation"`）
- 第二层 key = 属性值（如 `"Beijing"`、`"Engineer"`）
- value = 指向数据记录的指针列表

### 索引文件

```
<graph_dir>/index/
├── index_state                  ← 持久化标记
├── index_vertex_properties      ← 顶点属性索引（bincode）
├── index_edge_properties        ← 边属性索引（bincode）
└── ...其他索引
```

### 生命周期

```
正常关闭 (SIGTERM)
  └─ Graph::close()
       ├─ flush() + sync()
       ├─ save_to_dir("index")
       │    ├─ save_one("index_vertex_properties", &self.vertex_properties)
       │    ├─ save_one("index_edge_properties", &self.edge_properties)
       │    └─ write("index_state")
       └─ renew()

重启
  ├─ load_from_dir("index") → 有 index_state?
  │    ├─ 是 → 从文件加载所有索引 ← 快速启动
  │    └─ 否 → build_memory_index() 扫描 data file ← 崩溃恢复

崩溃
  └─ 无 index_state → 回退到 data file 扫描
```

### 构建/维护

| 操作 | 位置 | 动作 |
|------|------|------|
| 启动重建 | `memory_index_builder.rs` | 扫描顶点/边时，遍历 properties，对每个 string/数值属性写入索引 |
| 创建顶点 | `crud.rs::create_vertex` | 遍历 properties，对每个已索引的 key 插入 value→ptr |
| 更新顶点 | `crud.rs::update_vertex` | 对比新旧 properties，增删索引条目 |
| 删除顶点 | `crud.rs::hard_delete_vertex` | 遍历 properties，移除所有索引条目 |
| 创建边 | `crud.rs::create_edge` | 同上 |
| 更新边 | `crud.rs::update_edge` | 同上 |
| 删除边 | `crud.rs::hard_delete_edge` | 同上 |

> **注意**：只有通过 `POST /indices/vertex/properties` 或 `POST /indices/edge/properties` 注册过的 property key 才会被索引。未注册的 key 不建立索引，减少不必要的索引开销。

## REST API

### 路由前缀

```
/indices/vertex/properties
/indices/edge/properties
```

### 端点

#### POST `/indices/vertex/properties` — 创建顶点属性索引

为指定 property key 建立索引。如果该 key 已有索引，返回已存在状态。

**请求体**：

```json
{
  "key": "city"
}
```

**响应**：

```json
{
  "status": "ok",
  "key": "city",
  "type": "vertex",
  "created": true
}
```

#### DELETE `/indices/vertex/properties/:key` — 删除顶点属性索引

删除指定 property key 的索引。内存中清除该 key 的索引数据。

**响应**：

```json
{
  "status": "ok",
  "key": "city",
  "deleted": true
}
```

#### GET `/indices/vertex/properties` — 列出所有已索引的顶点属性 key

**响应**：

```json
{
  "status": "ok",
  "keys": ["city", "occupation", "age"]
}
```

#### GET `/indices/vertex/properties/:key?value=xxx` — 按属性值查询顶点

查询指定 property key 下值为 value 的所有顶点。

**查询参数**：`value`（必填）

**响应**：

```json
{
  "status": "ok",
  "key": "city",
  "value": "Beijing",
  "data": [
    {"id": 1, "name": "Wang Wei_0", "labels": ["person"], "properties": {"city": "Beijing", ...}},
    {"id": 5, "name": "Zhao Huang_4", "labels": ["person"], "properties": {"city": "Beijing", ...}}
  ]
}
```

#### DELETE `/indices/vertex/properties` — 批量删除顶点属性索引

**请求体**：

```json
{
  "keys": ["city", "occupation"]
}
```

**响应**：

```json
{
  "status": "ok",
  "deleted": ["city", "occupation"]
}
```

边属性索引同上，路径前缀为 `/indices/edge/properties`。

### 批量查询

#### POST `/indices/vertex/properties/query` — 批量查询顶点属性索引

**请求体**：

```json
{
  "queries": [
    {"key": "city", "value": "Beijing"},
    {"key": "occupation", "value": "Engineer"}
  ]
}
```

**响应**：

```json
{
  "status": "ok",
  "results": [
    {"key": "city", "value": "Beijing", "count": 50, "data": [...]},
    {"key": "occupation", "value": "Engineer", "count": 31, "data": [...]}
  ]
}
```

## Python SDK / CLI

### Client 方法

```python
class Client:
    # 顶点属性索引
    def create_vertex_property_index(self, key: str, graph: str = None) -> dict
    def delete_vertex_property_index(self, key: str, graph: str = None) -> dict
    def delete_vertex_property_indices(self, keys: list[str], graph: str = None) -> dict
    def list_vertex_property_indices(self, graph: str = None) -> dict
    def query_vertex_property_index(self, key: str, value: str, graph: str = None) -> dict
    def query_vertex_property_indices(self, queries: list[dict], graph: str = None) -> dict

    # 边属性索引（同上，edge 替换 vertex）
    def create_edge_property_index(self, key: str, graph: str = None) -> dict
    def delete_edge_property_index(self, key: str, graph: str = None) -> dict
    def delete_edge_property_indices(self, keys: list[str], graph: str = None) -> dict
    def list_edge_property_indices(self, graph: str = None) -> dict
    def query_edge_property_index(self, key: str, value: str, graph: str = None) -> dict
    def query_edge_property_indices(self, queries: list[dict], graph: str = None) -> dict
```

### CLI 命令

```bash
# 顶点属性索引
bgcli index vertex-property create --key city
bgcli index vertex-property delete --key city
bgcli index vertex-property delete-batch --keys '["city","occupation"]'
bgcli index vertex-property list
bgcli index vertex-property query --key city --value Beijing
bgcli index vertex-property query-batch --queries '[{"key":"city","value":"Beijing"},{"key":"age","value":"30"}]'

# 边属性索引（edge-property 替换 vertex-property）
bgcli index edge-property create --key strength
bgcli index edge-property delete --key strength
bgcli index edge-property list
bgcli index edge-property query --key strength --value 0.8
```

### CLI 命令行结构

```
bgcli index
├── bgcli index vertex-property
│   ├── create --key <key>
│   ├── delete --key <key>
│   ├── delete-batch --keys <json>
│   ├── list
│   ├── query --key <key> --value <value>
│   └── query-batch --queries <json>
└── bgcli index edge-property
    ├── create --key <key>
    ├── delete --key <key>
    ├── delete-batch --keys <json>
    ├── list
    ├── query --key <key> --value <value>
    └── query-batch --queries <json>
```

## 实现步骤

### 步骤 1: MemoryIndex 新增字段和方法

**文件**: `src/storage/memory_index.rs`

- 新增 `vertex_properties: HashMap<String, BTreeMap<String, Vec<MetaPointer>>>`
- 新增 `edge_properties: HashMap<String, BTreeMap<String, Vec<MetaPointer>>>`
- 添加 Serialize/Deserialize derives
- 新增方法:
  - `add_vertex_property(key, value, ptr)` / `add_edge_property(key, value, ptr)`
  - `remove_vertex_property(key, value, ptr)` / `remove_edge_property(key, value, ptr)`
  - `remove_vertex_property_key(key, value)` / `remove_edge_property_key(key, value)` — 删除整个 key 的所有条目
  - `get_vertex_property(key, value)` / `get_edge_property(key, value)` — 返回 `Option<&[MetaPointer]>`
  - `list_vertex_property_keys()` / `list_edge_property_keys()` — 返回所有已注册 key
  - `has_vertex_property(key)` / `has_edge_property(key)` — 检查 key 是否已索引

### 步骤 2: 持久化

**文件**: `src/storage/memory_index.rs`

- `save_to_dir()` 中增加 `save_one("index_vertex_properties", &self.vertex_properties)`
- `save_to_dir()` 中增加 `save_one("index_edge_properties", &self.edge_properties)`
- `load_from_dir()` 中增加对应 `load_one` 调用
- `remove_index_files()` 中增加文件名

### 步骤 3: REST API

**文件**: `src/gremlin/mod.rs`（路由注册）+ 新建 `src/gremlin/indices.rs`

路由（36 条）：

```rust
// 顶点属性索引
.route("/indices/vertex/properties", post(create_vertex_property_index))
.route("/indices/vertex/properties", get(list_vertex_property_indices))
.route("/indices/vertex/properties/query", post(query_vertex_property_indices))
.route("/indices/vertex/properties/:key", get(query_vertex_property_index))
.route("/indices/vertex/properties/:key", delete(delete_vertex_property_index))
.route("/indices/vertex/properties", delete(delete_vertex_property_indices))

// 边属性索引
.route("/indices/edge/properties", post(create_edge_property_index))
.route("/indices/edge/properties", get(list_edge_property_indices))
.route("/indices/edge/properties/query", post(query_edge_property_indices))
.route("/indices/edge/properties/:key", get(query_edge_property_index))
.route("/indices/edge/properties/:key", delete(delete_edge_property_index))
.route("/indices/edge/properties", delete(delete_edge_property_indices))
```

处理函数：在 `indices.rs` 中实现 12 个 handler 函数。

**注意路由顺序**：带 `/query` 的路由必须在 `/:key` 之前注册，否则 `query` 会被当作 `key` 参数。

### 步骤 4: CRUD 维护

**文件**: `src/graph/crud.rs`

在以下位置添加索引维护代码：

- `create_vertex` — 对每个 properties 中的 key：如果 `mi.vertex_properties.contains_key(key)`，则插入 `(value, ptr)`
- `hard_delete_vertex` — 反操作：删除所有已索引的 entry
- `update_vertex` — 对比新旧 properties，对已索引的 key 做 diff 更新
- `create_edge` / `hard_delete_edge` / `update_edge` — 同上

**关键设计决策**：每个顶点/边创建/删除时，都检查 `mi.vertex_properties.contains_key(key)`。只有用户已注册的 key 才被索引。这意味着启动时扫描 data file 也需要做同样的检查——但启动时 `vertex_properties` 是空的。所以启动扫描时：默认不索引任何属性；需要用户通过 API 注册 key 后，后续的 CRUD 操作才生效。

对于启动扫描的改进方案：启动时扫描 data file，对每个顶点/边，遍历所有 properties 的 key，如果该 key **在启动时已知是已索引的**（从持久化的 index 文件恢复），则建立索引。但加载的 index 文件中已包含了所有数据，所以不需要扫描。

> 因此，`build_memory_index`（data file 扫描）中不自动建立属性索引。属性索引只通过以下方式构建：
> 1. 从持久化的 index 文件加载（正常关闭后重启）
> 2. 用户通过 API 注册 key 后，后续 CRUD 操作增量维护

如果用户在启动后注册一个 key，但之前已有大量数据，需要提供**重建**机制：

- POST `/indices/vertex/properties/:key/rebuild` — 扫描所有顶点，为指定 key 建立索引

### 步骤 5: 重建 API

```rust
POST /indices/vertex/properties/:key/rebuild
POST /indices/edge/properties/:key/rebuild
```

实现：遍历 `mi.vertex_id` 或 `mi.edge_id`，读取 payload，提取属性值，写入索引。这个过程可能需要时间（全量扫描），但只在首次创建索引时执行一次。

### 步骤 6: Python SDK

**文件**: `sdk/python/bionic_graph/client.py`

添加 12 个 Client 方法（vertex 6 个 + edge 6 个）。

### 步骤 7: Python CLI

**文件**: `sdk/python/bionic_graph/cli.py`

在 `index` 主题下添加 `vertex-property` 和 `edge-property` 两个子命令组，每个支持 `create`, `delete`, `delete-batch`, `list`, `query`, `query-batch` 动作。

### 步骤 8: CLI Mock 测试

**文件**: `sdk/python/tests/test_cli.py`

为新的 12 个命令添加 mock 测试用例。

### 步骤 9: SDK Mock 测试

**文件**: `sdk/python/tests/test_client.py`

为新的 12 个 Client 方法添加单元测试。

## 数据结构

```rust
/// 属性索引的内部表示
pub type PropertyIndex = HashMap<String, BTreeMap<String, Vec<MetaPointer>>>;

impl PropertyIndex {
    fn insert(&mut self, key: &str, value: &str, ptr: MetaPointer);
    fn remove(&mut self, key: &str, value: &str, ptr: &MetaPointer);
    fn remove_key(&mut self, key: &str);
    fn get(&self, key: &str, value: &str) -> Option<&[MetaPointer]>;
    fn has_key(&self, key: &str) -> bool;
    fn keys(&self) -> Vec<String>;
}
```

此类型可在 `memory_index.rs` 中定义为公共类型，供 `vertex_properties` 和 `edge_properties` 字段使用，也可在 builder 和 CRUD 中复用。

## 文件清单

| 文件 | 变更类型 | 说明 |
|------|----------|------|
| `src/storage/memory_index.rs` | 修改 | 新增字段、方法、持久化 |
| `src/gremlin/indices.rs` | 新建 | 12 个 REST handler |
| `src/gremlin/mod.rs` | 修改 | 注册 12 条路由 |
| `src/graph/crud.rs` | 修改 | create/update/delete 时维护索引 |
| `src/graph/locked.rs` | 可能修改 | 暴露属性索引相关的 locked 包装 |
| `sdk/python/bionic_graph/client.py` | 修改 | 新增 12 个方法 |
| `sdk/python/bionic_graph/cli.py` | 修改 | 新增 index 子命令 |
| `sdk/python/bionic_graph/models.py` | 修改 | 新增请求/响应模型 |
| `sdk/python/tests/test_cli.py` | 修改 | mock 测试用例 |
| `sdk/python/tests/test_client.py` | 修改 | SDK 单元测试 |
