# Plan 301: Social Activities Knowledge Graph — Python CLI Pipeline

> **目标**: 在 `examples/social_activities/` 下实现一套 Python 命令行工具，利用 Bionic-Graph Python SDK 和大模型 API，构建并驱动一个社交活动知识图谱的三阶段生命周期：加载社交活动 → 规划活动 → 模拟执行活动。

---

## 项目结构

```
examples/social_activities/
├── social_activities.md     # 原始社交活动文档（已填充）
├── __init__.py              # 包标识
├── cli.py                   # 主入口：load / plan / act 三个子命令
├── llm.py                   # LLM 调用封装（与 self_awareness 相同模式）
├── prompts.py               # 社交活动相关的 prompt 模板
├── graph_utils.py           # 图谱操作工具函数
└── log/                     # [生成] 时间戳输出文件
    ├── social_activities.json
    ├── plan_activities_<timestamp>.json
    └── exec_activities_<timestamp>.json
```

## 整体数据流

```
social_activities.md
    │
    ▼  [load] LLM 提取
log/social_activities.json ────► Bionic-Graph 图库 (graph="social-graph")
    │                                │
    │                                ├── 10+ Person 顶点（角色）
    │                                ├── 30+ Activity 顶点（聚餐/骑行/出差...）
    │                                ├── Location 顶点（成都/太古里/青城山...）
    │                                └── Relation 边（participates_in/friend_of/married_to...）
    │
    ▼  [plan] 从图库搜索+LLM
log/plan_activities_<ts>.json ──► 更新图库
    │                                │
    │                                ├── Plan 顶点（社交活动规划）
    │                                └── has_plan 边（角色 → 计划）
    │
    ▼  [act] 从图库搜索plan+LLM模拟
log/exec_activities_<ts>.json ──► 更新图库
                                     ├── Activity 顶点（模拟执行的活动实例）
                                     ├── has_activity 边（plan → activity）
                                     └── 更新计划状态
```

---

## 阶段划分

### Phase 1: 基础设施

**Step 1.1: 创建文件骨架**
- `__init__.py` — 空包标识
- `llm.py` — 封装对 Bionic-Graph MaaS proxy 的调用（`call_llm` / `call_llm_json`），与 self_awareness 版本相同
- `prompts.py` — 社交活动专用的 prompt 模板
- `graph_utils.py` — 图操作工具函数（与 self_awareness 模式相同，适配多角色场景）
- `cli.py` — 主入口

**Step 1.2: LLM 封装 (`llm.py`)**
- `call_llm(system_prompt, user_prompt, model, client, max_retries, timeout) -> str`
- `call_llm_json(...) -> dict` — 自动提取 ```json 代码块并解析
- 重试 2 次，超时 120s

**Step 1.3: Prompt 模板 (`prompts.py`)**
- `EXTRACT_SYSTEM_PROMPT` / `EXTRACT_USER_PROMPT_TEMPLATE` — 从 Markdown 提取社交活动实体和关系
- `PLAN_SYSTEM_PROMPT` / `PLAN_USER_PROMPT_TEMPLATE` — 基于图谱状态生成社交活动规划
- `EXEC_SYSTEM_PROMPT` / `EXEC_USER_PROMPT_TEMPLATE` — 模拟社交活动执行

### Phase 2: load 子命令

**Step 2.1: Markdown → JSON 提取**
1. 读取 `social_activities.md`
2. 调用 LLM 解析文档，提取 entities 和 relations
3. 实体包括：人物（10个角色）、活动（聚餐/骑行/出差等）、地点（成都/太古里等）
4. 关系包括：participates_in、friend_of、married_to、dating、organizes 等
5. 保存到 `log/social_activities.json`

**LLM 输出 JSON 字段与 SDK 参数对应:**
```
entities[i] = {name, labels, keywords, properties}  ← create_vertex(**entity)
relations[i] = {source, target, name, labels, keywords, strength, properties}  ← source/target 为字符串 name
```

**提取粒度要求:**
| 维度 | 最少数量 |
|------|---------|
| 人物（person/character） | 10 |
| 社交活动（social_activity） | 15+ |
| 地点（location） | 8+ |
| 关系类型 | 10+ |
| 总关系数 | 80+ |

**Step 2.2: 加载 JSON 到图库**
1. 创建图（默认 `social-graph`）
2. 去重加载顶点（以 name 为唯一标识）
3. 创建边（source/target name → int ID 映射）

### Phase 3: plan 子命令

**Step 3.1: 搜索图谱**
- 搜索 `"activity plan"` 获取当前活动状态
- 回退方案：Gremlin V() 全量扫描
- 按 priority + rank 排序

**Step 3.2: LLM 生成社交活动规划**
- 输入：当前图谱状态（已有活动、角色关系、个人计划）
- 输出：新的社交活动规划（entities + relations）
- 计划 labels 包含 `["plan", "social_activity", "<category>"]`
- 属性包含 category、timeframe、priority(1-10)、status="pending"

**Step 3.3: 加载到图库**
- 去重加载规划实体和 has_plan 关系

### Phase 4: act 子命令

**Step 4.1: 获取计划并按 priority 排序**
- 搜索 `"activity plan"`，按 priority 降序排列
- 选择 top N（默认 3）

**Step 4.2: LLM 模拟社交活动执行**
- 输入：计划详情 + 人物背景
- 输出：活动实体（execution 叙述、result、time_spent_hours、takeaway、progress_pct）
- 输出 has_activity 关系 + plan_updates

**Step 4.3: 更新图库**
- 创建活动顶点 + has_activity 边
- 更新计划状态

## CLI 入口

```
Usage:
  python cli.py <command> [OPTIONS]

Commands:
  load   Load social activities from MD into the graph
           --md PATH             Markdown path (default: social_activities.md)
           --graph TEXT           Graph name (default: social-graph)
           --model TEXT           LLM model name
           --output PATH          Output path (default: log/social_activities.json)
           --base-url TEXT        Backend URL (default: http://127.0.0.1:8080)
           --force                Force re-extract

  plan   Generate new social activity plans
           --graph TEXT           Graph name (default: social-graph)
           --model TEXT           LLM model name
           --base-url TEXT        Backend URL
           --output PATH          Output path (default: log/plan_activities_<ts>.json)

  act    Simulate social activity execution
           --count N             Activities to simulate (default: 3)
           --graph TEXT           Graph name (default: social-graph)
           --model TEXT           LLM model name
           --base-url TEXT        Backend URL
           --output PATH          Output path (default: log/exec_activities_<ts>.json)
```

## 关键设计决策

1. **字段即 SDK 参数**: entities/relations 的字段名与 `create_vertex()` / `create_edge()` 参数名完全一致
2. **去重方式**: 以顶点 name 为唯一标识
3. **无单一根顶点**: 社交图谱有多个核心人物，而非单个 self 根顶点
4. **图谱 Schema**:
   - 人物: labels=`["person", "character"]`
   - 活动: labels=`["social_activity", "<category>"]`
   - 计划: labels=`["plan", "social_activity", "<category>"]`
   - 活动执行: labels=`["activity_execution", "<category>"]`
5. **日志持久化**: 所有 LLM 输出保存到 `log/` 目录，不删除
