const BASE = '';

async function api(path, opts = {}) {
  const res = await fetch(BASE + path, {
    ...opts,
    headers: { 'Content-Type': 'application/json', ...opts.headers },
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(body);
  }
  return res.json();
}

export async function health() {
  return api('/health');
}

export async function listGraphs() {
  return api('/graphs');
}

export async function createGraph(name, description = '', timeTravel = false) {
  return api('/graphs', {
    method: 'POST',
    body: JSON.stringify({ name, description, time_travel: timeTravel }),
  });
}

export async function deleteGraph(name) {
  return api(`/graphs/${name}`, { method: 'DELETE' });
}

export async function setDefaultGraph(name) {
  return api('/graphs', {
    method: 'PUT',
    body: JSON.stringify({ default: name }),
  });
}

export async function updateGraphMeta(name, description, timeTravel) {
  return api(`/graphs/${encodeURIComponent(name)}`, {
    method: 'PUT',
    body: JSON.stringify({ description, time_travel: timeTravel }),
  });
}

export async function gremlin(steps, graph = '', timeTravelAt) {
  const headers = {};
  if (graph) headers['X-Graph-Name'] = graph;
  if (timeTravelAt) headers['X-Time-Travel'] = timeTravelAt;
  return api('/gremlin', {
    method: 'POST',
    headers,
    body: JSON.stringify({ steps }),
  });
}

export async function graphSearch(text, graph = 'default', mode) {
  return gremlin([{ step: 'search', text, mode }], graph);
}

/**
 * Traverse from a vertex by ID.
 * `timeTravelAt` sets X-Time-Travel header for point-in-time queries.
 */
export async function traverse(vid, label = null, graph = 'default', timeTravelAt) {
  const steps = [
    { step: 'V', ids: [vid] },
    { step: 'expand', depth: 1, ...(label ? { label } : {}) },
  ];
  return gremlin(steps, graph, timeTravelAt);
}

// ─── Sync extraction (legacy, still works) ───────────────────────

export async function extractDoc(content, graph = '') {
  const headers = { 'Content-Type': 'text/markdown' };
  if (graph) headers['X-Graph-Name'] = graph;
  const res = await fetch(BASE + '/extract', {
    method: 'POST',
    headers,
    body: content,
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

// ─── Async extraction (task-based) ──────────────────────────────

/** Submit a markdown document for async extraction. Returns { task_id, status } */
export async function extractDocAsync(content, graph = '') {
  const headers = { 'Content-Type': 'text/markdown' };
  if (graph) headers['X-Graph-Name'] = graph;
  const res = await fetch(BASE + '/extract', {
    method: 'POST',
    headers,
    body: content,
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** Get the status and results of an extraction task. */
export async function getTaskStatus(taskId) {
  const res = await fetch(BASE + `/tasks/${encodeURIComponent(taskId)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** List all extraction tasks (newest first). */
export async function listExtractTasks() {
  const res = await fetch(BASE + '/tasks');
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function getVertex(vid, graph = 'default') {
  return gremlin([{ step: 'V', ids: [vid] }], graph);
}

export async function updateVertex(vid, props, labels, graph = 'default') {
  return api(`/vertices/${vid}`, {
    method: 'PUT',
    headers: { 'X-Graph-Name': graph },
    body: JSON.stringify({
      name: props?.name || '',
      keywords: props?.keywords || [],
      labels: labels || [],
      properties: props || {},
    }),
  });
}

// ─── LLM Chat (OpenAI-compatible streaming) ─────────────────────

/**
 * Call an OpenAI-compatible chat completion API with SSE streaming.
 * Returns an object with:
 *   - response: the fetch Response (for reading body as SSE)
 *   - abort: () => void  to abort the request
 */
export function chatCompletion(messages, { apiBase, model, apiKey }) {
  const controller = new AbortController();
  const url = apiBase.replace(/\/+$/, '') + '/chat/completions';

  const promise = fetch(url, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(apiKey ? { Authorization: `Bearer ${apiKey}` } : {}),
    },
    body: JSON.stringify({
      model,
      messages,
      stream: true,
    }),
    signal: controller.signal,
  });

  return {
    response: promise,
    abort: () => controller.abort(),
  };
}

/**
 * Parse an SSE stream from a chat completion response.
 * Calls `onToken(token: string)` for each content chunk and
 * `onDone()` when the stream ends.
 * Returns when the stream completes.
 */
export async function parseSSEStream(response, onToken, onDone) {
  if (!response.ok) {
    const body = await response.text();
    throw new Error(`LLM API error ${response.status}: ${body}`);
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() || ''; // keep incomplete line

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed || !trimmed.startsWith('data: ')) continue;
      const data = trimmed.slice(6);
      if (data === '[DONE]') { onDone?.(); return; }
      try {
        const parsed = JSON.parse(data);
        const content = parsed.choices?.[0]?.delta?.content || '';
        if (content) onToken?.(content);
      } catch {
        // ignore parse errors for partial chunks
      }
    }
  }
  onDone?.();
}

// ─── Document Management ─────────────────────────────────────────

export async function listDocuments() {
  return api('/documents');
}

export async function addDocument(title, content, tags = []) {
  return api('/documents', {
    method: 'POST',
    body: JSON.stringify({ title, content, tags }),
  });
}

export async function getDocument(id) {
  return api(`/documents/${encodeURIComponent(id)}`);
}

export async function getDocumentContent(id) {
  const res = await fetch(`/documents/${encodeURIComponent(id)}/content`);
  if (!res.ok) throw new Error(await res.text());
  return res.text();
}

export async function updateDocument(id, title, tags = []) {
  return api(`/documents/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify({ title, tags }),
  });
}

export async function deleteDocument(id, cleanGraph) {
  const url = cleanGraph ? `/documents/${encodeURIComponent(id)}?clean=true` : `/documents/${encodeURIComponent(id)}`;
  return api(url, { method: 'DELETE' });
}

// ─── MaaS Proxy (OpenAI-compatible backend proxy) ──────────────

/**
 * Fetch available models from the backend MaaS proxy.
 * Returns { models: [...], defaultModel: "Provider/Model" }
 * where models is the OpenAI-compatible list { object, data }.
 */
export async function fetchModels() {
  const res = await fetch('/proxy/openai/v1/models');
  if (!res.ok) throw new Error(await res.text());
  const defaultModel = res.headers.get('x-default-model') || '';
  const models = await res.json();
  return { models, defaultModel };
}

/**
 * Call the backend MaaS proxy for chat completions.
 * The backend forwards to the actual provider using stored API keys.
 * Returns an object with:
 *   - response: the fetch Response (for reading body as SSE)
 *   - abort: () => void  to abort the request
 */
export function chatCompletionProxy(messages, model, stream = true) {
  const controller = new AbortController();

  const promise = fetch('/proxy/openai/v1/chat/completions', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      model,
      messages,
      stream,
    }),
    signal: controller.signal,
  });

  return {
    response: promise,
    abort: () => controller.abort(),
  };
}

// ─── Settings Sync ───────────────────────────────────────────────

/** Get a single provider config (apiKey, apiBase) from backend by provider name. */
export async function fetchProviderConfig(providerName) {
  const res = await fetch('/settings/llm');
  if (!res.ok) throw new Error(await res.text());
  const data = await res.json();
  const provider = data?.llm?.providers?.find(p => p.name === providerName);
  if (!provider) throw new Error(`Provider "${providerName}" not found in backend settings`);
  return provider;
}

/** Fetch full LLM settings from backend (providers, models, api_keys). */
export async function fetchLlmSettings() {
  const res = await fetch('/settings/llm');
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/**
 * Save full LLM providers config to backend.
 * @param {Array} providers - [{ name, api_base_url, api_key, models: ["model1", "model2"] }]
 * @param {string} defaultModel - "ProviderName/ModelName"
 */
export async function updateLlmSettings(providers, defaultModel) {
  const body = { providers };
  if (defaultModel) body.default_model = defaultModel;
  const res = await fetch('/settings/llm', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

// ─── Search Config (new) ───────────────────────────────────────

/**
 * Fetch search/traverse configuration from backend.
 * Returns: { greedy: {traverse,activate,decay,depth,score}, exact: {...} }
 */
export async function fetchSearchConfig() {
  const res = await fetch('/settings/graph/search');
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/**
 * Update search/traverse configuration on the backend.
 * @param {Object} config - { greedy: {...}, exact: {...} }
 */
export async function updateSearchConfig(config) {
  const res = await fetch('/settings/graph/search', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

// ─── Rank Config ────────────────────────────────────────────────

/** Fetch rank configuration from the backend. */
export async function fetchRankConfig() {
  const res = await fetch('/settings/graph/rank');
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** Update rank configuration on the backend. */
export async function updateRankConfig(config) {
  const res = await fetch('/settings/graph/rank', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

// ─── Document Extraction (Backend Task) ─────────────────────────

/** Submit a document for background extraction. Returns { task_id, status } */
export async function startDocumentExtraction(docId, graphName, model) {
  const headers = {};
  if (graphName) headers['X-Graph-Name'] = graphName;
  let url = `/documents/${encodeURIComponent(docId)}/extract`;
  if (model) url += `?model=${encodeURIComponent(model)}`;
  const res = await fetch(url, {
    method: 'POST',
    headers,
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** Get extraction task status (with step progress). */
export async function getExtractionTask(taskId) {
  const res = await fetch(`/tasks/${encodeURIComponent(taskId)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** List all extraction tasks. */
export async function listExtractionTasks() {
  const res = await fetch('/tasks');
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

// ─── Vertex Management ───────────────────────────────────────────

export async function addVertex(labels, properties = {}, graph = 'default', name = '', keywords = []) {
  return api('/vertices', {
    method: 'POST',
    headers: { 'X-Graph-Name': graph },
    body: JSON.stringify({ name, keywords, labels, properties }),
  });
}

export async function updateVertexProperties(id, labels, properties, graph = 'default', name, keywords) {
  const res = await fetch(`/vertices/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json', 'X-Graph-Name': graph },
    body: JSON.stringify({ name, keywords, labels, properties }),
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(body);
  }
  // PUT /vertices/:id returns empty body (StatusCode)
  const text = await res.text();
  return text ? JSON.parse(text) : { success: true };
}

export async function updateEdgeProperties(id, name, properties, graph = 'default', labels, keywords, strength) {
  const res = await fetch(`/edges/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json', 'X-Graph-Name': graph },
    body: JSON.stringify({ name, labels, keywords, strength, properties }),
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(body);
  }
  // PUT /edges/:id returns empty body (StatusCode)
  const text = await res.text();
  return text ? JSON.parse(text) : { success: true };
}

export async function addEdge(name, source, target, properties = {}, graph = 'default', labels, keywords, strength) {
  return api('/edges', {
    method: 'POST',
    headers: { 'X-Graph-Name': graph },
    body: JSON.stringify({ name, source, target, labels, keywords, strength, properties }),
  });
}

export async function deleteVertex(id, graph = 'default', force) {
  let url = `/vertices/${id}`;
  if (force) url += '?force=true';
  const res = await fetch(url, {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json', 'X-Graph-Name': graph },
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(body);
  }
  const text = await res.text();
  return text ? JSON.parse(text) : { success: true };
}

export async function deleteEdge(id, graph = 'default', force) {
  let url = `/edges/${id}`;
  if (force) url += '?force=true';
  const res = await fetch(url, {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json', 'X-Graph-Name': graph },
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(body);
  }
  const text = await res.text();
  return text ? JSON.parse(text) : { success: true };
}

// ─── Web Search ────────────────────────────────────────────────

export async function fetchWebSearchConfig() {
  const res = await fetch('/settings/web-search');
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function updateWebSearchConfig(config) {
  const res = await fetch('/settings/web-search', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** Search the web via the backend proxy (avoids CORS). Returns the raw response text. */
export async function searchWeb(provider, query) {
  const res = await fetch('/proxy/web-search', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query, provider_id: provider?.id }),
  });
  const data = await res.json();
  if (!data.success) throw new Error(data.error || 'search failed');
  return data.data;
}

