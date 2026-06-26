# Plan 011: DiskGraph 集成 + 增量持久化 + 神经搜索过滤

## 目标
将知识图谱的持久化从全量快照（graph.bin）改为基于 DiskGraph + SubgraphCache 的增量、按需加载模式，并优化神经搜索结果的相关性。

## 完成的工作

### 1. DiskGraph 集成（取代 Graph 全量持久化）
- `GraphHandle` 新增 `disk_graph: Arc<Mutex<DiskGraph>>` 字段
- 移除 `graph: Arc<Mutex<Graph>>` 字段，Gremlin 查询通过 DiskGraph snapshot 加载
- `open_graph()` 使用 `DiskGraph::open()` 替代 `load_or_create()`
- `save_graph_snapshot()` 使用 `disk_graph.checkpoint()`（只写脏子图文件）
- `execute_query_with_llm` 内部自动 snapshot DiskGraph → Graph 供 step 引擎执行

### 2. 子图分区增量 Checkpoint
- 替换全量 `graph.bin` 写入为 `subgraphs/{id}.bin`（BFS 聚类分区）
- 每次 checkpoint 写入时计算 CRC32 哈希，与 `manifest.json` 对比
- 哈希未变 → 跳过写入，实现真正的块级增量持久化
- 启动时读取所有 `subgraphs/*.bin` 重建 Graph

### 3. WAL 多文件轮换（RDBMS 风格）
- `RedologWal::rotate()` — checkpoint → rename `redolog.wal` → `redolog.wal.{seq:04}` → 打开新文件
- 启动时按序 replay 所有 `redolog.wal.*` 归档 + 当前 `redolog.wal`
- 归档以 checkpoint 结尾，replay 自动跳过（无需回放条目）
- `clean_archived(max_keep)` — 保留最近 N 个归档，更老的自动删除
- `file_size()` — 获取 WAL 文件大小用于阈值检查

### 4. 事务一致性（所有操作原子化）
- `add_vertex`: 内存创建 → 原子 write_batch([vertex, neuron]) WAL → 失败回滚
- `add_edge`: 同上
- `update/delete vertex/edge`: 保存旧状态 → 内存修改 → 原子 batch → 失败回滚
- `delete_vertex_handler`: 收集受影响边 + 快照 → 一次性 batch → 失败恢复
- `delete_edge_handler`: neuron mark_deleted 写 WAL（之前缺失）
- 软删除边的神经元 mark_deleted 现在通过 `update_neuron` WAL 记录

### 5. 新增 DiskGraph API
- `get_edge()`, `edge_index`, `vertex_ids()`, `edge_ids()`, `all_edges()`
- `outgoing_edges()`, `incoming_edges()`
- `update_edge()`, `add_edge_with_props()`, `remove_edge()`, `soft_delete_edge()`
- `rebuild_edge_index()` — checkpoint 时重建
- `snapshot()` — 全量快照为内存 Graph（供 Gremlin 引擎）

### 6. 神经搜索结果过滤（跨域污染修复）
- **Layer 1**: 关键词匹配 — `match_keywords()` 找出匹配神经元的 keywords
- **Layer 2**: 激活扩散过滤 — Exact 模式仅直接匹配；Greedy 模式允许扩散激活≥0.3
- **Layer 3**: 顶点级二次过滤 — 顶点 name/keywords/labels 必须包含 query token

### 7. Bug 修复
- **Edge ID 不一致**: `Subgraph::add_edge()` 返回子图本地 ID，`edge_index` 用全局 ID → 创建后覆写为全局 ID
- **`nodesRef is not defined`**: VertexSearchSelect 通过 props 传入 nodesRef

### 8. UI 改进
- 添加边时支持搜索源/目标顶点（VertexSearchSelect 组件）
- 图层搜索框同时支持下拉 + 搜索
- 右上角 LANG 文字替换为地球 SVG 图标
- Light 主题马卡龙蓝暖色调配色

### 9. Rust Warning 清理
- 移除未使用的 `Neuron` 导入
- `_doc_id` 前缀未使用参数
- 移除未使用的 `total_relations` 重复赋值
- 移除未调用的 `fill_vertex_details` 函数

## 文件变更
- `src/graph_manager.rs` — GraphHandle 增加 disk_graph，持久化改为 DiskGraph
- `src/storage/disk_graph.rs` — 新增 ~15 个读写方法 + edge_index + snapshot()
- `src/storage/redolog_wal.rs` — rotate(), clean_archived(), replay_archived(), file_size()
- `src/storage/subgraph.rs` — Subgraph::from_bytes()
- `src/gremlin/steps.rs` — snapshot wrapper, vertex 级二次过滤
- `src/gremlin/server.rs` — handler 全部改为 disk_graph API
- `src/gremlin/mod.rs` — execute_query_graph 导出
- `src/neuron/activation.rs` — direct_set 过滤 + mode 感知收集
- `src/main.rs` — handle.graph → handle.disk_graph
- `src/memory_system.rs` — 兼容 execute_query_graph 路径
- `src/ui/src/index.css` — Light 主题马卡龙配色
- `src/ui/src/components/ChatArea.jsx` — LANG → SVG 图标
- `src/ui/src/components/GraphViewer.jsx` — VertexSearchSelect + 搜索改进

## 验证
- CRUD API 全部通过：创建/查询/更新/删除顶点和边
- 文档解析 + 语义搜索：2 份文档共提取 57 顶点 72 边
- 跨域搜索分离：韩立 → 3 结果，乔峰 → 1 结果，无交集
- `cargo build` 零 warning
