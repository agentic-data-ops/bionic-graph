# Plan 300: Self-Awareness Knowledge Graph — Python CLI Pipeline

> **目标**: 在 `examples/self_awareness/` 下实现一套 Python 命令行工具，利用 Bionic-Graph Python SDK 和大模型 API，构建并驱动一个自我意识知识图谱的三阶段生命周期：加载自我认知 → 反思规划 → 模拟执行。

---

> **代码规范**: 所有 `.py` 文件的代码注释、CLI help 文本、log/print 输出必须使用**英文**。中文仅用于本计划文档和 prompt 模板中的说明性文字。

## 项目结构

```
examples/self_awareness/
├── self_soul.md             # 原始自我认知文档（已填充）
├── self_soul.json           # [生成] LLM 提取的图谱数据（entities + relations）
├── cli.py                   # 主入口：load / plan / act 三个子命令
├── llm.py                   # LLM 调用封装（prompt + parse）
├── graph_utils.py           # 图谱操作工具函数（去重、搜索、批量创建）
└── prompts.py               # 所有 LLM prompt 模板
```

---

## 整体数据流

```
self_soul.md
    │
    ▼  [load] LLM 提取
self_soul.json  ──────► Bionic-Graph 图库 (graph="self-awareness")
    │                       │
    │                       ├── Vertex "self"（根顶点）
    │                       ├── Entity 顶点（知识/兴趣/技能/任务/社交...）
    │                       └── Relation 边（连接 self 与各实体）
    │
    ▼  [plan] 从图库搜索+LLM
next_plan.json ────────► 更新图库
    │                       │
    │                       ├── Plan 顶点（学习/运动/工作/爱好/社交计划）
    │                       └── has_plan 边（self → 各计划）
    │
    ▼  [act] 从图库搜索plan+LLM模拟
activity_log.json ────────► 更新图库
                            ├── Activity 顶点（执行的活动实例）
                            ├── has_activity 边（plan → activity）
                            └── 更新计划状态（进行中/已完成）
```

---

## 阶段划分

### Phase 1: 基础设施 — 目录结构 + LLM 封装

**Step 1.1: 创建目录和模块骨架**
- 创建 `examples/self_awareness/` 下的 Python 文件骨架
- `llm.py` — 封装对 Bionic-Graph MaaS proxy 的调用（`POST /proxy/openai/v1/chat/completions`）
- `prompts.py` — 存放所有 prompt 模板
- `graph_utils.py` — 存放图操作工具函数

**Step 1.2: LLM 封装模块 (`llm.py`)**
- 使用 `bionic_graph.Client` 的 `chat_completion()` 方法调用 LLM
- 支持 `call_llm(system_prompt, user_prompt, model=None) -> str` 返回纯文本
- 支持 `call_llm_json(system_prompt, user_prompt, model=None) -> dict` 返回解析后的 JSON
- 错误处理：重试 2 次，超时 120s

**Step 1.3: Prompt 模块 (`prompts.py`)**
- `EXTRACT_SYSTEM_PROMPT` — 告知 LLM 从 Markdown 文档中提取实体和关系
- `EXTRACT_USER_PROMPT_TEMPLATE` — 包含格式化指令和 JSON 输出 schema
- `PLAN_SYSTEM_PROMPT` — 告知 LLM 基于当前知识图谱内容生成下一步规划
- `PLAN_USER_PROMPT_TEMPLATE` — 包含图谱搜索结果和规划指令
- `ACT_SYSTEM_PROMPT` — 告知 LLM 模拟活动执行过程
- `ACT_USER_PROMPT_TEMPLATE` — 包含计划详情和执行指令

### Phase 2: load 子命令 — 文档解析 → 图谱加载

**Step 2.1: Markdown → JSON 提取 (`cli.py load`)**
1. 读取 `self_soul.md`
2. 调用 LLM（`call_llm_json`）解析文档，提取实体（entities）和关系（relations）
3. **重要**: LLM 输出的 JSON 中，描述"自我"的根顶点 `name` 必须为 `"self"`
4. 验证输出 JSON 完整性
5. 将提取结果保存到 `self_soul.json`

**LLM 输出 JSON schema（字段与 SDK 参数一一对应）:**

```json
{
  "entities": [
    {
      "name": "self",
      "labels": ["person", "self"],
      "keywords": ["introspective", "researcher", "engineer"],
      "properties": {
        "full_name": "Alex Chen",
        "age": 28,
        "occupation": "Research Engineer",
        "nationality": "Canadian",
        "education": "M.Sc. in Cognitive Science",
        "residence": "Vancouver, BC, Canada"
      }
    },
    {
      "name": "Vancouver",
      "labels": ["location", "city"],
      "keywords": ["home", "residence"],
      "properties": {
        "country": "Canada",
        "type": "city"
      }
    },
    {
      "name": "GraphRAG Paper",
      "labels": ["task", "project", "research"],
      "keywords": ["publication", "knowledge-graph", "LLM"],
      "properties": {
        "priority": "high",
        "deadline": "2 weeks",
        "status": "in-progress"
      }
    }
  ],
  "relations": [
    {
      "source": "self",
      "target": "Vancouver",
      "name": "resides_in",
      "labels": ["location", "residence"],
      "strength": 1.0,
      "properties": {}
    },
    {
      "source": "self",
      "target": "GraphRAG Paper",
      "name": "working_on",
      "labels": ["task", "current"],
      "strength": 0.9,
      "properties": {}
    }
  ]
}
```

> **注意**: `entities[i]` 的字段 = `client.create_vertex()` 签名（`name, labels, keywords, properties`）。`relations[i]` 的字段 = `client.create_edge()` 签名（`source, target, name, labels, keywords, strength, properties`），其中 `source`/`target` 在 JSON 中存顶点 name（字符串），加载时自动解析为 int ID。

**提取粒度要求（LLM 需提取以下维度的实体，每类≥3个）：**
| 维度 | 实体示例 | 最少数量 |
|------|---------|---------|
| 身份 | self, 教育背景, 职业, 居住地 | 4 |
| 身体特征 | 身高/体重/血型/健康状况... | 3+ |
| 性格特质 | INTP, 好奇心, 内向, 完美主义... | 5+ |
| 价值观 | Truth, Growth, Kindness, Autonomy... | 3+ |
| 动机/驱动力 | 理解心智, 掌握, 贡献... | 3+ |
| 兴趣/爱好 | Running, Chess, Cooking, Photography... | 5+ |
| 技能 | Rust, Python, Technical Writing, Research... | 5+ |
| 任务 | GraphRAG Paper, Bionic-Graph v0.8... | 3+ |
| 故事 | Open-Source Epiphany, The Talk That Went Wrong... | 3+ |
| 社会关系 | Maya Patel, James Okonkwo, Dr. Anika Sharma... | 5+ |
| 社交活动 | Rust Meetup, Reading Group, Running Club... | 3+ |

**Step 2.2: 加载 JSON 到图库 (`graph_utils.py`)**
1. 客户端连接：`Client(base_url=...)`
2. 创建图：`POST /graphs` → 图名 `"self-awareness"`，如果已存在则复用
3. 去重加载顶点：
   - 先搜索图中所有顶点（`/gremlin` with `V()` 或分批搜索）
   - 对 `self_soul.json` 中每个实体，检查 `name` 是否已存在
   - 去重逻辑：**以 `name` 为唯一标识**，若同名顶点已存在则跳过，否则创建
   - 查找方式：使用 Gremlin `has("name", entity_name)` 或遍历所有顶点匹配
   - 注意：顶层根顶点 `"self"` 必须存在且只有一个
4. 创建边：
   - 将 JSON 中的 source/target 名称映射为创建后的顶点 ID
   - 维护一个 `name → id` 映射表
   - 对每条 relation，使用 source/target 的 name 查找映射，创建边
5. 反馈进度（终端打印：创建了 N 个顶点，M 条边）

**去重加载的伪代码（直接解包 entity/relation，与 SDK 签名一致）:**

```python
def load_json_to_graph(client, graph_name, data):
    """Load entities/relations JSON into the graph.
    
    Each entity dict is unpacked directly as client.create_vertex(**entity).
    Each relation dict uses string source/target (vertex names), resolved to int IDs.
    """
    # 1. Ensure graph exists
    ensure_graph(client, graph_name)
    
    # 2. Build name → id mapping from existing vertices
    name_to_id = get_all_vertex_names(client, graph_name)
    
    # 3. Create vertices with dedup by name
    #    entity keys match create_vertex() params exactly — direct unpack
    for entity in data["entities"]:
        if entity["name"] in name_to_id:
            print(f"  ⏭️  Skip '{entity['name']}' — already exists (id={name_to_id[entity['name']]})")
        else:
            resp = client.create_vertex(**entity, graph=graph_name)
            name_to_id[entity["name"]] = resp.id
            print(f"  ✅ Created '{entity['name']}' (id={resp.id})")

    # 4. Create edges, resolving source/target names to IDs
    for rel in data["relations"]:
        src_id = name_to_id.get(rel["source"])
        tgt_id = name_to_id.get(rel["target"])
        if src_id is None or tgt_id is None:
            print(f"  ⚠️  Skip relation '{rel['name']}': source/target not found")
            continue
        # Unpack all fields except source/target, pass resolved int IDs
        edge_kwargs = {k: v for k, v in rel.items() if k not in ("source", "target")}
        resp = client.create_edge(source=src_id, target=tgt_id, **edge_kwargs, graph=graph_name)
        print(f"  🔗 Created edge '{rel['name']}' {rel['source']}→{rel['target']} (id={resp.id})")
```

### Phase 3: plan 子命令 — 自我反思 + 规划生成

**Step 3.1: 搜索图谱中的兴趣和任务**
1. 执行 Gremlin 查询：
   ```
   [{"step": "search", "text": "my interests and tasks"}]
   ```
   或通过 `/search?text=interests+tasks&mode=greedy`
2. 获取与 `self` 顶点关联的兴趣、任务、技能等实体
3. 将搜索结果整理为结构化文本供 LLM 使用

**Step 3.2: LLM 生成下一步规划**
1. 调 LLM（`call_llm_json`），输入图谱搜索到的兴趣/任务/技能信息
2. LLM 输出未来一段时间的规划，包含以下维度（每个维度≥2项）：
   - **学习**（Learning）— 技术/语言/学术
   - **运动**（Sports）— 跑步/瑜伽/健身
   - **工作**（Work）— 研究/编码/会议
   - **爱好**（Hobbies）— 摄影/游戏/烹饪
   - **社交**（Social）— 朋友/家人/社区
3. 输出格式为规划图谱数据（entities + relations）

**LLM 返回的规划 JSON schema（字段同样与 SDK 参数一一对应）:**

```json
{
  "entities": [
    {
      "name": "Complete GraphRAG Paper Revision",
      "labels": ["plan", "work", "high-priority"],
      "keywords": ["paper", "revision", "research"],
      "properties": {
        "dimension": "work",
        "timeframe": "next 2 weeks",
        "priority": 9,
        "status": "pending"
      }
    },
    {
      "name": "Train for Half-Marathon",
      "labels": ["plan", "sports", "medium-priority"],
      "keywords": ["running", "fitness", "training"],
      "properties": {
        "dimension": "sports",
        "timeframe": "next 3 months",
        "priority": 7,
        "status": "pending"
      }
    }
  ],
  "relations": [
    {
      "source": "self",
      "target": "Complete GraphRAG Paper Revision",
      "name": "has_plan",
      "labels": ["ownership"],
      "strength": 1.0,
      "properties": {}
    }
  ]
}
```

> **注意**: plan 阶段和 load 阶段复用**完全相同**的 `entities` + `relations` 顶层 key 名，以及相同的字段签名，因此 `load_json_to_graph()` 可被两个阶段共用。

**Step 3.3: 将规划加载到图库**
1. 使用与 Step 2.2 相同的去重逻辑（按 name 去重）
2. 创建 Plan 顶点
3. 创建 `self → plan` 的 `has_plan` 边

### Phase 4: act 子命令 — 活动选择 → 模拟执行 → 图谱更新

**Step 4.1: 从图库获取计划并按 Rank 排序**
1. 搜索 `self` 顶点关联的所有 `has_plan` 边，获取计划列表
2. 对每个计划顶点，获取其 `rank`（访问频率/重要性）
3. 按 rank 降序排列
4. 选择优先级最高的 N 个活动（N 可通过 `--count` 参数指定，默认 3）

**Step 4.2: LLM 模拟活动执行**
1. 对每个选中的计划，调用 LLM 模拟执行过程
2. 输入：计划详情（name, labels, properties, rank）
3. LLM 输出活动模拟日志，包含：
   - 活动名称
   - 执行描述（做了什么、过程如何）
   - 执行结果（成功/部分成功/失败）
   - 消耗的时间
   - 心得体会
   - 更新的计划状态（completed / in-progress / blocked）

**Step 4.3: 将活动执行结果更新到图库**
1. 创建 `Activity` 顶点，记录执行详情
2. 创建 `plan → activity` 的 `has_activity` 边
3. 更新 plan 顶点的 `status` 属性（pending → in-progress / completed）
4. 可选：更新 `self` 顶点的状态或添加 `experience` 属性

**LLM 模拟输出 schema（同样使用 entities + relations 格式，与 SDK 参数一致）:**

```json
{
  "entities": [
    {
      "name": "Work on GraphRAG Paper Revision",
      "labels": ["activity", "work"],
      "keywords": ["paper", "ablation", "experiment"],
      "properties": {
        "execution": "Spent 4 hours running ablation experiments on chunk size vs retrieval quality. Review #2's concerns about scalability addressed by adding 3 new benchmark configurations. Wrote methodology section for ablation study.",
        "result": "success",
        "time_spent_hours": 4.0,
        "takeaway": "Smaller chunks (256 tokens) improve retrieval precision by 12% but increase index size by 40%. Need to find optimal trade-off.",
        "progress_pct": 65
      }
    }
  ],
  "relations": [
    {
      "source": "Complete GraphRAG Paper Revision",
      "target": "Work on GraphRAG Paper Revision",
      "name": "has_activity",
      "labels": ["execution"],
      "strength": 1.0,
      "properties": {}
    }
  ],
  "plan_updates": [
    {
      "name": "Complete GraphRAG Paper Revision",
      "properties": {
        "status": "in-progress",
        "progress_pct": 65
      }
    }
  ]
}
```

> `plan_updates` 是额外字段，用于标记需要更新属性的已有 plan 顶点（通过 name 查找，用 `client.update_vertex()` 更新 properties）。

### Phase 5: CLI 入口整合

**Step 5.1: 命令行参数设计**

```
Usage:
  python cli.py <command> [OPTIONS]

Commands:
  load   Load self-awareness from a Markdown document into the graph
           Options:
             --md PATH             Markdown document path (default: self_soul.md)
             --graph TEXT           Graph name (default: self-awareness)
             --model TEXT           LLM model name (default: settings default_model)
             --output PATH          Output JSON file path (default: self_soul.json)
             --base-url TEXT        Backend URL (default: http://127.0.0.1:8080)
             --force                Force re-extract and overwrite existing vertices
  
  plan   Reflect on current graph state and generate next-phase plans
           Options:
             --graph TEXT           Graph name (default: self-awareness)
             --model TEXT           LLM model name
             --base-url TEXT        Backend URL (default: http://127.0.0.1:8080)
             --output PATH          Output plan JSON file path (default: next_plan.json)
  
  act    Execute top-N activities sorted by rank
           Options:
             --count N             Number of activities to simulate (default: 3)
             --graph TEXT           Graph name (default: self-awareness)
             --model TEXT           LLM model name
             --base-url TEXT        Backend URL (default: http://127.0.0.1:8080)
             --output PATH          Output activity log JSON path (default: activity_log.json)

Global options:
  --help           Show this help message
```

**Step 5.2: CLI 入口实现 (`cli.py`)**

```python
#!/usr/bin/env python3
"""Self-Awareness Knowledge Graph CLI — load / plan / act"""

import argparse
import sys

def main():
    parser = argparse.ArgumentParser(description="Self-Awareness Knowledge Graph CLI")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # load subcommand
    load_parser = subparsers.add_parser("load", help="Load self-awareness from MD into graph")
    load_parser.add_argument("--md", default="self_soul.md")
    load_parser.add_argument("--graph", default="self-awareness")
    load_parser.add_argument("--model", default=None)
    load_parser.add_argument("--output", default="self_soul.json")
    load_parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    load_parser.add_argument("--force", action="store_true")

    # plan subcommand
    plan_parser = subparsers.add_parser("plan", help="Reflect on graph state and generate next-phase plans")
    plan_parser.add_argument("--graph", default="self-awareness")
    plan_parser.add_argument("--model", default=None)
    plan_parser.add_argument("--output", default="next_plan.json")
    plan_parser.add_argument("--base-url", default="http://127.0.0.1:8080")

    # act subcommand
    act_parser = subparsers.add_parser("act", help="Execute top-N activities sorted by rank")
    act_parser.add_argument("--count", type=int, default=3)
    act_parser.add_argument("--graph", default="self-awareness")
    act_parser.add_argument("--model", default=None)
    act_parser.add_argument("--output", default="activity_log.json")
    act_parser.add_argument("--base-url", default="http://127.0.0.1:8080")

    args = parser.parse_args()

    if args.command == "load":
        from load import run_load
        run_load(args)
    elif args.command == "plan":
        from plan import run_plan
        run_plan(args)
    elif args.command == "act":
        from act import run_act
        run_act(args)


if __name__ == "__main__":
    main()
```

### Phase 6: 测试与验证

**Step 6.1: 单元测试**
- `test_llm.py` — 测试 `call_llm` 和 `call_llm_json` 的 mock 调用
- `test_graph_utils.py` — 测试去重逻辑、name→id 映射

**Step 6.2: 端到端运行验证**
1. 启动 Bionic-Graph 后端（`cargo run`）
2. 运行 `python cli.py load` — 验证 LLM 提取结果保存到 `self_soul.json`
3. 验证图谱：`bgcli vertex search --name self --graph self-awareness`
4. 运行 `python cli.py plan` — 验证生成规划并写入图谱
5. 运行 `python cli.py act --count 2` — 验证活动模拟和图谱更新

---

## 实现顺序

| 步骤 | 文件 | 内容 |
|------|------|------|
| 1 | `examples/self_awareness/__init__.py` | 空包标识 |
| 2 | `examples/self_awareness/llm.py` | LLM 调用封装 |
| 3 | `examples/self_awareness/prompts.py` | Prompt 模板 |
| 4 | `examples/self_awareness/graph_utils.py` | 图操作工具 |
| 5 | `examples/self_awareness/cli.py` | 主入口 CLI |
| 6 | (cli.py 内) `load` 子命令实现 | Phase 2 |
| 7 | (cli.py 内) `plan` 子命令实现 | Phase 3 |
| 8 | (cli.py 内) `act` 子命令实现 | Phase 4 |
| 9 | `examples/self_awareness/tests/` | 测试 |

## 关键设计决策

1. **字段即 SDK 参数**: LLM 输出的 JSON 字段名与 `client.create_vertex()` / `client.create_edge()` 参数名完全一致。`entities[i]` = `{name, labels, keywords, properties}`；`relations[i]` = `{source, target, name, labels, keywords, strength, properties}`。加载代码直接用 `**entity` 和 `**rel` 解包调用 SDK。

2. **去重方式**: 以顶点 `name` 为唯一标识。每次加载前先查询图中所有顶点的 name→id 映射表，同名跳过。

3. **根顶点名称**: 必须为 `"self"`，这是 LLM 提取和后续查询的约定锚点。

4. **LLM 调用方式**: 使用 Bionic-Graph 内置的 MaaS proxy（`/proxy/openai/v1/chat/completions`），而非直接调用外部 API。这保证了与后端配置的 LLM 提供商一致。

5. **Rank 的作用**: 利用 Bionic-Graph 的内置 rank 机制（自动递增）作为"重要性/活跃度"信号，`act` 命令按 rank 降序选择活动。

6. **图谱 schema 约定**:
   - `"self"` 顶点: labels=`["person", "self"]`
   - 计划顶点: labels 包含 `"plan"` + 维度标签（如 `"work"`, `"sports"`）
   - 活动顶点: labels 包含 `"activity"`
   - 边类型: `has_plan`（self→plan）, `has_activity`（plan→activity）

7. **三阶段复用同一加载函数**: `load` / `plan` / `act` 三阶段的 JSON 均使用 `{entities, relations}`（`act` 额外有 `plan_updates`）顶层结构，因此 `load_json_to_graph()` 函数可被三个阶段复用，`plan_updates` 由 `act` 阶段单独处理。
