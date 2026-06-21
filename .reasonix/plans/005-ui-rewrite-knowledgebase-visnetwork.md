# 已执行计划: 前端 UI 重构 + 知识库 + vis-network 迁移

> 执行日期: 2026-06-21
> 对应 Commit: `9df5eec` (当前 HEAD)

---

## 1. 前端重写为 AI 助手式聊天界面

### 目标
将原有图谱搜索 UI 重构为 DeepSeek 风格的 AI 聊天界面。

### 变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/ui/src/App.jsx` | 重写 | 整体布局: 左侧 Sidebar + 右侧 ChatArea，全局状态(会话/设置)，localStorage 持久化 |
| `src/ui/src/App.css` | 重写 | 清除旧模板样式 |
| `src/ui/src/components/Sidebar.jsx` | 新建 | 左侧会话列表 + 新建对话 + 设置入口 |
| `src/ui/src/components/ChatArea.jsx` | 新建 | 聊天主区域: 消息列表 + 输入框，管理搜索/提取轮询 + LLM streaming |
| `src/ui/src/components/MessageList.jsx` | 新建 | 消息渲染: 用户气泡、助手流式消息、搜索进度步骤、图谱结果卡片 |
| `src/ui/src/components/ChatInput.jsx` | 新建 | 输入框 + 模型选择 + 图谱开关 + 附件上传 |
| `src/ui/src/components/SettingsDialog.jsx` | 新建 | 设置弹窗: 模型供应商管理 + 图库管理 + 通用设置 |
| `src/ui/src/components/NavBar.jsx` | 删除 | 功能迁移至 SettingsDialog |
| `src/ui/src/components/SearchBar.jsx` | 删除 | 功能迁移至 ChatInput |
| `src/ui/src/__tests__/components.test.jsx` | 更新 | 移除 NavBar/SearchBar 测试，适配新 API |
| `src/ui/src/locales/en.json` | 更新 | 添加 chat/settings 翻译 |
| `src/ui/src/locales/zh.json` | 更新 | 添加 chat/settings 翻译 |

### 数据流
- 会话历史 → `localStorage('bgraph-convs')`
- 供应商配置 → `localStorage('bgraph-settings')`
- LLM 聊天 → 前端直接 fetch OpenAI 兼容 API (SSE streaming)
- 图谱搜索 → `graphSearch` (gremlin `search` step)
- 语义搜索 → 前端 LLM 提取关键词 → graphSearch → 前端 LLM 过滤结果

---

## 2. 从 relation-graph 迁移到 vis-network

### 目标
替换 relation-graph (DOM/SVG-based) 为 vis-network (Canvas 2D)，解决 WebGL 兼容性和性能问题。

### 变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/ui/package.json` | 更新 | 移除 `@relation-graph/react`，添加 `vis-network` + `vis-data` |
| `src/ui/src/components/GraphViewer.jsx` | 重写 | 使用 vis-network 的 Network + DataSet |
| `src/ui/src/index.css` | 更新 | 移除 relation-graph/reagraph CSS 覆盖 |

### 对比

| 维度 | relation-graph (之前) | vis-network (现在) |
|------|----------------------|-------------------|
| 渲染引擎 | DOM/SVG | Canvas 2D |
| JS 体积 | ~1.6MB (含 Three.js) | ~818KB |
| CPU 占用 | 高 | 低 (物理 100 轮后停止) |
| WebGL 依赖 | 否 | 否 |
| 交互 | 易用但难以定制 | click/doubleClick 原生事件 |

---

## 3. 嵌入式前端 (rust-embed)

### 目标
将前端编译产物嵌入 Rust 二进制，单文件部署。

### 变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `Cargo.toml` | 更新 | 添加 `rust-embed` + `mime_guess` |
| `src/ui_serve.rs` | 新建 | 嵌入式静态文件服务模块，支持 MIME 类型 + SPA fallback |
| `src/gremlin/server.rs` | 更新 | 用嵌入式 handler 替换 `ServeDir` |
| `src/lib.rs` | 更新 | 注册 `ui_serve` 模块 |

### 工作方式
- `cargo build` 时: rust-embed 将 `src/ui/dist/` 嵌入二进制
- 运行时: 所有 `/ui/*` 请求从内存响应，不依赖磁盘
- SPA 路由: `/ui/*` 非文件路径 fallback 到 `index.html`

---

## 4. 语义搜索迁移到前端

### 目标
后端删除 `semanticSearch` gremlin 步骤和 `/search/semantic` 端点，前端直接调用 LLM 完成语义搜索。

### 变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/gremlin/query.rs` | 更新 | `keywordSearch` → `search`；删除 `SemanticSearch` 变体 |
| `src/gremlin/steps.rs` | 更新 | 删除 `SemanticSearch` match arm 及相关辅助函数 |
| `src/gremlin/server.rs` | 更新 | 删除 `SearchTaskManager`、`/search/semantic`、`search_task_handler` |
| `src/memory_system.rs` | 更新 | 移除 `search_task_manager` 字段 |
| `src/ui/src/api.js` | 更新 | `keywordSearch` → `graphSearch`；删除 async search API |
| `src/ui/src/components/ChatArea.jsx` | 更新 | 重写 graph 模式为 前端 LLM → search → 前端 LLM 过滤 |

### 搜索流程
```
关键词模式: 用户输入 → 按空格分词 → graphSearch → 展示图谱
语义模式:   用户输入 → LLM 提取关键词 → graphSearch → LLM 过滤结果 → 展示图谱
```

---

## 5. 知识库管理 (文档 CRUD + 前端 LLM 提取)

### 目标
左侧导航增加知识库入口，支持 Markdown 文档管理 + LLM 自动提取实体关系到图谱。

### 变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/documents.rs` | 新建 | 文档管理模块: CRUD + 文件存储 + JSON 索引 |
| `src/lib.rs` | 更新 | 注册 `documents` 模块 |
| `src/gremlin/server.rs` | 更新 | 添加文档路由 + 顶点删除端点 |
| `src/memory_system.rs` | 更新 | `AppState` 添加 `document_manager` |
| `src/ui/src/api.js` | 更新 | 添加文档 CRUD + 顶点/边 API |
| `src/ui/src/components/KnowledgeBase.jsx` | 新建 | 知识库弹窗: 文件列表(标签过滤) + 添加(上传/粘贴) + 编辑 + 删除 |
| `src/ui/src/components/Sidebar.jsx` | 更新 | 添加知识库入口按钮 |
| `src/ui/src/locales/*.json` | 更新 | 添加知识库翻译 |

### 后端 API

| 端点 | 方法 | 说明 |
|------|------|------|
| `/documents` | GET | 文档列表 |
| `/documents` | POST | 添加文档 |
| `/documents/:id` | GET/PUT/DELETE | 文档元数据 CRUD |
| `/documents/:id/content` | GET | 获取文档原文 |
| `/vertices/:id` | DELETE | 删除顶点及关联边 |

### 前端 LLM 提取流程
1. 用户添加/编辑文档 → 前端调用 LLM 生成标题 (≤30字, 原文语言, 无标点)
2. LLM 生成标签 (2-4 个)
3. LLM 提取实体和关系 (JSON) → 调用后端 `POST /vertices` + `POST /edges` 写入图库
4. 顶点属性: `source_file`, `chapter_path`
5. 编辑时: 删除旧顶点/边 → 重新提取
6. 删除时: 删除文档 + 清除相关顶点/边

---

## 6. 样式优化 (Mac 风格)

### 变更
- `src/ui/src/index.css`: 添加 macOS 系统字体栈、自定义滚动条、动画、毛玻璃效果
- 所有组件: 替换配色为 macOS 暗色系 (`#1a1a1e`/`#1c1c20`/`#2a2a2e`) + 系统蓝 `#0a84ff`
- vis-network 选项: `font.face: '-apple-system'`、`color` 暗色配

---

## 7. Mac 暗色系配色表

| 用途 | 色值 | 说明 |
|------|------|------|
| 页面背景 | `#1a1a1e` | 最深色 |
| 卡片/面板 | `#1c1c20` | 次级背景 |
| 输入框/按钮 | `#2a2a2e` | 三级背景 |
| hover 状态 | `#3a3a3e` | 悬浮高亮 |
| 主文字 | `#e5e5e7` | 白色文字 |
| 次级文字 | `#636366` | 灰色文字 |
| 占位文字 | `#48484a` | 暗灰文字 |
| 强调色 | `#0a84ff` | 苹果蓝 |
| 成功 | `#30d158` | 绿色 |
| 警告 | `#ff9f0a` | 橙色 |
| 错误 | `#ff453a` | 红色 |
