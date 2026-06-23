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

export async function createGraph(name, timeTravel = false) {
  return api('/graphs', {
    method: 'POST',
    body: JSON.stringify({ name, time_travel: timeTravel }),
  });
}

export async function deleteGraph(name) {
  return api(`/graphs/${name}`, { method: 'DELETE' });
}

export async function gremlin(steps, graph = 'default') {
  return api('/gremlin', {
    method: 'POST',
    headers: { 'X-Graph-Name': graph },
    body: JSON.stringify({ steps }),
  });
}

export async function graphSearch(keywords, graph = 'default') {
  return gremlin([{ step: 'search', keywords }], graph);
}

export async function compact(beforeTs, graph = 'default') {
  return gremlin([{ step: 'compact', before: beforeTs }], graph);
}

// ─── Sync extraction (legacy, still works) ───────────────────────

export async function extractDoc(content, graph = 'default') {
  const res = await fetch(BASE + '/extract', {
    method: 'POST',
    headers: { 'Content-Type': 'text/markdown', 'X-Graph-Name': graph },
    body: content,
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

// ─── Async extraction (task-based) ──────────────────────────────

/** Submit a markdown document for async extraction. Returns { task_id, status } */
export async function extractDocAsync(content, graph = 'default') {
  const res = await fetch(BASE + '/extract', {
    method: 'POST',
    headers: { 'Content-Type': 'text/markdown', 'X-Graph-Name': graph },
    body: content,
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** Get the status and results of an extraction task. */
export async function getTaskStatus(taskId) {
  const res = await fetch(BASE + `/extract/task/${encodeURIComponent(taskId)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** List all extraction tasks (newest first). */
export async function listExtractTasks() {
  const res = await fetch(BASE + '/extract/tasks');
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function traverse(vid, label = null, graph = 'default') {
  const edgeFilter = label ? { label } : {};
  // Fetch both neighboring vertices AND edges in a single merged response
  const [vertRes, edgeRes] = await Promise.all([
    gremlin([
      { step: 'V', ids: [vid] },
      { step: 'both', depth: 1, ...edgeFilter },
    ], graph),
    gremlin([
      { step: 'V', ids: [vid] },
      { step: 'bothE', ...edgeFilter },
    ], graph),
  ]);
  // Merge: vertices from vertRes + edges from edgeRes
  const merged = {
    success: vertRes.success && edgeRes.success,
    data: [...(vertRes.data || []), ...(edgeRes.data || [])],
    error: vertRes.error || edgeRes.error,
    ticks_used: vertRes.ticks_used,
    neurons_fired: vertRes.neurons_fired,
  };
  return merged;
}

export async function getVertex(vid, graph = 'default') {
  return gremlin([{ step: 'V', ids: [vid] }], graph);
}

export async function updateVertex(vid, props, labels, graph = 'default') {
  return gremlin([
    { step: 'V', ids: [vid] },
    { step: 'property', key: 'name', value: props.name || '' },
  ], graph);
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

export async function addDocument(title, content, keywords = [], graphName = '') {
  return api('/documents', {
    method: 'POST',
    body: JSON.stringify({ title, content, keywords, graph_name: graphName }),
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

export async function updateDocument(id, title, keywords = [], graphName) {
  return api(`/documents/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify({ title, keywords, graph_name: graphName || undefined }),
  });
}

export async function deleteDocument(id) {
  return api(`/documents/${encodeURIComponent(id)}`, { method: 'DELETE' });
}

// ─── Settings Sync ───────────────────────────────────────────────

/** Fetch full LLM settings from backend (providers, models, api_keys). */
export async function fetchSettings() {
  const res = await fetch('/settings');
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/**
 * Save full LLM providers config to backend.
 * @param {Array} providers - [{ name, api_base_url, api_key, models: ["model1", "model2"] }]
 * @param {string} defaultModel - "ProviderName/ModelName"
 */
export async function updateSettings(providers, defaultModel) {
  const res = await fetch('/settings', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      providers,
      default_model: defaultModel,
    }),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

// ─── Document Extraction (Backend Task) ─────────────────────────

/** Submit a document for background extraction. Returns { task_id, status } */
export async function startDocumentExtraction(docId, graphName) {
  const headers = {};
  if (graphName) headers['X-Graph-Name'] = graphName;
  const res = await fetch(`/documents/${encodeURIComponent(docId)}/extract`, {
    method: 'POST',
    headers,
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** Get extraction task status (with step progress). */
export async function getExtractionTask(taskId) {
  const res = await fetch(`/extract/tasks/${encodeURIComponent(taskId)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** List all extraction tasks. */
export async function listExtractionTasks() {
  const res = await fetch('/extract/tasks');
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
  return api(`/vertices/${id}`, {
    method: 'PUT',
    headers: { 'X-Graph-Name': graph },
    body: JSON.stringify({ name, keywords, labels, properties }),
  });
}

export async function updateEdgeProperties(id, label, properties, graph = 'default') {
  return api(`/edges/${id}`, {
    method: 'PUT',
    headers: { 'X-Graph-Name': graph },
    body: JSON.stringify({ label, properties }),
  });
}

export async function addEdge(label, source, target, properties = {}, graph = 'default') {
  return api('/edges', {
    method: 'POST',
    headers: { 'X-Graph-Name': graph },
    body: JSON.stringify({ label, source, target, properties }),
  });
}

export async function deleteVertex(id, graph = 'default') {
  return api(`/vertices/${id}`, {
    method: 'DELETE',
    headers: { 'X-Graph-Name': graph },
  });
}
