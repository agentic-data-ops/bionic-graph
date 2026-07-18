# Plan 110 — 前端联网搜索功能

## Summary

为聊天功能增加联网搜索能力。用户可在聊天输入框上方开启/关闭联网搜索（放在模型选择器右侧、图谱搜索开关左侧），在设置弹窗中管理联网搜索供应商（名称、search URL、可选请求参数、可选请求头），并预置 Bing 搜索作为默认供应商。开启联网搜索时，前端直接请求供应商获取搜索结果，展示搜索过程和结果概况，让 LLM 识别最相关的 1~5 条结果并获取对应网页详情，最后将搜索结果作为参考信息交给 LLM 回答用户问题。联网搜索与图谱搜索可同时开启，两者的结果都作为 LLM 上下文。

## Changes

### Backend

| File | Change |
|------|--------|
| `src/config/settings.rs` | Add `WebSearchConfig` struct (list of providers, default provider ID). Each provider: `id`, `name`, `search_url` (template with `{text}` placeholder), optional `params` (key-value pairs), optional `headers` (key-value pairs). Include in `Settings` struct. |
| `src/config/loader.rs` | Load/save WebSearchConfig. On first run, pre-populate with default Bing provider. |
| `src/gremlin/settings.rs` | Add `GET /settings/web-search` and `PUT /settings/web-search` routes. Register in router. |

### Frontend

| File | Change |
|------|--------|
| `src/ui/src/api.js` | Add `fetchWebSearchConfig()`, `updateWebSearchConfig()`, `searchWeb()` functions. |
| `src/ui/src/components/ChatInput.jsx` | Add web search toggle switch between model selector and graph toggle. Wire `useWebSearch`, `onWebSearchToggle` props. |
| `src/ui/src/components/ChatArea.jsx` | Integrate web search flow into `handleSend`: if web search enabled, call search provider, show progress steps, let LLM pick top results, fetch page details, inject into LLM context alongside graph data if both enabled. |
| `src/ui/src/components/SettingsDialog.jsx` | Add "联网搜索" tab with provider list editor (same pattern as LLM providers), default provider selector, per-provider config (name, search URL, params, headers). |
| `src/ui/src/components/MessageList.jsx` | Handle `web_search_progress` message type for displaying search steps and results. |
| `src/ui/src/locales/*.json` | Add i18n keys for web search UI strings. |

### Side Effects

Frontend CORS: if the web search provider is a different origin, the browser's CORS policy may block `fetch()` requests. Mitigation: (1) prefer providers that support CORS, (2) if needed, add a backend proxy endpoint `/api/web-search` that forwards the request server-side.

## Details

### 1. Backend Settings — WebSearchConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchProvider {
    pub id: String,
    pub name: String,
    /// URL template, e.g. "https://cn.bing.com/search?q={text}"
    pub search_url: String,
    #[serde(default)]
    pub params: HashMap<String, String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    #[serde(default)]
    pub providers: Vec<WebSearchProvider>,
    /// ID of the default provider
    #[serde(default)]
    pub default_provider: String,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            providers: vec![WebSearchProvider {
                id: "bing".to_string(),
                name: "Bing Search".to_string(),
                search_url: "https://cn.bing.com/search?q={text}".to_string(),
                params: HashMap::new(),
                headers: HashMap::new(),
            }],
            default_provider: "bing".to_string(),
        }
    }
}
```

### 2. Frontend — ChatInput Toggle

Toggle placed between model selector and graph toggle:

```
[ Model Selector ▼ ] [🌐 Web Search ⬜] [🔍 Graph Search ⬜] [ graph0 ▼ ] [ 贪婪 ▼ ] [⏱ time travel]
```

Props added to ChatInput:
- `useWebSearch: boolean`
- `onWebSearchToggle: (enabled: boolean) => void`

### 3. Frontend — Web Search Flow in ChatArea.handleSend

```
User sends message
  │
  ├── if useWebSearch:
  │     ├── Show progress: "Searching web..." (streaming)
  │     ├── Call searchWeb(provider, query) → raw HTML/text results
  │     ├── Show progress: "Analyzing search results..."
  │     ├── Call LLM with results text → LLM picks 1-5 best URLs + reasons
  │     ├── Show progress: "Fetching page details..." with selected URLs
  │     ├── Fetch each selected URL content (via web_fetch or backend proxy)
  │     ├── Collect page contents
  │     └── Mark web search complete
  │
  ├── if useGraph:
  │     └── (existing graph search flow)
  │
  ├── Build LLM context:
  │     ├── If web search results exist → inject as system message
  │     ├── If graph data exists → inject as system message
  │     └── If both exist → inject both
  │
  └── Call LLM with combined context
```

### 4. Frontend — searchWeb() API function

```javascript
export async function searchWeb(provider, query) {
  const url = provider.search_url.replace('{text}', encodeURIComponent(query));
  const params = new URLSearchParams(provider.params || {});
  const fullUrl = params.toString() ? url + '&' + params.toString() : url;

  const res = await fetch(fullUrl, {
    headers: { ...(provider.headers || {}) },
  });
  const html = await res.text();
  return html; // Raw HTML, sent to LLM for parsing
}
```

Since CORS may block frontend cross-origin requests, two strategies:
1. **Prefer CORS-friendly providers** (e.g., some search APIs return `Access-Control-Allow-Origin: *`)
2. **Backend proxy fallback**: if frontend direct fetch fails, add `POST /api/web-search` backend endpoint that forwards the request

### 5. Frontend — Web Search Message Type

New message type `web_search_progress` with steps:
- `searching` — Calling search engine
- `analyzing` — LLM selecting relevant results
- `fetching` — Fetching page details
- `done` — Complete

Display progress steps similar to `search_progress`, with clickable result URLs and snippet previews.

### 6. SettingsDialog — Web Search Tab

New tab "联网搜索" with:
- Provider list (same pattern as LLM providers tab): add/edit/delete providers
- Each provider form: name, search URL (with `{text}` placeholder hint), params (key-value list), headers (key-value list)
- Default provider selector (dropdown)
- Cancel/Save buttons

### 7. LLM Context Injection

When both web search and graph search are enabled, build context messages in this order:

```
System: Web search results:
[搜索结果摘要 + 选中页面的详细内容]

System: Knowledge graph data:
[图谱实体和关系]

User: [原始问题]
```

The LLM receives both information sources and can synthesize an answer.

## Files Changed (Estimated)

```
M src/config/settings.rs          — WebSearchConfig struct + Default
M src/config/loader.rs            — Load/save web search config
M src/gremlin/settings.rs         — GET/PUT /settings/web-search routes
M src/ui/src/api.js               — fetchWebSearchConfig, updateWebSearchConfig, searchWeb
M src/ui/src/components/ChatInput.jsx    — Web search toggle
M src/ui/src/components/ChatArea.jsx     — Web search flow in handleSend
M src/ui/src/components/SettingsDialog.jsx — Web search tab
M src/ui/src/components/MessageList.jsx  — web_search_progress message rendering
M src/ui/src/locales/en/translation.json — i18n keys
M src/ui/src/locales/zh/translation.json — i18n keys
```

## Implementation Order

1. ✅ Backend: add `WebSearchConfig` struct + default Bing provider + settings routes
2. ✅ Frontend: `ChatInput` web search toggle + wire props through `ChatArea`
3. ✅ Frontend: `api.js` — `searchWeb()`, `fetchWebSearchConfig()`, `updateWebSearchConfig()`
4. ✅ Frontend: `SettingsDialog` — web search tab
5. ✅ Frontend: `ChatArea.handleSend` — web search flow (search → LLM select → fetch pages → inject context)
6. ✅ Frontend: `MessageList` — `web_search_progress` rendering
7. ✅ Frontend: i18n strings
8. ✅ Backend: `/web-search/proxy` proxy endpoint to avoid CORS issues
9. ⬜ Test: end-to-end with browser, verify combined graph+web mode
