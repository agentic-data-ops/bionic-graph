# Plan 003: keywordSearch + SemanticSearch + 全局 LLM 配置

## 目标

1. **重命名** `search` → `keywordSearch`（Gremlin 步骤 + JSON 接口）
2. **新增** `semanticSearch` 步骤：输入自然语言 → LLM 提取关键词 → keywordSearch
3. **全局化** LLM 配置：`BGRAPH_EXTRACT_API_KEY` → `BGRAPH_LLM_API_KEY`，配置从 extraction 子模块提升为全局

## 步骤

### Step 1: 环境变量重命名

| 文件 | 改动 |
|------|------|
| `src/config/loader.rs` | `BGRAPH_EXTRACT_API_KEY` → `BGRAPH_LLM_API_KEY` |
| `src/extract/config.rs` | `from_settings` 读取 key 的 env var 名更新 |
| `src/config/settings.rs` | 如有 extraction.api_key 引用则更新 |

### Step 2: search → keywordSearch

| 文件 | 改动 |
|------|------|
| `src/gremlin/query.rs` | `#[serde(rename = "search")]` → `#[serde(rename = "keywordSearch")]` |
| `src/gremlin/steps.rs` | 所有 `TraversalStep::Search` 引用 |
| `src/gremlin/server.rs` | `POST /search` handler 改 JSON 步骤名 |
| `src/memory_system.rs` | `search()` 方法中的步骤引用 |
| 测试文件 | 更新所有 JSON roundtrip + 执行测试 |

### Step 3: 新增 semanticSearch 步骤

| 文件 | 改动 |
|------|------|
| `src/gremlin/query.rs` | 新增 `SemanticSearch { query: String }` 变体 |
| `src/gremlin/steps.rs` | 实现：调 LLM → 提取关键词 → 调 keywordSearch |
| `src/gremlin/server.rs` | 可选：新增 `POST /semantic-search` 端点 |
| 测试 | JSON roundtrip + 执行测试 |

### Step 4: 验证

- `cargo test` 全部通过
- 重命名后旧 `search` 接口返回错误提示

## 执行顺序

1. 先改环境变量（影响最小）
2. 再改 search → keywordSearch
3. 最后加 semanticSearch
