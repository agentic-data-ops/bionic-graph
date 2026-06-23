# 计划: 聊天输入框工具栏布局重排

> 状态: 待执行

## 目标

重构 `ChatInput.jsx` 上方工具栏的布局和交互，修复自动聚焦问题，增加 label、调整间距、优化搜索模式 UI、增加时间旅行日期选择器、移动模型选择器到最右。

---

## 1. 修复响应完成后自动聚焦

**现状**: 之前在 `ChatArea.jsx` 三处响应完成路径添加了 `chatInputRef.current?.focus()`，但未生效。

**排查方向**:
- `chatInputRef.current` 可能为 null（ref 绑定时机问题）
- `focus()` 方法调用了但浏览器焦点未转移（textarea 可能被覆盖/隐藏）
- React 状态更新后焦点被抢走

**修复方案**:
- 在 `ChatArea.jsx` 中使用 `requestAnimationFrame` 或 `setTimeout(..., 0)` 延迟聚焦
- 确认 `chatInputRef` 正确绑定到 `ChatInput` (forwardRef + useImperativeHandle)
- 打开 `ChatInput` 确认 `textareaRef.current?.focus()` 在 textarea 元素上正确执行

**涉及文件**:
- `src/ui/src/components/ChatArea.jsx` — 添加延迟聚焦逻辑
- `src/ui/src/components/ChatInput.jsx` — 确认 ref 暴露正确

---

## 2. 图库选择 + Label

**现状**: 图库下拉框没有 label。

**要求**: 在图库 `<select>` 前面添加 label「图库」。

**涉及文件**:
- `src/ui/src/components/ChatInput.jsx`

---

## 3. 搜索模式切换 + Label

**现状**: 搜索模式切换按钮没有 label。

**要求**: 在搜索模式 toggle 前面添加 label「搜索模式」。

**涉及文件**:
- `src/ui/src/components/ChatInput.jsx`

---

## 4. 搜索模式布局调整 + 默认语义

**现状**:
- 默认模式是 `keyword`
- 按钮顺序：关键词 | 语义
- 搜索模式储存在 `localStorage('bgraph-settings')` 的 `searchMode` 字段

**要求**:
- 交换按钮顺序：语义 | 关键词（语义在左，关键词在右）
- 默认选中「语义」
- 用户切换后持久化到 `localStorage('bgraph-settings')`

**涉及文件**:
- `src/ui/src/App.jsx` — 修改默认值 `searchMode: 'semantic'`
- `src/ui/src/components/ChatInput.jsx` — 交换按钮顺序

---

## 5. 关键词匹配模式下拉框

**现状**: 选中「关键词」模式时，展示 Greedy/Exact 下拉框。

**要求**:
- 保持选中「关键词」时显示此下拉框
- 下拉框可选：贪婪搜索 / 精确搜索（英文: Greedy Search / Exact Search）
- 使用 `t()` 国际化

**涉及文件**:
- `src/ui/src/components/ChatInput.jsx`
- `src/ui/src/locales/en.json` — 添加翻译
- `src/ui/src/locales/zh.json` — 添加翻译

---

## 6. 各选项组件间增加间隔

**现状**: 图库、搜索模式、时间旅行、Greedy/Exact 下拉等组件排列过于紧密。

**要求**: 各组件间增加适当间距（如 `gap-3` 或 `ml-3` / `mr-3`）。

**涉及文件**:
- `src/ui/src/components/ChatInput.jsx`

---

## 7. 时间旅行日期选择器

**现状**: 时间旅行只有一个 checkbox。

**要求**: 勾选时间旅行时，额外显示一个日期+时间选择器 (`<input type="datetime-local">`)，用户可指定快照时间点。

**涉及文件**:
- `src/ui/src/components/ChatInput.jsx` — 添加条件渲染的 datetime-local input
- `src/ui/src/components/ChatArea.jsx` — 将时间值传递给搜索/LLM 调用
- `src/ui/src/App.jsx` — 可能需添加 `timeTravelPoint` 状态

---

## 8. 模型选择器移到最右

**现状**: 模型选择器在输入框上方工具栏的最左侧。

**要求**: 将模型选择器移到工具栏的最右侧。

**涉及文件**:
- `src/ui/src/components/ChatInput.jsx`

---

## 涉及文件清单

| 文件 | 改动 |
|------|------|
| `src/ui/src/components/ChatInput.jsx` | 大幅调整：label、布局、间距、按钮顺序、模型选择器位置、时间旅行日期选择器 |
| `src/ui/src/components/ChatArea.jsx` | 修复自动聚焦、传递时间旅行时间戳 |
| `src/ui/src/App.jsx` | 默认搜索模式改回 semantic |
| `src/ui/src/locales/en.json` | 添加 Greedy Search / Exact Search 翻译 |
| `src/ui/src/locales/zh.json` | 添加 贪婪搜索 / 精确搜索 翻译 |
