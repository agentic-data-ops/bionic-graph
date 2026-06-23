# 已执行计划: 设置系统重构 + 搜索配置 + 聊天UI改进

> 执行日期: 2026-06-23

## 变更内容

### 1. NeuralConfig 拆分为三个子分组

`src/config/settings.rs` — `NeuralConfig` 从扁平结构拆分为 `activate` / `search` / `learn` 三个嵌套组：

| 分组 | 字段 | 说明 |
|------|------|------|
| `activate` | `default_threshold`, `default_decay_rate`, `default_refractory_ticks`, `max_ticks`, `hot_threshold`, `min_synapse_strength`, `auto_stabilize` | 神经元激活参数 |
| `search` | `default_search_mode`, `greedy_exact_score`, `greedy_partial_score`, `exact_min_score`, `fuzzy_match_enabled`, `fuzzy_match_threshold` | 搜索模式/分数/模糊匹配 |
| `learn` | `enabled`, `co_fire_window`, `min_plasticity`, `synaptic_decay` | Hebbian 学习参数 |

- `fuzzy_match_enabled` 默认值改为 `true`
- 新增 `ActivateConfig`, `SearchConfig`, `LearnConfig` 三个子结构体
- `config/mod.rs` 重新导出所有子结构

### 2. 可配置搜索分数 + Levenshtein 模糊匹配

`src/neuron/neuron.rs`:
- 新增 `ScoreConfig` 结构体（搜索模式 + 各分数阈值 + 模糊匹配开关/阈值）
- 新增 `levenshtein_similarity()` 函数计算归一化编辑距离
- `match_keywords()` 签名从 `&SearchMode` 改为 `&ScoreConfig`，所有分数可配置
- 新增模糊匹配分支：子串匹配失败时触发 Levenshtein 距离匹配

`src/neuron/activation.rs`:
- `ActivationConfig` 扩展添加 `greedy_exact_score`, `greedy_partial_score`, `exact_min_score`, `fuzzy_match_enabled`, `fuzzy_match_threshold`
- `search()` 从 config 构建 `ScoreConfig` 并传递给 `match_keywords`

### 3. 配置从 settings.json 注入 NeuralNetwork

`src/neuron/network.rs`:
- 新增 `NeuralNetwork::with_config(activation, learning)` 构造器

`src/persistence/auto_save.rs`:
- `load_or_create()` 新增 `ActivationConfig` + `LearningConfig` 参数

`src/graph_manager.rs`:
- `GraphManager::open()` 接受 `&NeuralConfig`
- 新增 `neural_to_configs()` 方法将 `NeuralConfig` 转为 `ActivationConfig` + `LearningConfig`
- 存储 `neural_config` 供后续新图创建使用

`src/main.rs`:
- 将 `settings.neural` 传入 `GraphManager::open()`

### 4. 新增 `/settings/neural` API 端点

`src/gremlin/server.rs`:
- `GET /settings/neural` — 返回嵌套 JSON 结构（activate/search/learn）
- `PUT /settings/neural` — 接受扁平或嵌套键名更新配置并持久化到 settings.json

`src/ui/src/api.js`:
- 新增 `fetchNeuralConfig()` 和 `updateNeuralConfig()` 函数

### 5. 前端搜索设置页签

`src/ui/src/components/SettingsDialog.jsx`:
- 新增「搜索」页签，表单分三组：
  - 激活参数（max_ticks, hot_threshold, min_synapse, default_threshold, decay_rate, refractory_ticks, auto_stabilize）
  - 搜索模式 & 分数（default_search_mode, greedy_exact_score, greedy_partial_score, exact_min_score）
  - 模糊匹配（fuzzy_match_enabled, fuzzy_match_threshold）
  - Hebbian 学习（enabled, co_fire_window, min_plasticity, synaptic_decay）
- 加载时将嵌套 API 响应展平（`{...activate, ...search, ...learn}`），表单代码无需改动
- 保存调用 `PUT /settings/neural`

### 6. 预存 Bug 修复

| Bug | 文件 | 修复 |
|-----|------|------|
| `kwModeOpen` 未声明 | `ChatInput.jsx` | 添加 `useState(false)` |
| `m is not defined` | `KnowledgeBase.jsx` | 将 `isGlobalDefault` 移到 `models.map` 内部 |
| 下拉默认选中第一个而非全局默认模型 | `KnowledgeBase.jsx` | 移除 `__default__` 虚拟值，value 始终用真实模型名 |
| 选择默认模型时跳变为第一个 | `KnowledgeBase.jsx` | onChange 中判断模型是否等于 defaultModel |
| `chatCompletion is not defined` | `KnowledgeBase.jsx` | 添加缺失的 import |

### 7. 搜索模式 UI 调整

`src/ui/src/App.jsx`:
- 默认搜索模式从 `semantic` 改为 `keyword`

`src/ui/src/components/ChatInput.jsx`:
- Greedy/Exact 下拉仅在「关键词」模式下显示
- 「语义」模式下隐藏模式下拉

`src/ui/src/components/ChatArea.jsx`:
- 语义模式强制传 `greedy` 给搜索 API

### 8. 聊天消息操作图标

`src/ui/src/components/MessageList.jsx`:
- **用户消息**：hover 显示 📋（复制） + ✏️（修改后重提交）
- **助手消息**：hover 显示 📋（复制） + 💾（保存到知识库）
- 复制使用 `navigator.clipboard.writeText()`
- 编辑通过 `chatInputRef.current.setText()` + `.focus()` 实现

`src/ui/src/components/ChatInput.jsx`:
- `useImperativeHandle` 新增暴露 `setText(text)` 方法

### 9. 保存到知识库

`src/ui/src/App.jsx`:
- 新增 `kbInitialContent` / `kbInitialGraph` 状态
- `onSaveToKB` 回调：设置初始内容并打开知识库弹窗
- 关闭弹窗时清除初始内容

`src/ui/src/components/KnowledgeBase.jsx`:
- 新增 `initialContent` / `initialGraph` props
- 打开时自动预填导入内容、图库，并展开导入面板

### 10. 响应完成后自动聚焦输入框

`src/ui/src/components/ChatArea.jsx`:
- 关键词搜索、语义搜索、LLM 聊天三种路径的 `.finally` 或完成处均添加 `chatInputRef.current?.focus()`

## 涉及文件清单

| 文件 | 改动类型 |
|------|----------|
| `src/config/settings.rs` | 重构 — NeuralConfig 拆分为三组 |
| `src/config/mod.rs` | 新增导出 |
| `src/neuron/neuron.rs` | 新增 — ScoreConfig, levenshtein_similarity, 可配置 match_keywords |
| `src/neuron/activation.rs` | 扩展 — ActivationConfig 新字段, ScoreConfig 传递 |
| `src/neuron/network.rs` | 新增 — with_config 构造器 |
| `src/neuron/learning.rs` | 无改动 |
| `src/persistence/auto_save.rs` | 修改 — load_or_create 接受配置参数 |
| `src/graph_manager.rs` | 修改 — 接受 NeuralConfig, neural_to_configs |
| `src/main.rs` | 修改 — 传递 settings.neural |
| `src/gremlin/server.rs` | 新增 — GET/PUT /settings/neural |
| `src/memory_system.rs` | 修改 — 适配 load_or_create 新签名 |
| `src/storage/version_log.rs` | 修复 — 测试中 VersionRecord 缺字段 |
| `src/gremlin/query.rs` | 修复 — 测试缺字段 + serde skip_serializing_if |
| `src/gremlin/steps.rs` | 修复 — 测试缺 mode 字段 |
| `src/ui/src/api.js` | 新增 — fetchNeuralConfig, updateNeuralConfig |
| `src/ui/src/components/SettingsDialog.jsx` | 新增 — 搜索页签 |
| `src/ui/src/components/ChatInput.jsx` | 修复 — kwModeOpen + setText |
| `src/ui/src/components/ChatArea.jsx` | 修改 — 搜索模式逻辑 + 自动聚焦 + 传递 onSaveToKB |
| `src/ui/src/components/MessageList.jsx` | 新增 — 操作图标 (复制/编辑/保存) |
| `src/ui/src/components/KnowledgeBase.jsx` | 修复 — 导入缺失 + 模型选择 + initialContent |
| `src/ui/src/App.jsx` | 修改 — 默认搜索模式 + onSaveToKB |

## 配置示例 (`~/.config/bionic-graph/settings.json`)

```json
{
  "neural": {
    "activate": {
      "default_threshold": 0.7,
      "default_decay_rate": 0.1,
      "default_refractory_ticks": 3,
      "max_ticks": 20,
      "hot_threshold": 0.3,
      "min_synapse_strength": 0.01,
      "auto_stabilize": true
    },
    "search": {
      "default_search_mode": "greedy",
      "greedy_exact_score": 1.0,
      "greedy_partial_score": 0.8,
      "exact_min_score": 0.5,
      "fuzzy_match_enabled": true,
      "fuzzy_match_threshold": 0.6
    },
    "learn": {
      "enabled": true,
      "co_fire_window": 5,
      "min_plasticity": 0.001,
      "synaptic_decay": 0.01
    }
  }
}
```
