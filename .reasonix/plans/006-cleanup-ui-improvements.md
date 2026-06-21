# 已执行计划: 清理 + UI 改进 + 知识库重构

> 执行日期: 2026-06-21

## 变更内容

### 1. 移除后端 extract/task API
后端 `POST /extract`、`GET /extract/task/:id`、`GET /extract/tasks` 已无用（文档解析已全部前端化），移除路由和对应 handler。

### 2. 移除前端附件上传
ChatInput 的附件按钮和 ChatArea 的 handleAttach 逻辑一并移除。

### 3. 消息支持选择和复制
MessageList 的消息文本添加 `select-text` / `user-select` CSS，允许用户选中和复制。

### 4. 图节点属性支持选择和复制
GraphViewer 的 InfoPanel 添加 `select-text`，属性值可选中复制。

### 5. 支持删除聊天会话
Sidebar 每个会话项悬停时显示删除按钮。

### 6. 模型选择器移到最左侧、始终可见
ChatInput 顶部的 provider 下拉移到最左侧，不管是否开启语义搜索都显示。

### 7. 知识库移到设置上方
Sidebar 中知识库按钮移到设置按钮之前。

### 8. 知识库弹窗重构
- "粘贴文本"和"上传 .md"合并为"导入"按钮
- 导入时可选图库 + 可选解析模型
- 展示导入进度和每步结果
