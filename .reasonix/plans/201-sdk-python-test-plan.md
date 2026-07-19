# Plan 201 — Python SDK CLI 全量测试计划（更新版）

## 目标

覆盖 CLI 支持的全部 11 个主题、45 个动作，通过两种方式：

1. **mock 测试**（`tests/test_cli.py`）— 使用 `click.testing.CliRunner` + `respx` mock 后端，快速验证命令解析、参数传递、退出码和输出格式
2. **真实调用测试**（`tests/test_cli_real.sh`）— 连接运行中的后端服务，真实执行 API 调用，验证端到端功能

## 测试脚本

### Mock 测试

```bash
cd sdk/python && python3 -m pytest tests/test_cli.py -v
```

- 55 个测试用例
- 使用 `respx` mock HTTP 后端
- 不依赖后端服务运行

### 真实调用测试

```bash
cd sdk/python && bash tests/test_cli_real.sh
```

- 57 个测试命令
- 要求后端服务在 `http://127.0.0.1:8080` 运行
- 可通过环境变量 `BASE_URL` 指定其他地址

## 测试覆盖矩阵

### 1. health (1 个动作)

| 动作 | Mock 测试 | 真实调用 | 参数说明 |
|------|-----------|---------|---------|
| `check` | ✅ | ✅ | — |

### 2. graph (7 个动作)

| 动作 | Mock 测试 | 真实调用 | 参数说明 |
|------|-----------|---------|---------|
| `list` | ✅ | ✅ | — |
| `create` | ✅ | ⚠️ 见下方 | `name`(必填), `--description`, `--time-travel` |
| `set-default` | ✅ | ✅ | `name`(必填) |
| `delete` | ✅ | ✅ | `name`(必填), `--force` |
| `update-meta` | ✅ | ✅ | `name`, `--description`, `--time-travel` |
| `get-config` | ✅ | ✅ | `name`(必填) |
| `set-config` | ✅ | ✅ | `name`, `--config`(JSON) |

### 3. vertex (5 个动作)

| 动作 | Mock 测试 | 真实调用 | JSON 参数 |
|------|-----------|---------|-----------|
| `create` | ✅ | ✅ | `--labels`, `--keywords`, `--properties` |
| `update` | ✅ | ✅ | 同上 |
| `delete` | ✅ | ✅ | `--force` |
| `get-meta` | ✅ | ⚠️ 见下方 | — |
| `update-meta` | ✅ | ✅ | `--rank` |

### 4. edge (5 个动作)

| 动作 | Mock 测试 | 真实调用 | JSON 参数 |
|------|-----------|---------|-----------|
| `create` | ✅ | ✅ | `--labels`, `--keywords`, `--properties` |
| `update` | ✅ | ✅ | 同上 |
| `delete` | ✅ | ✅ | `--force` |
| `get-meta` | ✅ | ⚠️ 见下方 | — |
| `update-meta` | ✅ | ✅ | `--rank` |

### 5. gremlin (2 个动作)

| 动作 | Mock 测试 | 真实调用 | JSON 参数 |
|------|-----------|---------|-----------|
| `execute` | ✅ | ✅ | `--steps`(Gremlin 管道) |
| `search` | ✅ | ✅ | `--text`, `--mode`, `--limit` |

### 6. document (6 个动作)

| 动作 | Mock 测试 | 真实调用 | JSON 参数 |
|------|-----------|---------|-----------|
| `list` | ✅ | ⚠️ 见下方 | — |
| `create` | ✅ | ✅ | `--tags` |
| `get` | ✅ | ✅ | — |
| `update` | ✅ | ✅ | `--tags` |
| `delete` | ✅ | ✅ | — |
| `get-content` | ✅ | ✅ | — |

### 7. extract (4 个动作)

| 动作 | Mock 测试 | 真实调用 | 说明 |
|------|-----------|---------|------|
| `submit` | ✅ | ✅ | 需要先创建文档 |
| `get-task` | ✅ | ✅ | 轮询任务状态 |
| `list-tasks` | ✅ | ✅ | 列出所有任务 |
| `wait` | ✅ | ⚠️ | 超时取决于 LLM 响应速度 |

### 8. settings (12 个动作)

| 动作 | Mock 测试 | 真实调用 |
|------|-----------|---------|
| `get-search` | ✅ | ✅ |
| `set-search` | ✅ | ✅ |
| `get-llm` | ✅ | ✅ |
| `set-llm` | ✅ | ✅ |
| `get-rank` | ✅ | ✅ |
| `set-rank` | ✅ | ✅ |
| `get-web-search` | ✅ | ✅ |
| `set-web-search` | ✅ | ✅ |
| `proxy` | ✅ | ❌ (需要真实 API key) |
| `get-tokenizer` | ✅ | ✅ |
| `add-tokenizer-words` | ✅ | ✅ |
| `remove-tokenizer-words` | ✅ | ✅ |

### 9. maas (2 个动作)

> `maas chat` 通过 `--messages` 参数一次性传入消息数组，非交互式。

| 动作 | Mock 测试 | 真实调用 | 参数覆盖 |
|------|-----------|---------|---------|
| `list-models` | ✅ | ✅ | — |
| `chat` | ✅ (3 种) | ✅ (消耗 token) | `--messages`, `--model`, `--stream` |

### 10. chat (1 个命令)

| 命令 | Mock 测试 | 真实调用 |
|------|-----------|---------|
| `chat` | ✅ | ❌ (交互式，不适合脚本) |

### 11. 全局选项

| 选项 | Mock 测试 | 真实调用 |
|------|-----------|---------|
| `--output json` | ✅ | ✅ |
| `--base-url` | ✅ | 所有测试使用 |
| `--timeout` | ✅ | ✅ |
| `--api-key` | ✅ | ✅ |

## 运行方式

```bash
# Mock 测试（快速，无外部依赖）
python3 -m pytest tests/test_cli.py -v

# 真实调用测试（需要后端运行中）
bash tests/test_cli_real.sh

# 同时运行所有测试
python3 -m pytest tests/test_cli.py -v && bash tests/test_cli_real.sh
```
