# Plan: 知识库文档提取架构重构 & 多图库支持

## 背景

将文档知识图谱提取（调用大模型解析文档、创建顶点/边）从前端迁移到后端，并通过后台任务跟踪任务状态、步骤进度和结果统计。同时支持多供应商/多模型配置，持久化到后端文件。

## 架构变化

### 之前（前端完成全部提取）

```
Frontend → LLM API (直接浏览器调用) → 解析 JSON → POST /vertices + POST /edges
```

### 之后（后端后台任务）

```
Frontend → POST /documents → POST /documents/:id/extract
                              ↓
                         后台任务（tokio::spawn）
                              ↓
                  1. 读取文档 + token 估算
                  2. 一次 LLM 调用提取实体/关系
                  3. 创建 vertices（含 neuron 索引）
                  4. 创建 edges（含 auto-synapse）
                              ↓
Frontend ← poll GET /extract/tasks/:id ← 步骤进度 + 统计
```

## 新增模块

### `src/extract/document_extractor.rs`
- 全文档一次性 LLM 提取引擎
- Token 估算 + 上下文窗口检查
- 截断检测（finish_reason == "length"）
- Step 级进度回调

## 修改文件清单

### 后端

| 文件 | 改动 |
|------|------|
| `src/config/settings.rs` | `ExtractionConfig` → `LlmConfig` (多供应商+多模型)；`models` 改为字符串数组；`default_model` 改为 `"Provider/Model"` 格式；移除 `default_provider` |
| `src/config/loader.rs` | 新增 `save_settings()` 运行时持久化；移除 LLM 相关 env var 覆写 |
| `src/config/mod.rs` | 导出 `LlmConfig`、`LlmProvider`、`save_settings` |
| `src/extract/config.rs` | 新增 `from_llm_config()`；解析 `"Provider/Model"` 查找对应供应商的 api_key |
| `src/extract/llm_client.rs` | `LlmResult` 新增 `finish_reason` 字段 |
| `src/extract/task_manager.rs` | 新增 `ExtractionStep` 步进进度模型；`submit_document_extraction()` 方法；`TaskResponse` 前端视图 |
| `src/extract/mod.rs` | 导出 `document_extractor` 模块 |
| `src/documents.rs` | `Document` 新增 `graph_name: String`；`update()` 不再修改内容 |
| `src/gremlin/server.rs` | `AppState.settings: Arc<Mutex<Settings>>`；新增 `GET/PUT /settings`；`POST /documents/:id/extract` 支持 `X-Graph-Name` 读取图库；`add_vertex_handler` 创建 neuron 时加入 name 关键词 |
| `src/gremlin/steps.rs` | `fill_vertex_details` 用 `filter_map` 过滤已删除的 vertex ID |
| `src/memory_system.rs` | 适配新的 AppState |
| `src/main.rs` | 构建 `Arc<Mutex<Settings>>`；移除 `ExtractionConfig` 直接构造 |

### 前端

| 文件 | 改动 |
|------|------|
| `src/ui/src/api.js` | 新增 `fetchSettings()` / `updateSettings()`；`addDocument()` 接受 `graphName`；`updateDocument()` 不再传 content；`startDocumentExtraction(docId, graphName)` 传图库 header |
| `src/ui/src/App.jsx` | 启动时从后端加载 settings；provider 切换时同步到后端；`tempModel` 临时模型切换 |
| `src/ui/src/components/ChatInput.jsx` | 临时模型输入框改为 `Provider/Model` 下拉框；与图谱控件同一行右侧 |
| `src/ui/src/components/ChatArea.jsx` | 标题栏右侧添加主题/语言切换；传递 tempModel |
| `src/ui/src/components/SettingsDialog.jsx` | 标签页名"供应商"→"大模型"（后改为"模型"）；多模型编辑（添加/删除/默认）；默认模型下拉框"Provider/Model"；图库改为行内操作（设为默认/归档/删除）；去掉"通用"标签页 |
| `src/ui/src/components/KnowledgeBase.jsx` | 导入弹窗独立（Modal 覆盖层）；使用后端任务提取；标签过滤/图库过滤 label；文档列表显示图库名；编辑文档改为弹窗（仅标题+标签） |
| `src/ui/src/locales/*.json` | 新增多个 i18n 键 |

## 关键数据结构

### settings.json 新格式

```json
{
  "llm": {
    "providers": [
      {
        "name": "DeepSeek",
        "api_base_url": "https://api.deepseek.com/v1",
        "api_key": "sk-...",
        "models": ["deepseek-v4-flash", "deepseek-v4-pro"]
      }
    ],
    "default_model": "DeepSeek/deepseek-v4-flash",
    "context_window": 65536,
    "max_output_tokens": 16384,
    "max_retries": 3
  }
}
```

### 步骤级进度模型

```rust
pub struct ExtractionStep {
    pub label: String,        // "Calling LLM to extract knowledge"
    pub status: String,       // "pending" | "running" | "completed" | "failed"
    pub progress_pct: f64,    // 0.0–100.0
    pub detail: Option<String>, // "15/20 vertices created"
}
```

## API 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/settings` | 返回完整 LLM 配置（含 api_key） |
| `PUT` | `/settings` | 更新 LLM 配置并持久化到文件 |
| `GET` | `/extract/tasks` | 列出所有提取任务 |
| `GET` | `/extract/tasks/:id` | 查询任务状态（含步骤进度） |
| `POST` | `/documents/:id/extract` | 提交文档提取（支持 X-Graph-Name header） |

## 待办 / 已知问题

- [ ] KnowledgeBase.jsx 中的 `runExtraction` 仍使用前端提取流程（调用 LLM 创建顶点），应改为使用 `startDocumentExtraction` 后端任务流程
- [ ] 删除文档时应清理关联的 neural network 引用
- [ ] `/documents/:id/extract` 端点应支持更多配置（如选择模型）
