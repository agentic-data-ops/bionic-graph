import { useEffect, useRef, useState, forwardRef, useImperativeHandle, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Network } from 'vis-network';
import { DataSet } from 'vis-data';
import { traverse, updateVertexProperties, updateEdgeProperties, deleteVertex, getDocument, getDocumentContent } from '../api';


const DARK_OPTS = {
  nodes: { shape: 'dot', size: 18, font: { face: '-apple-system, BlinkMacSystemFont, "SF Pro Text", Helvetica, Arial, sans-serif', size: 13, color: '#e5e5e7', strokeWidth: 3, strokeColor: '#1a1a1e' }, color: { background: '#3a3a3e', border: '#4a4a4e', highlight: { background: '#0a84ff', border: '#0a84ff' }, hover: { background: '#4a4a4e', border: '#5a5a5e' } }, borderWidth: 1.5, borderWidthSelected: 2, shadow: { enabled: true, color: 'rgba(0,0,0,0.3)', size: 6, x: 0, y: 2 } },
  edges: { width: 1.2, color: { color: '#3a3a3e', highlight: '#0a84ff', hover: '#4a4a4e' }, font: { face: '-apple-system, BlinkMacSystemFont, "SF Pro Text", Helvetica, Arial, sans-serif', size: 10, color: '#636366', strokeWidth: 2, strokeColor: '#1c1c20', align: 'middle' }, smooth: { type: 'curvedCW', roundness: 0.15 }, arrows: { to: { enabled: true, scaleFactor: 0.6 } } },
  physics: { solver: 'forceAtlas2Based', forceAtlas2Based: { gravitationalConstant: -40, centralGravity: 0.005, springLength: 180, springConstant: 0.02 }, stabilization: { iterations: 100 } },
  interaction: { hover: true, tooltipDelay: 200, zoomView: true, dragView: true },
  layout: { randomSeed: 42 },
};
const LIGHT_OPTS = {
  nodes: { shape: 'dot', size: 18, font: { face: '-apple-system, BlinkMacSystemFont, "SF Pro Text", Helvetica, Arial, sans-serif', size: 13, color: '#1d1d1f', strokeWidth: 0, strokeColor: '#ffffff' }, color: { background: '#e8e8ed', border: '#d1d1d6', highlight: { background: '#0a84ff', border: '#0a84ff' }, hover: { background: '#d1d1d6', border: '#aeaeb2' } }, borderWidth: 1.5, borderWidthSelected: 2, shadow: { enabled: false, color: 'rgba(0,0,0,0.08)', size: 3, x: 0, y: 1 } },
  edges: { width: 1.2, color: { color: '#aeaeb2', highlight: '#0a84ff', hover: '#8e8e93' }, font: { face: '-apple-system, BlinkMacSystemFont, "SF Pro Text", Helvetica, Arial, sans-serif', size: 10, color: '#636366', strokeWidth: 3, strokeColor: '#ffffff', align: 'middle' }, smooth: { type: 'curvedCW', roundness: 0.15 }, arrows: { to: { enabled: true, scaleFactor: 0.6 } } },
  physics: { solver: 'forceAtlas2Based', forceAtlas2Based: { gravitationalConstant: -40, centralGravity: 0.005, springLength: 180, springConstant: 0.02 }, stabilization: { iterations: 100 } },
  interaction: { hover: true, tooltipDelay: 200, zoomView: true, dragView: true },
  layout: { randomSeed: 42 },
};
/** Document detail dialog — shows name, tags, and markdown content. */
function DocViewer({ docId, onClose }) {
  const { t } = useTranslation();
  const [doc, setDoc] = useState(null);
  const [content, setContent] = useState('');
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!docId) return;
    setLoading(true);
    Promise.all([
      getDocument(docId).catch(() => null),
      getDocumentContent(docId).catch(() => ''),
    ]).then(([d, c]) => {
      setDoc(d);
      setContent(c || '');
      setLoading(false);
    });
  }, [docId]);

  return (
    <div className="fixed inset-0 z-[200] flex items-center justify-center" onClick={onClose}>
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
      <div className="relative bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-6 max-w-2xl max-h-[80vh] overflow-y-auto shadow-2xl min-w-[500px]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-4">
          <span className="text-sm font-semibold text-[var(--text-primary)] tracking-tight">
            {loading ? '加载中...' : (doc?.title || '文档')}
          </span>
          <button className="w-7 h-7 rounded-lg bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-all" onClick={onClose}>
            <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
        {loading ? (
          <div className="text-xs text-[var(--text-tertiary)] text-center py-8">加载中...</div>
        ) : (
          <div className="space-y-4">
            {doc?.tags?.length > 0 && (
              <div className="flex flex-wrap gap-1.5">
                {doc.tags.map((tag, i) => (
                  <span key={i} className="px-2 py-0.5 rounded-md bg-[var(--accent)]/15 text-[var(--accent)] text-[11px] font-medium">{tag}</span>
                ))}
              </div>
            )}
            <div className="text-xs text-[var(--text-primary)] font-mono break-words whitespace-pre-wrap max-h-96 overflow-y-auto border border-[var(--border)] rounded-xl p-4 bg-[var(--bg-tertiary)] leading-relaxed">{content}</div>
          </div>
        )}
      </div>
    </div>
  );
}

function InfoPanel({ item, type, onClose, graphName, onDelete, onShowDocument, onSelectVertex, graphData, nodesRef }) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [editLabels, setEditLabels] = useState('');
  const [editProps, setEditProps] = useState({});
  const [localName, setLocalName] = useState("");
  const [localKeywords, setLocalKeywords] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [docName, setDocName] = useState('');
  const [docIdState, setDocIdState] = useState('');

  // Fetch document name when document ID changes
  useEffect(() => {
    const docId = item?.document;
    if (!docId || docId === docIdState) return;
    setDocIdState(docId);
    getDocument(docId).then((doc) => {
      if (doc && doc.title) setDocName(doc.title);
    }).catch(() => {});
  }, [item?.document]);
  const [newPropKey, setNewPropKey] = useState('');
  const [newPropVal, setNewPropVal] = useState('');

  if (!item) return null;
  const props = item.properties || {};
  const labels = item.labels || [];

  const startEdit = useCallback(() => {
    setEditLabels(labels.join(', '));
    setLocalName(item.name || '');
    setLocalKeywords((item.keywords || []).join(', '));
    setEditProps(Object.fromEntries(Object.entries(props).map(([k, v]) => [k, String(v)])));
    setError('');
    setEditing(true);
  }, [labels, props]);

  const cancelEdit = useCallback(() => {
    setEditing(false);
    setError('');
  }, []);

  const saveEdit = useCallback(async () => {
    setSaving(true);
    setError('');
    try {
      const newLabels = editLabels.split(',').map((s) => s.trim()).filter(Boolean);
      const newProps = Object.fromEntries(
        Object.entries(editProps).map(([k, v]) => [k, v])
      );
      const name = localName || item.name || '';
      const keywords = localKeywords.split(',').map(s => s.trim()).filter(Boolean);
      if (type === 'vertex') {
        await updateVertexProperties(item.id, newLabels, editProps, graphName, name, keywords);
      } else {
        const newLabel = newLabels[0] || item.label || '';
        await updateEdgeProperties(item.id, newLabel, editProps, graphName);
      }
      item.labels = newLabels;
      item.properties = editProps;
      item.name = name;
      item.keywords = keywords;
      setEditing(false);
    } catch (e) {
      setError(e.message || 'Save failed');
    }
    setSaving(false);
  }, [editLabels, editProps, item, type, graphName, localName, localKeywords]);

  return (
    <div className="w-72 bg-[var(--bg-secondary)] border-l border-[var(--border)] flex flex-col h-full overflow-y-auto flex-shrink-0 select-text">
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border)] flex-shrink-0">
        <span className="text-xs font-semibold text-[var(--text-secondary)] uppercase tracking-wider">
          {type === 'vertex' ? 'Vertex' : 'Edge'}
          <span className="text-[var(--text-muted)] font-mono ml-2 normal-case">#{item.id}</span>
        </span>
        <div className="flex items-center gap-1">
          {!editing && type === 'vertex' && (
            <>
              <button className="w-5 h-5 rounded-md bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] text-[11px]" onClick={startEdit} title={t('graph.modify')}>
                <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                </svg>
              </button>
              <button className="w-5 h-5 rounded-md bg-[var(--bg-tertiary)] hover:bg-[var(--danger)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)]" onClick={() => { if (confirm(t('graph.confirmDelete', { id: item.id, name: item.name || item.id }))) onDelete?.(item.id); }} title={t('graph.delete')}>
                <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                </svg>
              </button>
            </>
          )}
          <button className="w-5 h-5 rounded-md bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)]" onClick={onClose}>
            <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
      </div>
      <div className="p-4 space-y-4">
        {/* Labels (vertex) / Label (edge) */}
        <div>
          <div className="text-[10px] font-semibold text-[var(--text-tertiary)] uppercase tracking-wider mb-2">
            {type === 'edge' ? 'Label' : 'Labels'} {editing && <span className="text-[var(--text-muted)] normal-case font-normal">(comma-separated)</span>}
          </div>
          {type === 'edge' ? (
            <div className="text-xs text-[var(--text-primary)] font-medium">{item.label || '—'}</div>
          ) : editing ? (
            <input
              className="w-full px-2.5 py-1.5 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
              value={editLabels}
              onChange={(e) => setEditLabels(e.target.value)}
            />
          ) : (
            <div className="flex flex-wrap gap-1.5">
              {labels.map((l, i) => <span key={i} className="px-2 py-0.5 rounded-md bg-[var(--accent)]/15 text-[var(--accent)] text-[11px] font-medium">{l}</span>)}
            </div>
          )}
        </div>
        {/* Name (built-in) — vertices only */}
        {type === 'vertex' && (
        <div>
          <div className="text-[10px] font-semibold text-[var(--text-tertiary)] uppercase tracking-wider mb-2">Name</div>
          {editing ? (
            <input
              className="w-full px-2.5 py-1.5 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
              value={localName}
              onChange={(e) => setLocalName(e.target.value)}
            />
          ) : (
            <div className="text-xs text-[var(--text-primary)] font-medium">{item.name || '—'}</div>
          )}
        </div>
        )}
        {/* Keywords (built-in) — vertices only */}
        {type === 'vertex' && (
        <div>
          <div className="text-[10px] font-semibold text-[var(--text-tertiary)]  uppercase tracking-wider mb-2">Keywords</div>
          {editing ? (
            <input
              className="w-full px-2.5 py-1.5 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
              value={localKeywords}
              onChange={(e) => setLocalKeywords(e.target.value)}
              placeholder="comma-separated"
            />
          ) : (
            <div className="flex flex-wrap gap-1.5">
              {(item.keywords || []).length > 0 ? item.keywords.map((tag, i) => (
                <span key={i} className="px-2 py-0.5 rounded-md bg-[var(--success-bg)] text-[var(--success)] text-[11px] font-medium">{tag}</span>
              )) : <span className="text-xs text-[var(--text-muted)] italic">—</span>}
            </div>
          )}
        </div>
        )}
        {/* Edge source/target — clickable vertex links */}
        {type === 'edge' && (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-xs">
              <span className="text-[var(--text-tertiary)] font-medium w-14">SOURCE</span>
              <button className="text-[var(--accent)] hover:underline font-mono text-xs text-left"
                onClick={() => onSelectVertex?.(item.source)}>
                {(() => {
                  const allData = graphData?.data || [];
                  let v = allData.find(d => d.type === 'vertex' && d.id === item.source);
                  if (!v && nodesRef?.current) {
                    const node = nodesRef.current.get(item.source);
                    v = node?._original;
                  }
                  return v ? v.name || `#${item.source}` : `#${item.source}`;
                })()}
              </button>
            </div>
            <div className="flex items-center gap-2 text-xs">
              <span className="text-[var(--text-tertiary)] font-medium w-14">TARGET</span>
              <button className="text-[var(--accent)] hover:underline font-mono text-xs text-left"
                onClick={() => onSelectVertex?.(item.target)}>
                {(() => {
                  const allData = graphData?.data || [];
                  let v = allData.find(d => d.type === 'vertex' && d.id === item.target);
                  if (!v && nodesRef?.current) {
                    const node = nodesRef.current.get(item.target);
                    v = node?._original;
                  }
                  return v ? v.name || `#${item.target}` : `#${item.target}`;
                })()}
              </button>
            </div>
          </div>
        )}
        {/* Document (clickable link) */}
        {item.document && (
          <div>
            <div className="text-[10px] font-semibold text-[var(--text-tertiary)] uppercase tracking-wider mb-2">Document</div>
            <button
              className="text-xs text-[var(--accent)] hover:underline text-left break-all"
              onClick={() => onShowDocument?.(item.document)}
            >{docName || `#${item.document.slice(0,8)}…`}</button>
          </div>
        )}
        {/* Custom Properties */}
        <div>
          <div className="text-[10px] font-semibold text-[var(--text-tertiary)] uppercase tracking-wider mb-2">Custom Properties</div>
          {editing ? (
            <div className="space-y-1.5">
              {Object.entries(editProps).map(([k, v], idx) => (
                <div key={idx} className="flex items-start gap-1 py-1.5 px-2.5 rounded-lg bg-[var(--bg-tertiary)]">
                  <div className="flex-1 flex flex-col gap-1 min-w-0">
                    <input
                      className="w-full px-2 py-1 rounded-md bg-[var(--bg-secondary)] text-[var(--text-primary)] text-[10px] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] font-mono"
                      value={k}
                      onChange={(e) => {
                        const { [k]: _, ...rest } = editProps;
                        setEditProps({ ...rest, [e.target.value]: v });
                      }}
                      placeholder="key"
                    />
                    <input
                      className="w-full px-2 py-1 rounded-md bg-[var(--bg-secondary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                      value={v}
                      onChange={(e) => setEditProps({ ...editProps, [k]: e.target.value })}
                    />
                  </div>
                  <button
                    className="flex-shrink-0 w-5 h-5 rounded-md bg-[var(--bg-hover)] hover:bg-[var(--danger)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] text-[10px] mt-1"
                    onClick={() => { const { [k]: _, ...rest } = editProps; setEditProps(rest); }}
                  >✕</button>
                </div>
              ))}
              {Object.keys(editProps).length === 0 && <div className="text-xs text-[var(--text-muted)] italic">No properties</div>}
              <button
                className="w-full py-1 rounded-lg border border-dashed border-[#3a3a3e] text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:border-[#0a84ff] text-xs font-medium transition-all"
                onClick={() => {
                  const key = newPropKey || 'key' + (Object.keys(editProps).length + 1);
                  setEditProps({ ...editProps, [key]: newPropVal || '' });
                  setNewPropKey('');
                  setNewPropVal('');
                }}
              >+ {t('graph.addProperty')}</button>
            </div>
          ) : (
            <>
              {Object.keys(props).length === 0 ? <div className="text-xs text-[var(--text-muted)] italic">—</div> : (
                <div className="space-y-1">
                  {Object.entries(props).map(([k, v]) => (
                    <div key={k} className="flex justify-between items-start py-1.5 px-2.5 rounded-lg bg-[var(--bg-tertiary)]">
                      <span className="text-[11px] text-[var(--text-tertiary)] font-medium mr-3 whitespace-nowrap">{k}</span>
                      <span className="text-[11px] text-[var(--text-primary)] text-right break-all max-w-[160px] font-mono">{String(v)}</span>
                    </div>
                  ))}
                </div>
              )}
            </>
          )}
        </div>


        {/* Edit buttons */}
        {editing && (
          <div className="space-y-2">
            {error && <div className="text-[11px] text-[var(--danger)] bg-[#3a2a2e] rounded-lg px-2.5 py-1.5">{error}</div>}
            <div className="flex gap-2">
              <button className="flex-1 py-1.5 rounded-lg bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium transition-all" onClick={cancelEdit}>Cancel</button>
              <button className="flex-1 py-1.5 rounded-lg bg-[var(--accent)] text-white text-xs font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm disabled:opacity-50" onClick={saveEdit} disabled={saving}>
                {saving ? 'Saving...' : 'Save'}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * Build initial nodes/edges from API data, preserving _original for snapshot.
 */
function buildFromData(dataItems) {
  const nodes = [], edges = [];
  const vSet = new Set(), eSet = new Set();
  if (!dataItems) return { nodes, edges };
  for (const item of dataItems) {
    if (item.type === 'vertex' && !vSet.has(item.id)) {
      vSet.add(item.id);
      nodes.push({ id: item.id, label: item.name || `#${item.id}`, _original: item });
    } else if (item.type === 'edge') {
      const key = `${item.source}-${item.target}`;
      if (!eSet.has(key)) { eSet.add(key); edges.push({ id: item.id, from: item.source, to: item.target, label: item.label || '', _original: item }); }
    }
  }
  return { nodes, edges };
}

const GraphViewer = forwardRef(({ data, graph, className, theme }, ref) => {
  const containerRef = useRef(null);
  const netRef = useRef(null);
  const nodesRef = useRef(null);
  const edgesRef = useRef(null);
  const [selected, setSelected] = useState(null);
  const [showDoc, setShowDoc] = useState(null);
  const dataRef = useRef(data);

  const selectVertex = useCallback((vid) => {
    const net = netRef.current;
    const ns = nodesRef.current;
    if (!net || !ns) return;
    const node = ns.get(vid);
    if (!node) return;
    const curData = dataRef.current;
    let found = null;
    if (node._original) {
      found = { item: node._original, type: 'vertex' };
    } else {
      found = null;
      for (const d of (curData?.data || [])) {
        if (d.type === 'vertex' && d.id === vid) { found = { item: d, type: 'vertex' }; break; }
      }
    }
    if (found) setSelected(found);
    net.selectNodes([vid]);
    net.focus(vid, { scale: 1.5, animation: { duration: 300, easingFunction: 'easeInOutQuad' } });
  }, []); // track latest data for event handlers

  useEffect(() => { dataRef.current = data; }, [data]);

  useImperativeHandle(ref, () => ({
    getSnapshot: () => {
      if (!nodesRef.current) return null;
      return {
        nodes: nodesRef.current.get().map((n) => ({ id: n.id, label: n.label, _original: n._original })),
        edges: edgesRef.current?.get().map((e) => ({ id: e.id, from: e.from, to: e.to, label: e.label, _original: e._original })) || [],
      };
    },
    /** Merge snapshot data into current DataSet (add missing, skip existing). */
    applySnapshot: (snapshot) => {
      if (!snapshot || !nodesRef.current) return;
      for (const n of snapshot.nodes || []) {
        if (!nodesRef.current.get(n.id)) {
          nodesRef.current.add({ id: n.id, label: n.label, _original: n._original });
        }
      }
      for (const e of snapshot.edges || []) {
        const existing = edgesRef.current?.get({ filter: (edge) => edge.from === e.from && edge.to === e.to }) || [];
        if (existing.length === 0) {
          edgesRef.current?.add({ id: e.id, from: e.from, to: e.to, label: e.label, _original: e._original });
        }
      }
      netRef.current?.fit({ animation: { duration: 300, easingFunction: 'easeInOutQuad' } });
    },
  }), []);

  useEffect(() => {
    return () => { netRef.current?.destroy(); };
  }, []);

  useEffect(() => {
    if (!data?.data?.length) {
      netRef.current?.destroy();
      netRef.current = null;
      return;
    }

    const container = containerRef.current;
    if (!container) return;

    // Ensure container has dimensions before network creation
    // The parent h-[420px] in MessageList provides the height constraint

    const { nodes: nds, edges: eds } = buildFromData(data.data);
    const nodes = new DataSet(nds);
    const edges = new DataSet(eds);
    nodesRef.current = nodes;
    edgesRef.current = edges;

    netRef.current?.destroy();

    const isLight = (theme || "dark") === "light";
    const net = new Network(container, { nodes, edges }, isLight ? LIGHT_OPTS : DARK_OPTS);
    // Fit view immediately (no animation to avoid interaction interference)
    net.fit({ animation: false });
    net.on('click', (evt) => {
      const curData = dataRef.current;
      if (evt.nodes.length) {
        const nodeData = nodes.get(evt.nodes[0]);
        // Prefer _original if available, else search data.data
        if (nodeData._original) { setSelected({ item: nodeData._original, type: 'vertex' }); return; }
        for (const item of (curData?.data || [])) {
          if (item.type === 'vertex' && item.id === nodeData.id) { setSelected({ item, type: 'vertex' }); return; }
        }
      } else if (evt.edges.length) {
        const edgeData = edges.get(evt.edges[0]);
        if (edgeData._original) { setSelected({ item: edgeData._original, type: 'edge' }); return; }
        for (const item of (curData?.data || [])) {
          if (item.type === 'edge' && item.id === edgeData.id) { setSelected({ item, type: 'edge' }); return; }
        }
      } else {
        setSelected(null);
      }
    });

    net.on('doubleClick', async (evt) => {
      if (!evt.nodes.length) return;
      const vid = evt.nodes[0];
      try {
        const res = await traverse(vid, null, graph);
        if (!res?.data) return;
        for (const item of res.data) {
          if (item.type === 'vertex' && !nodes.get(item.id)) {
            nodes.add({ id: item.id, label: item.name || `#${item.id}`, _original: item });
          } else if (item.type === 'edge') {
            const existing = edges.get({ filter: (e) => e.from === item.source && e.to === item.target });
            if (existing.length === 0) edges.add({ id: item.id, from: item.source, to: item.target, label: item.label || '', _original: item });
          }
        }
        net.fit({ animation: { duration: 300, easingFunction: 'easeInOutQuad' } });
      } catch (e) { console.error('Expand error:', e); }
    });

    netRef.current = net;
  }, [data, graph, theme]);

  if (!data?.data?.length) {
    return <div className="flex items-center justify-center text-[var(--text-tertiary)] text-sm min-h-[200px]">No graph data</div>;
  }

  return (
    <div className={`flex-1 flex min-h-0 ${className || ''}`} style={{ height: '100%', width: '100%' }}>
      <div ref={containerRef} className="flex-1 min-h-0" />
      {selected && (
        <InfoPanel
          item={selected.item}
          type={selected.type}
          graphName={graph}
          graphData={data}
          nodesRef={nodesRef}
          onClose={() => setSelected(null)}
          onShowDocument={(docId) => setShowDoc(docId)}
          onSelectVertex={selectVertex}
          onDelete={async (vid) => {
            try {
              await deleteVertex(vid, graph);
            } catch (e) {
              console.error('Delete failed:', e);
              return;
            }
            const ns = nodesRef.current;
            const es = edgesRef.current;
            if (!ns) return;
            if (es) {
              const toRemove = es.get().filter((e) => e.from === vid || e.to === vid);
              toRemove.forEach((e) => es.remove(e.id));
            }
            ns.remove(vid);
            setSelected(null);
          }}
        />
      )}
      {showDoc && <DocViewer docId={showDoc} onClose={() => setShowDoc(null)} />}
    </div>
  );
});

export default GraphViewer;
