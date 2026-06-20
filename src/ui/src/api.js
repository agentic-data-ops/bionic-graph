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

export async function keywordSearch(keywords, graph = 'default') {
  return gremlin([{ step: 'keywordSearch', keywords }], graph);
}

export async function semanticSearch(query, graph = 'default') {
  return gremlin([{ step: 'semanticSearch', query }], graph);
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

// ─── Async Semantic Search ──────────────────────────────────────

/** Submit a semantic search as an async background task. Returns { task_id, status } */
export async function semanticSearchAsync(query, graph = 'default') {
  const res = await fetch(BASE + '/search/semantic', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-Graph-Name': graph },
    body: JSON.stringify({ query }),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

/** Get the status and results of an async search task. */
export async function getSearchTaskStatus(taskId) {
  const res = await fetch(BASE + `/search/task/${encodeURIComponent(taskId)}`);
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
