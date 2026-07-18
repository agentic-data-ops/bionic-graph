# Bionic-Graph CLI — `bgcli`

Python 命令行工具，通过 REST API 与 Bionic-Graph 后端交互。

## 安装

```bash
# 从 PyPI 安装（发布后）
pip install bionic-graph-sdk

# 或从源码安装
cd sdk/python
pip install .
```

## 快速开始

```bash
# 设置后端地址（默认 http://127.0.0.1:8080）
export BIONIC_GRAPH_BASE_URL=http://127.0.0.1:8080

# 检查服务状态
bgcli health check

# 列出图谱
bgcli graph list

# 创建顶点
bgcli vertex create --name "王耀辉" --labels '["person"]'

# 创建边
bgcli edge create --source 1 --target 2 --name "宠物" --strength 0.9

# 搜索
bgcli gremlin search --text "王耀辉"

# 执行 Gremlin 查询
bgcli gremlin execute --steps '[{"step":"V","ids":[1]},{"step":"expand"}]'
```

## 命令结构

```
bgcli [全局选项] <主题> <动作> [参数]
```

### 全局选项

| 选项 | 环境变量 | 默认值 | 说明 |
|------|---------|--------|------|
| `--base-url` | `BIONIC_GRAPH_BASE_URL` | `http://127.0.0.1:8080` | 后端地址 |
| `--api-key` | `BIONIC_GRAPH_API_KEY` | — | API 密钥 |
| `--timeout` | — | `30.0` | 请求超时（秒） |
| `--output` | — | `text` | 输出格式：`text` 或 `json` |

### 主题和动作

| 主题 | 动作 | 说明 |
|------|------|------|
| `health` | `check` | 检查服务健康状态 |
| `graph` | `list`, `create`, `set-default`, `delete`, `update-meta`, `get-config`, `set-config` | 图谱生命周期管理 |
| `vertex` | `create`, `update`, `delete`, `get-meta`, `update-meta` | 顶点增删改查 |
| `edge` | `create`, `update`, `delete`, `get-meta`, `update-meta` | 边增删改查 |
| `gremlin` | `execute`, `search` | Gremlin 查询和搜索 |
| `document` | `list`, `create`, `get`, `update`, `delete`, `get-content` | 文档管理 |
| `extract` | `submit`, `get-task`, `list-tasks`, `wait` | 知识提取任务 |
| `settings` | `get-search`, `set-search`, `get-llm`, `set-llm`, `get-rank`, `set-rank`, `get-web-search`, `set-web-search`, `proxy`, `get-tokenizer`, `add-tokenizer-words`, `remove-tokenizer-words` | 全部设置管理 |
| `maas` | `list-models`, `chat` | MaaS 代理 |
| **`chat`** | — | **交互式聊天会话** |

## 交互式聊天

```bash
# 启动聊天（默认开启联网搜索和图谱搜索）
bgcli chat

# 指定选项
bgcli chat --model "DeepSeek/deepseek-v4-flash" \
           --web-search --graph-search \
           --extract-keywords --graph graph0 \
           --search-mode greedy

# 禁用联网搜索
bgcli chat --no-web-search
```

聊天会话中的内部命令：

| 命令 | 说明 |
|------|------|
| `/exit` 或 `/quit` | 退出聊天 |
| `/clear` | 清除对话历史 |
| `/graph <name>` | 切换当前图谱 |
| `/help` | 显示帮助 |

### 聊天工作流程

```
用户输入 → LLM 提取关键词（可选）→ 联网搜索（可选）
→ 图谱搜索（可选）→ 合并上下文 → LLM 回答
```

## 输出格式

默认输出可读文本，支持 `--output json` 输出原始 JSON：

```bash
bgcli --output json health check
bgcli --output json vertex get-meta 1
```

## 完整示例

```bash
# 1. 创建图谱
bgcli graph create my-graph --description "测试图谱"

# 2. 添加顶点
bgcli vertex create --name "Alice" --labels '["person"]' --graph my-graph
bgcli vertex create --name "Bob" --labels '["person"]' --graph my-graph

# 3. 添加关系
bgcli edge create --source 1 --target 2 --name "朋友" --graph my-graph

# 4. 搜索
bgcli gremlin search --text "Alice" --graph my-graph
```

## Python SDK 编程接口

```python
from bionic_graph import Client

client = Client(base_url="http://127.0.0.1:8080")

# 健康检查
print(client.health().status)

# 创建顶点
resp = client.create_vertex("王耀辉", labels=["person"], properties={"age": "40"})
print(f"Vertex ID: {resp.id}")

# Gremlin 查询
result = client.execute_gremlin([{"step": "V", "ids": [1]}, {"step": "expand"}])
for item in result.data:
    print(item)
```
