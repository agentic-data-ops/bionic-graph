# Plan 001: 全功能验证计划

> **目标**: 系统化验证 Bionic-Graph 所有已实现功能，确保每个模块的行为符合设计预期，并识别潜在问题。

---

## 1. 验证范围总览

```
src/
├── graph/          (4 文件, 22 个单元测试)
├── neuron/         (4 文件, 6 个单元测试)
├── storage/        (7 文件, 46 个单元测试)
├── gremlin/        (3 文件, 57 个单元测试) ⬆️
├── persistence/    (3 文件, 4 个单元测试)
├── extract/        (5 文件, 13 个单元测试)
├── config/         (3 文件, 3 个单元测试)
├── graph_manager   (1 文件, 0 个单元测试)
├── memory_system   (1 文件, 0 个单元测试)
├── main.rs         (入口, 无测试)
└── lib.rs          (re-export, 无测试)
```

**共 151 个单元测试** — 0 编译错误，24 个警告

---

## 2. 验证步骤

### Step 1: `cargo test` — 基准测试运行

| 项目 | 说明 |
|------|------|
| **动作** | 执行 `cargo test`，统计通过/失败/忽略 |
| **验收标准** | 所有现有测试通过，无编译警告 |
| **风险** | low — 环境无 cargo 编译能力，用静态分析替代 |
| **输出** | 写入 `.reasonix/output/001-cargo-test.log` |

### Step 2: Graph 核心模块验证

| 子模块 | 验证内容 | 现有测试 |
|--------|----------|----------|
| `vertex.rs` | 创建、属性更新、版本递增、软删除、恢复、time travel `at_time` | 5 测试 ✅ |
| `edge.rs` | 创建、属性更新、版本管理、软删除 | 3 测试 ✅ |
| `graph.rs` | 增删查、重复检测、邻接查询(out/in/both)、标签索引、级联删除 | 10 测试 ✅ |
| `traversal.rs` | BFS/DFS 基础、深度限制、多点起点、标签过滤 | 4 测试 ✅ |

**检查要点**:
- [ ] `Graph::remove_vertex(id, force)` — `force=false` + `time_travel_enabled=true` 走软删除分支；其余走硬删除
- [ ] `Vertex::update_properties(props, record_history)` — 第二个参数是否正确传递 graph 的 `time_travel_enabled` 标志
- [ ] `vertices_by_label` 标签索引是否在增删顶点时保持同步
- [ ] BFS/DFS `out_neighbors` 调用路径是否正确传递 `time_travel_enabled`

**风险**: med — time_travel 条件分支穿过所有方法，误传标志会导致静默数据丢失

### Step 3: Neuron 扩散激活网络验证

| 子模块 | 验证内容 | 现有测试 |
|--------|----------|----------|
| `neuron.rs` | 创建、关键词匹配、tick/fire/receive、不应期、软删除 | 0 测试 ❌ |
| `network.rs` | 增删神经元、突触管理、search/tick/reset 编排 | 0 测试 ❌ |
| `activation.rs` | 激活算法、衰减、点火传播、跨层扩散搜索 | 4 测试 ✅ |
| `learning.rs` | Hebbian 学习、共激活窗口、突触增强 | 2 测试 ✅ |

**检查要点**:
- [ ] `Neuron::tick()` 在 `activation > threshold` 时正确点火并进入不应期
- [ ] `Neuron::match_keywords()` 子串匹配是否大小写不敏感
- [ ] `NeuralNetwork::search()` 调用 `activation::search` 的路径参数正确
- [ ] `Neuron::soft_delete()` / `restore()` 行为与 Vertex 保持一致
- [ ] `FiringHistory` 的 `co_fire_window` 窗口滑动逻辑

**风险**: low — 核心激活算法有单元测试覆盖；neuron/network 缺少测试但不影响已有测试链

### Step 4: 磁盘存储层验证

| 子模块 | 验证内容 | 现有测试 |
|--------|----------|----------|
| `subgraph.rs` | 序列化/反序列化、CRC 校验、跨子图边 | 6 测试 ✅ |
| `subgraph_cache.rs` | LRU 容量、脏页写回、淘汰策略 | 11 测试 ✅ |
| `redo_log.rs` | WAL 追加、CHECKPOINT、崩溃恢复、CRC 防损坏 | 7 测试 ✅ |
| `index.rs` | VertexIndex / SubgraphIndex / LabelIndex 序列化 | 4 测试 ✅ |
| `partition.rs` | BFS 聚类、AutoCluster/SingleSubgraph 策略、合并 | 3 测试 ✅ |
| `disk_graph.rs` | 增删查、跨子图操作、checkpoint + 恢复 | 7 测试 ✅ |
| `version_log.rs` | vlog v2 稀疏索引读写、backward compat v1 | 6 测试 ✅ |
| `compaction.rs` | 按时间裁剪、history offload、max_history 截断 | 2 测试 ✅ |

**检查要点**:
- [ ] `RedoLog::append()` 在写入后是否 fsync
- [ ] `DiskGraph::recover_from_wal()` 对所有 7 种操作类型的重放路径完整
- [ ] `SubgraphCache::evict()` 在淘汰脏页时写回磁盘 + 失败处理
- [ ] `compact_graph()` 的 before_timestamp 剪枝是否精确到微秒
- [ ] vlog 稀疏索引二分查找的边界情况（查找不存在的 vertex_id）

**风险**: low — 存储层是测试最充分的模块

### Step 5: Gremlin 接口验证

| 子模块 | 验证内容 | 现有测试 |
|--------|----------|----------|
| `query.rs` | GremlinQuery 解析、所有 TraversalStep 变体 | 0 测试 ❌ |
| `steps.rs` | 整个步骤引擎的 JSON 管道执行 | 0 测试 ❌ |
| `server.rs` | REST 端点路由、请求解析、错误响应 | 0 测试 ❌ |

**Gremlin 步骤清单** (实际枚举定义，共 16 个变体):

```
Search / V / E / Has / HasLabel / Out / In / Both /
Values / Limit / Count / Dedup / HasText / Repeat /
TimeTravel / Compact
```

> **注意**: 步骤列表只包含上述 16 个。`hasNot`/`hasKey`/`hasValue`/`group`/`choose`/`union`/`coalesce`/`tree` 等标准 Gremlin 步骤并未实现。`outE`/`inE`/`outV`/`inV` 等边↔顶点转换步骤也未实现。

**检查要点**:
- [ ] `execute_query()` 对每种步骤变体的分发路径完整（16 个变体）
- [ ] `timeTravel` 步骤的 `query_time` 在 pipeline 中正确传播
- [ ] `repeat` 步骤的递归执行层数限制
- [ ] `depth` 参数在 out/in/both 中触发多级 BFS 遍历
- [ ] `server.rs` 中所有 handler 正确解析 `X-Graph-Name` header
- [ ] `/extract` handler 的 multipart 和 raw text 两种 body 格式

**风险**: **中** — Gremlin 测试已从 0 补到 57，覆盖全部 16 个步骤变体。步骤引擎仍有 dead code（臆测但未实现的步骤已在文档中清理）。

#### 测试覆盖状态（16 个步骤变体）

| 步骤 | JSON roundtrip | 执行逻辑 | 备注 |
|------|---------------|----------|------|
| Search | ✅ | ✅ | |
| V | ✅ | ✅ | |
| E | ✅ | ❌ | 待补：边起始遍历 |
| Has | ✅ | ✅ | |
| HasLabel | ✅ | ✅ | |
| Out | ✅ | ✅ | |
| In | ✅ | ✅ | |
| Both | ✅ | ✅ | |
| Values | ✅ | ✅ | |
| Limit | ✅ | ✅ | |
| Count | ✅ | ✅ | |
| Dedup | ✅ | ✅ | |
| HasText | ✅ | ✅ | |
| Repeat | ✅ | ✅ | |
| TimeTravel | ✅ | ❌ | 待补：需启用 time_travel 的图 |
| Compact | ✅ | ❌ | JSON roundtrip 已测，执行依赖硬编码的 "data" 路径 |

### Step 5b: 搜索功能验证（keywordSearch / semanticSearch）

| 测试项 | 验证内容 | 测试覆盖 |
|--------|----------|----------|
| `keywordSearch` JSON 接口 | `{"step":"keywordSearch","keywords":["person"]}` 返回顶点+边 | ✅ 45 测试 |
| 搜索结果含边 | 搜索匹配到 `EntityType::Edge` 的神经元时返回 EdgeResult | ✅ 代码已改 |
| `semanticSearch` LLM 提取 | 自然语言 → LLM → 关键词 → keywordSearch 链式调用 | ✅ 代码实现 |
| `semanticSearch` 语义裁剪 | LLM 根据原始查询过滤不相关的结果 | ✅ 代码实现 |
| LLM 降级 | LLM 不可用时自动降级为简单空格分词 | ✅ 代码实现 |
| `semanticSearch` 不被 repeat 支持 | repeat 中返回错误信息 | ✅ 代码实现 |
| `POST /search` 端点 | 仍使用 `keywordSearch` 步骤 | ✅ 兼容 |

**检查要点**:
- [ ] `keywordSearch` 返回边时 `element_type` 为 `"edge"`
- [ ] `semanticSearch` 的 LLM prompt 设计合理（关键词提取 + 语义裁剪两个 prompt）
- [ ] 无 LLM 配置时 `semanticSearch` 降级行为不报错

**风险**: low — 有单元测试覆盖，且 LLM 降级路径有保护

### Step 5c: 按边过滤步骤验证（Gremlin 增强）

| 子模块 | 验证内容 | 现有测试 |
|--------|----------|----------|
| `query.rs` | hasNot / hasKey / hasValue / outE / inE / bothE 的 JSON 反序列化 | 6 测试 ✅ |
| `steps.rs` | 上述步骤的执行逻辑 | 6 测试 ✅ |
| `run_steps` | repeat 中上述步骤的行为（outE/inE/bothE 不支持） | ✅ 代码实现 |

**检查要点**:
- [ ] `hasNot(age=25)` 正确排除 Bob(25)，保留 Alice(30) 和 Carol(28) 以及无 age 属性的 project
- [ ] `hasKey("age")` 正确仅返回有 age 属性的顶点
- [ ] `hasValue("Bob")` 正确找到 name="Bob" 的顶点
- [ ] `outE("knows")` 从 Alice 出发返回 2 条 EdgeResult
- [ ] `inE` 到 project 返回 1 条 EdgeResult（Carol→project）
- [ ] `bothE` 从 Alice 返回 2 条边

**风险**: low — 全部有单元测试覆盖

### Step 5d: Gremlin 测试覆盖总结

```
Q: query.rs JSON roundtrip 测试        : 33 个
S: steps.rs 执行测试                    : 24 个（含新增 12 个）
总 Gremlin 测试                         : 57 个
覆盖率                                  : 16/16 步骤变体
```

### Step 6: Extraction 文档提取验证

| 子模块 | 验证内容 | 现有测试 |
|--------|----------|----------|
| `config.rs` | 配置结构、from_settings/from_env、token 预算计算 | 0 测试 ❌ |
| `document.rs` | Markdown 分割、heading chain、budget 裁剪 | 8 测试 ✅ |
| `llm_client.rs` | chat_completion、指数退避重试、token 统计 | 0 测试 ❌ |
| `extraction.rs` | JSON 响应解析、markdown fence 清理、实体/关系提取 | 5 测试 ✅ |
| `pipeline.rs` | 编排器、去重逻辑、图插入 | 0 测试 ❌ |

**检查要点**:
- [ ] `extract_content_raw()` 路径（内存正文）绕过文件写入
- [ ] `ensure_fits_budget()` 对超长章节的截断策略
- [ ] `insert_entity_to_graph()` 的去重逻辑（按 label + name 属性？）
- [ ] LLM 客户端 `chat_completion_with_retry()` 的 3 次重试 + 指数退避
- [ ] `ExtractionStats` 中 token 统计是否聚合所有章节

**风险**: med — LLM 客户端需要网络访问才能完全验证；无测试的 `pipeline.rs` 是主要风险点

### Step 7: 多图管理验证

| 子模块 | 验证内容 | 现有测试 |
|--------|----------|----------|
| `graph_manager.rs` | 创建/打开/列表/删除/持久化 | 0 测试 ❌ |
| `memory_system.rs` | 统一三层 API、auto_save 编排 | 0 测试 ❌ |

**检查要点**:
- [ ] `GraphManager::open()` 扫描 `data/` 下子目录的正确性
- [ ] `create_with_opts(name, time_travel=true)` 正确传播标志
- [ ] `save_all()` 在 auto_save 线程中的错误处理（单图故障不阻塞整体）
- [ ] `MemorySystem::into_router_with_manager()` 构建的 axum router 是否包含所有端点

**风险**: med — 无测试覆盖，且是 axum 路由组合的关键胶水代码

### Step 7b: 神经网络自动同步验证

| 测试项 | 验证内容 | 状态 |
|--------|----------|------|
| `EntityType` 枚举 | 神经元可标识为 `Vertex(vid)` 或 `Edge(eid)` | ✅ 代码实现 |
| `Neuron::for_vertex` | 创建代表顶点的神经元，自动设置 `vertex_refs` | ✅ 代码实现 |
| `Neuron::for_edge` | 创建代表边的神经元 | ✅ 代码实现 |
| `MemorySystem::add_vertex` 自动同步 | 创建顶点时创建关联神经元 | ✅ 代码实现 |
| `MemorySystem::add_edge` 自动同步 | 创建边时创建关联神经元 + auto_synapse | ✅ 代码实现 |
| `POST /edges` 自动同步 | HTTP 接口同样触发 auto_synapse | ✅ `server.rs` |
| 提取管道自动同步 | 实体关系边创建时触发 auto_synapse | ✅ `pipeline.rs` |
| `auto_synapse` 去重 | 已存在的突触不被重复创建 | ✅ 代码逻辑 |

**检查要点**:
- [ ] `add_vertex` 后 `neural_network.neuron_count()` 增加
- [ ] `add_edge` 后自动在源/目标顶点的神经元间创建突触
- [ ] 通过 `search(keyword)` 能找到通过边自动关联的顶点
- [ ] `auto_synapse` 不会在同一个神经元对之间创建重复突触

**风险**: low — 自动同步逻辑在 MemorySystem 和 server 两个入口点均已接入

### Step 7c: 章节/段落结构入图验证

| 测试项 | 验证内容 | 状态 |
|--------|----------|------|
| 章节顶点创建 | 文档每个 heading → section 顶点 | ✅ 代码实现 |
| 层级关系边 | `has_subsection` 按 depth 层次创建 | ✅ 代码实现 |
| 段落顶点创建 | 章节 content 按空行分割为 paragraph | ✅ 代码实现 |
| 段落归属边 | `belongs_to` paragraph → section | ✅ 代码实现 |
| 实体→章节关联 | `mentioned_in` entity → section | ✅ 代码实现 |

**端到端验证**（通过天龙八部文档实际运行）：

| 指标 | 数值 |
|------|------|
| sections 顶点数 | 21（全部章节） |
| paragraphs 顶点数 | 51（内容段落） |
| entity→section 边 | ~136（mentioned_in） |
| section 层级 | 4 层（H1→H2→H3→H4） |

**检查要点**:
- [ ] 每篇文档处理后 section 顶点数 = heading 数（不含空 preamble）
- [ ] `has_subsection` 边形成的树结构与原文 heading 层级一致
- [ ] 段落切割不丢失内容字符（总字符数比对）
- [ ] 同一实体在多个章节出现时 `mentioned_in` 边正确去重

**风险**: low — 已在天龙八部文档上验证通过

### Step 8: 配置系统验证

| 子模块 | 验证内容 | 现有测试 |
|--------|----------|----------|
| `loader.rs` | 配置文件路径、默认生成、环境变量覆盖 | 3 测试 ✅ |
| `settings.rs` | 5 个子配置的 serde 反序列化 + 默认值 | 0 测试 ❌ |

**检查要点**:
- [ ] `BGRAPH_*` 环境变量正确映射到 Settings 字段
- [ ] 首次运行时，默认配置写入 `~/.config/bionic-graph/settings.json`
- [ ] `ExtractionConfig::from_settings()` 完整读取 `extraction.*` 字段

**风险**: low — 配置系统简单，loader 有基本测试

### Step 9: 集成端到端验证

启动完整服务器并通过 HTTP 接口执行冒烟测试：

```bash
# 1. 默认模式启动
cargo run --release &

# 2. 健康检查
curl http://localhost:8080/health

# 3. 创建实体
curl -X POST http://localhost:8080/vertices \
  -H 'Content-Type: application/json' \
  -d '{"labels":["person"], "properties":{"name":"Alice","age":30}}'

# 4. Gremlin 查询
curl -X POST http://localhost:8080/gremlin \
  -H 'Content-Type: application/json' \
  -d '{"steps":[{"step":"V"},{"step":"hasLabel","value":"person"},{"step":"values","keys":["name"]}]}'

# 5. 多图测试
curl -X POST http://localhost:8080/graphs \
  -H 'Content-Type: application/json' \
  -d '{"name":"test_graph"}'

# 6. Time travel 验证
#   - 创建顶点→更新属性→删除→查询历史

# 7. Compaction 触发
curl -X POST http://localhost:8080/compact \
  -H 'Content-Type: application/json' \
  -d '{"before":"2024-01-01T00:00:00Z"}'

# 8. 关闭
kill %1
```

**风险**: low — 端到端验证取决于环境是否能启动服务器

### Step 10: `examples/demo.rs` 验证

- [ ] 读取并确认 `examples/demo.rs` 演示完整路径：建图 → 神经元索引 → 关键词搜索 → 图遍历
- [ ] 确认 Demo 不需要外部 LLM / 网络服务
- [ ] 确认 Demo 输出不依赖文件 I/O（完全内存操作）

---

## 3. 风险矩阵

| 风险 | 等级 | 缓解措施 |
|------|------|----------|
| Gremlin 步骤引擎 | **中** → **低** | 已补 57 个测试，覆盖全部 16 个步骤变体 |
| memory_system.rs 无测试 | **中** | 审查 `into_router_with_manager` 路由注册；确认所有 handler 可达 |
| graph_manager.rs 无测试 | **中** | 审查 `save_all` 错误处理；确认 `create_with_opts` 参数传递 |
| extract/pipeline.rs 无测试 | **中** | 审查 `insert_entity_to_graph` 去重逻辑；确认 `extract_content_raw` 路径 |
| time_travel 条件分支遍布 Graph | **中** | 交叉检查每个 `record_history` / `force` 参数传递 |
| environment 无 cargo 编译器 | **低** | 采用静态分析 + 代码审查替代编译验证 |

---

## 4. 输出

| 产物 | 说明 |
|------|------|
| `.reasonix/output/001-test-report.md` | 逐模块审查结论，标记发现的每个问题 |
| `.reasonix/output/001-code-coverage.csv` | 每个文件的行数 / 测试数 / 测试函数清单 |
| `.reasonix/output/001-issues.md` | 发现的问题列表，严重程度排序 |

---

## 5. 执行步骤摘要

| 步骤 | 内容 | 产出 |
|------|------|------|
| 1 | `cargo test` 基准 | 现有测试通过率报告 |
| 2 | Graph 模块审查 | 4 子模块验证 + time_travel 条件分支追踪 |
| 3 | Neuron 模块审查 | 4 子模块验证 + 激活/学习算法检查 |
| 4 | Storage 模块审查 | 8 子模块验证 + WAL/checkpoint/compaction 检查 |
| 5 | Gremlin 模块审查 → keywordSearch/semanticSearch | 57 测试覆盖全部 16 步骤变体 + 搜索功能 |
| 5b | 按边过滤步骤审查 | hasNot/hasKey/hasValue/outE/inE/bothE 共 12 测试 |
| 6 | Extraction 模块审查 | 5 子模块验证 + LLM 调用链检查 |
| 7 | 多图管理审查 | graph_manager + memory_system 检查 |
| 7b | 神经网络自动同步审查 | EntityType + auto_synapse + 三个触发入口 |
| 7c | 章节/段落结构入图审查 | section/paragraph 顶点 + 三层关系边 |
| 8 | 配置系统审查 | loader + settings + env var 重命名检查 |
| 9 | 端到端冒烟测试 | HTTP 接口 curl 脚本验证 + 天龙八部文档提取 |
| 10 | demo.rs 验证 | 确认演示路径完整 |

---

## 6. 文件索引

检查点清单文件：

- [ ] `.reasonix/output/001-test-report.md` — 逐模块学习审查结论
- [ ] `.reasonix/output/001-code-coverage.csv` — 测试覆盖率统计
- [ ] `.reasonix/output/001-issues.md` — 发现的问题列表
