import { useEffect, useRef, useState, forwardRef, useImperativeHandle, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Network } from 'vis-network';
import { DataSet } from 'vis-data';
import { traverse, updateVertexProperties, updateEdgeProperties, deleteVertex, deleteEdge, addVertex, addEdge, getDocument, getDocumentContent } from '../api';


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
/** Searchable vertex selector — filters visible vertices by name. */
function VertexSearchSelect({ graph, value, onChange, placeholder, disabled, nodesRef }) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState(false);
  const inputRef = useRef(null);

  /** All vertices from the vis-network DataSet (what's visible on canvas). */
  const allNodes = useMemo(() => {
    if (!nodesRef?.current) return [];
    return nodesRef.current.get().map((n) => ({
      id: n.id,
      name: n.label || `#${n.id}`,
      labels: n._original?.labels || [],
    }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodesRef?.current?.length]);

  /** Filtered results — client-side substring match on name. */
  const filtered = useMemo(() => {
    if (!query.trim()) return allNodes;
    const q = query.toLowerCase();
    return allNodes.filter((v) => {
      const label = v.name.toLowerCase();
      return label.includes(q) || String(v.id).includes(q);
    });
  }, [query, allNodes]);

  /** Currently selected vertex display info. */
  const selectedVertex = useMemo(() => {
    if (!value) return null;
    if (nodesRef?.current) {
      const n = nodesRef.current.get(value);
      if (n) return { id: n.id, label: n.label, _original: n._original };
    }
    return null;
  }, [value]);

  const handleInput = useCallback((e) => {
    setQuery(e.target.value);
    setOpen(true);
  }, []);

  const select = useCallback((vertex) => {
    onChange(vertex.id);
    setQuery('');
    setOpen(false);
    inputRef.current?.blur();
  }, [onChange]);

  const clear = useCallback(() => {
    onChange('');
    setQuery('');
    setOpen(false);
  }, [onChange]);

  const onFocus = useCallback(() => {
    setOpen(true);
  }, []);

  const onBlur = useCallback(() => {
    setTimeout(() => setOpen(false), 200);
  }, []);

  /** Render a vertex item in the dropdown list. */
  const renderVertexItem = (v) => (
    <button
      key={v.id}
      className="w-full px-3 py-2 text-left text-xs text-[var(--text-primary)] hover:bg-[var(--bg-hover)] transition-colors border-b border-[var(--border)] last:border-0"
      onMouseDown={(e) => { e.preventDefault(); select(v); }}
    >
      <span className="font-medium">{v.name || `#${v.id}`}</span>
      {v.name && <span className="text-[var(--text-muted)] ml-2">#{v.id}</span>}
      {v.labels?.length > 0 && (
        <div className="flex flex-wrap gap-1 mt-0.5">
          {v.labels.slice(0, 3).map((l, i) => (
            <span key={i} className="px-1.5 py-0.5 rounded bg-[var(--accent)]/10 text-[var(--accent)] text-[10px]">{l}</span>
          ))}
          {v.labels.length > 3 && <span className="text-[10px] text-[var(--text-muted)]">+{v.labels.length - 3}</span>}
        </div>
      )}
    </button>
  );

  return (
    <div className="relative">
      {selectedVertex ? (
        <div className="flex items-center gap-1 px-3 py-1.5 rounded-lg bg-[var(--accent)]/15 border border-[var(--accent)]/30">
          <svg className="w-3 h-3 flex-shrink-0 text-[var(--accent)]" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M9 20l-5.447-2.724A1 1 0 013 16.382V5.618a1 1 0 011.447-.894L9 7m0 13l6-3m-6 3V7m6 10l4.553 2.276A1 1 0 0021 18.382V7.618a1 1 0 00-.553-.894L15 4m0 13V4m0 0L9 7" />
          </svg>
          <span className="flex-1 text-xs text-[var(--accent)] font-medium truncate">
            {selectedVertex.label || `#${selectedVertex.id}`}
          </span>
          <button
            className="flex-shrink-0 w-4 h-4 rounded-full bg-[var(--bg-hover)] hover:bg-[var(--danger)] flex items-center justify-center text-[9px] text-[var(--text-tertiary)] hover:text-white transition-all"
            onClick={clear}
            title="Clear"
          >✕</button>
        </div>
      ) : (
        <>
          <div className="relative">
            <svg className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3 h-3 text-[var(--text-tertiary)] pointer-events-none" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
            </svg>
            <input
              ref={inputRef}
              className="w-full pl-7 pr-3 py-1.5 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
              placeholder={placeholder || 'Search or select vertex…'}
              value={query}
              onChange={handleInput}
              onFocus={onFocus}
              onBlur={onBlur}
              disabled={disabled}
            />
          </div>
          {open && (
            <div className="absolute z-[300] w-full mt-1 rounded-lg bg-[var(--bg-secondary)] border border-[var(--border)] shadow-xl max-h-48 overflow-y-auto">
              {filtered.length === 0 ? (
                <div className="px-3 py-2 text-xs text-[var(--text-tertiary)] text-center">
                  {query.trim() ? 'No matching vertices' : 'No visible vertices'}
                </div>
              ) : (
                filtered.map(renderVertexItem)
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}

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

function InfoPanel({ item, type, onClose, graphName, onDelete, onDeleteEdge, onShowDocument, onSelectVertex, graphData, nodesRef, readOnly }) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [editLabel, setEditLabel] = useState('');
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
    if (type === 'vertex') {
      setEditLabel(labels.join(', '));
    } else {
      setEditLabel(item.label || '');
    }
    setLocalName(item.name || '');
    setLocalKeywords((item.keywords || []).join(', '));
    setEditProps(Object.fromEntries(Object.entries(props).map(([k, v]) => [k, String(v)])));
    setError('');
    setEditing(true);
  }, [labels, props, type, item]);

  const cancelEdit = useCallback(() => {
    setEditing(false);
    setError('');
  }, []);

  const saveEdit = useCallback(async () => {
    setSaving(true);
    setError('');
    try {
      const newLabels = editLabel.split(',').map((s) => s.trim()).filter(Boolean);
      const newProps = Object.fromEntries(
        Object.entries(editProps).map(([k, v]) => [k, v])
      );
      const name = localName || item.name || '';
      const keywords = localKeywords.split(',').map(s => s.trim()).filter(Boolean);
      if (type === 'vertex') {
        await updateVertexProperties(item.id, newLabels, editProps, graphName, name, keywords);
      } else {
        const newLabel = editLabel.trim() || item.label || '';
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
  }, [editLabel, editProps, item, type, graphName, localName, localKeywords]);

  return (
    <div className="w-72 bg-[var(--bg-secondary)] border-l border-[var(--border)] flex flex-col h-full overflow-y-auto flex-shrink-0 select-text">
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border)] flex-shrink-0">
        <span className="text-xs font-semibold text-[var(--text-secondary)] uppercase tracking-wider">
          {type === 'vertex' ? 'Vertex' : 'Edge'}
          <span className="text-[var(--text-muted)] font-mono ml-2 normal-case">#{item.id}</span>
        </span>
        <div className="flex items-center gap-1">
          {!editing && !readOnly && (
            <>
              <button className="w-5 h-5 rounded-md bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] text-[11px]" onClick={startEdit} title={t('graph.modify')}>
                <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                </svg>
              </button>
              {type === 'vertex' ? (
                <button className="w-5 h-5 rounded-md bg-[var(--bg-tertiary)] hover:bg-[var(--danger)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)]" onClick={() => onDelete?.(item.id, item.name || item.id)} title={t('graph.delete')}>
                  <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                  </svg>
                </button>
              ) : (
                <button className="w-5 h-5 rounded-md bg-[var(--bg-tertiary)] hover:bg-[var(--danger)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)]" onClick={() => onDeleteEdge?.(item.id, item.label || item.id)} title={t('graph.delete')}>
                  <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                  </svg>
                </button>
              )}
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
              value={editLabel}
              onChange={(e) => setEditLabel(e.target.value)}
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

const GraphViewer = forwardRef(({ data, graph, className, theme, timeTravelEnabled, timeTravelAt }, ref) => {
  const containerRef = useRef(null);
  const netRef = useRef(null);
  const nodesRef = useRef(null);
  const edgesRef = useRef(null);
  const [selected, setSelected] = useState(null);
  const [showDoc, setShowDoc] = useState(null);
  const [confirmDelete, setConfirmDelete] = useState(null); // { vid, name }
  const [confirmDeleteEdge, setConfirmDeleteEdge] = useState(null); // { eid, label }
  const [searchQuery, setSearchQuery] = useState('');
  const [searchFocused, setSearchFocused] = useState(false);
  const [showAddVertex, setShowAddVertex] = useState(false);
  const [showAddEdge, setShowAddEdge] = useState(false);
  const [newVertexName, setNewVertexName] = useState('');
  const [newVertexKeywords, setNewVertexKeywords] = useState('');
  const [newVertexLabels, setNewVertexLabels] = useState('');
  const [newVertexProps, setNewVertexProps] = useState([{ k: '', v: '' }]);
  const [newEdgeLabel, setNewEdgeLabel] = useState('');
  const [newEdgeSource, setNewEdgeSource] = useState('');
  const [newEdgeTarget, setNewEdgeTarget] = useState('');
  const [newEdgeProps, setNewEdgeProps] = useState([{ k: '', v: '' }]);
  const dataRef = useRef(data);

  const searchFiltered = useMemo(() => {
    if (!nodesRef.current) return [];
    const q = searchQuery.toLowerCase();
    let results = [];
    for (const n of nodesRef.current.get()) {
      if (!q || (n.label && n.label.toLowerCase().includes(q))) {
        results.push({ type: 'vertex', id: n.id, label: n.label });
      }
    }
    if (edgesRef.current) {
      const ns = nodesRef.current;
      for (const e of edgesRef.current.get()) {
        if (!q || (e.label && e.label.toLowerCase().includes(q))) {
          const fromLabel = ns.get(e.from)?.label || `#${e.from}`;
          const toLabel = ns.get(e.to)?.label || `#${e.to}`;
          results.push({ type: 'edge', id: e.id, label: `[edge] ${e.label}`, fromLabel, toLabel });
        }
      }
    }
    return results.slice(0, 50);
  }, [searchQuery]);

  const selectSearchResult = useCallback((result) => {
    const net = netRef.current;
    const ns = nodesRef.current;
    if (!net || !ns) return;
    setSearchQuery('');
    if (result.type === 'vertex') {
      const node = ns.get(result.id);
      if (!node) return;
      const curData = dataRef.current;
      let found = null;
      if (node._original) {
        found = { item: node._original, type: 'vertex' };
      } else if (curData?.data) {
        for (const d of curData.data) {
          if (d.type === 'vertex' && d.id === result.id) { found = { item: d, type: 'vertex' }; break; }
        }
      }
      if (found) setSelected(found);
      net.selectNodes([result.id]);
      net.focus(result.id, { scale: 1.5, animation: { duration: 300, easingFunction: 'easeInOutQuad' } });
    } else {
      // Select edge — find its _original or build from DataSet
      const es = edgesRef.current;
      if (!es) return;
      const edgeData = es.get(result.id);
      if (!edgeData) return;
      if (edgeData._original) {
        setSelected({ item: edgeData._original, type: 'edge' });
      } else {
        setSelected({ item: edgeData, type: 'edge' });
      }
      net.selectEdges([result.id]);
      // Focus on the source node of the edge (vis-network focus() only works on nodes)
      if (edgeData.from) {
        net.focus(edgeData.from, { scale: 1.5, animation: { duration: 300, easingFunction: 'easeInOutQuad' } });
      }
    }
  }, []);

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
        const res = await traverse(vid, null, graph, timeTravelAt);
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

  const handleConfirmDelete = useCallback(async (force) => {
    if (!confirmDelete) return;
    const { vid, name } = confirmDelete;
    try {
      await deleteVertex(vid, graph, force);
    } catch (e) {
      console.error('Delete failed:', e);
      setConfirmDelete(null);
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
    setConfirmDelete(null);
  }, [confirmDelete, graph]);

  const handleConfirmDeleteEdge = useCallback(async (force) => {
    if (!confirmDeleteEdge) return;
    const { eid } = confirmDeleteEdge;
    try {
      await deleteEdge(eid, graph, force);
    } catch (e) {
      console.error('Delete edge failed:', e);
      setConfirmDeleteEdge(null);
      return;
    }
    const es = edgesRef.current;
    if (es) {
      es.remove(eid);
    }
    setSelected(null);
    setConfirmDeleteEdge(null);
  }, [confirmDeleteEdge, graph]);

  if (!data?.data?.length) {
    return <div className="flex items-center justify-center text-[var(--text-tertiary)] text-sm min-h-[200px]">No graph data</div>;
  }

  return (
    <div className={`flex-1 flex min-h-0 ${className || ''}`} style={{ height: '100%', width: '100%' }}>
      <div className="relative flex-1 min-h-0">
        {/* Toolbar: search + add buttons */}
        <div className="absolute top-3 left-3 z-10 flex items-start gap-2">
          <div className="w-48">
            <div className="relative">
              <svg className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3 h-3 text-[var(--text-tertiary)] pointer-events-none" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
              </svg>
              <input
                className="w-full pl-7 pr-3 py-1.5 rounded-lg bg-[var(--bg-secondary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] placeholder-[var(--text-tertiary)] shadow-lg"
                placeholder="Search graph..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onFocus={() => setSearchFocused(true)}
                onBlur={() => setTimeout(() => setSearchFocused(false), 200)}
              />
            </div>
            {searchFocused && searchFiltered.length > 0 && (
              <div className="absolute top-full left-0 right-0 mt-1 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-2xl max-h-60 overflow-y-auto z-20">
                {searchFiltered.map((r) => (
                  <button
                    key={`${r.type}-${r.id}`}
                    className="w-full text-left px-3 py-2 text-xs text-[var(--text-primary)] hover:bg-[var(--bg-hover)] transition-all flex items-center gap-2"
                    onMouseDown={() => selectSearchResult(r)}
                  >
                    <span className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${r.type === 'vertex' ? 'bg-[var(--accent)]' : 'bg-[var(--success)]'}`} />
                    <span className="truncate">{r.label}</span>
                    {r.type === 'edge' && (
                      <span className="text-[10px] text-[var(--text-tertiary)] ml-1 flex-shrink-0">{r.fromLabel} → {r.toLabel}</span>
                    )}
                    <span className="text-[var(--text-tertiary)] font-mono ml-auto flex-shrink-0">#{r.id}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
          {!timeTravelAt && (
            <div className="flex gap-1">
              <button className="px-2.5 py-1.5 rounded-lg bg-[var(--bg-secondary)] text-[var(--text-primary)] text-xs font-medium hover:bg-[var(--accent)] hover:text-white transition-all shadow-lg whitespace-nowrap"
                onClick={() => setShowAddVertex(true)}>+ Vertex</button>
              <button className="px-2.5 py-1.5 rounded-lg bg-[var(--bg-secondary)] text-[var(--text-primary)] text-xs font-medium hover:bg-[var(--accent)] hover:text-white transition-all shadow-lg whitespace-nowrap"
                onClick={() => setShowAddEdge(true)}>+ Edge</button>
            </div>
          )}
        </div>

        {/* Add Vertex Modal */}
        {showAddVertex && (
          <div className="fixed inset-0 z-[200] flex items-center justify-center" onClick={() => setShowAddVertex(false)}>
            <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
            <div className="relative bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-5 max-w-sm shadow-2xl w-80"
              onClick={(e) => e.stopPropagation()}>
              <h3 className="text-sm font-semibold text-[var(--text-primary)] mb-3">Add Vertex</h3>
              <div className="space-y-2.5">
                <input className="w-full px-3 py-1.5 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                  placeholder="Name" value={newVertexName} onChange={(e) => setNewVertexName(e.target.value)} />
                <input className="w-full px-3 py-1.5 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                  placeholder="Labels (comma-separated)" value={newVertexLabels} onChange={(e) => setNewVertexLabels(e.target.value)} />
                <input className="w-full px-3 py-1.5 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                  placeholder="Keywords (comma-separated)" value={newVertexKeywords} onChange={(e) => setNewVertexKeywords(e.target.value)} />
                <div>
                  <div className="text-[10px] text-[var(--text-tertiary)] mb-1 font-medium">Properties</div>
                  {newVertexProps.map((p, i) => (
                    <div key={i} className="flex gap-1 mb-1">
                      <input className="flex-1 px-2 py-1 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-[10px] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] font-mono"
                        placeholder="key" value={p.k} onChange={(e) => {
                          const copy = [...newVertexProps]; copy[i] = { ...copy[i], k: e.target.value }; setNewVertexProps(copy);
                        }} />
                      <input className="flex-1 px-2 py-1 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-[10px] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                        placeholder="value" value={p.v} onChange={(e) => {
                          const copy = [...newVertexProps]; copy[i] = { ...copy[i], v: e.target.value }; setNewVertexProps(copy);
                        }} />
                      {i === newVertexProps.length - 1 && (
                        <button className="px-1.5 text-[var(--text-tertiary)] hover:text-[var(--accent)] text-xs"
                          onClick={() => setNewVertexProps([...newVertexProps, { k: '', v: '' }])}>+</button>
                      )}
                    </div>
                  ))}
                </div>
              </div>
              <div className="flex gap-2 justify-end mt-4">
                <button className="px-3 py-1.5 rounded-lg bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium transition-all"
                  onClick={() => setShowAddVertex(false)}>Cancel</button>
                <button className="px-3 py-1.5 rounded-lg bg-[var(--accent)] text-white text-xs font-medium hover:opacity-80 transition-all shadow-sm"
                  onClick={async () => {
                    if (!newVertexName.trim()) return;
                    const labels = newVertexLabels.split(',').map(s => s.trim()).filter(Boolean);
                    const keywords = newVertexKeywords.split(',').map(s => s.trim()).filter(Boolean);
                    const props = Object.fromEntries(newVertexProps.filter(p => p.k.trim()).map(p => [p.k.trim(), p.v.trim()]));
                    try {
                      const res = await addVertex(labels, props, graph, newVertexName.trim(), keywords);
                      if (res.id) {
                        const ns = nodesRef.current;
                        if (ns) ns.add({ id: res.id, label: newVertexName.trim(), _original: { type: 'vertex', id: res.id, name: newVertexName.trim(), keywords, labels } });
                        netRef.current?.fit({ animation: { duration: 300 } });
                      }
                    } catch (e) { console.error('Add vertex failed:', e); }
                    setShowAddVertex(false);
                    setNewVertexName(''); setNewVertexKeywords(''); setNewVertexLabels(''); setNewVertexProps([{ k: '', v: '' }]);
                  }}>Create</button>
              </div>
            </div>
          </div>
        )}

        {/* Add Edge Modal */}
        {showAddEdge && (
          <div className="fixed inset-0 z-[200] flex items-center justify-center" onClick={() => setShowAddEdge(false)}>
            <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
            <div className="relative bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-5 max-w-sm shadow-2xl w-80"
              onClick={(e) => e.stopPropagation()}>
              <h3 className="text-sm font-semibold text-[var(--text-primary)] mb-3">Add Edge</h3>
              <div className="space-y-2.5">
                <input className="w-full px-3 py-1.5 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                  placeholder="Edge Label" value={newEdgeLabel} onChange={(e) => setNewEdgeLabel(e.target.value)} />
                <div>
                  <div className="text-[10px] text-[var(--text-tertiary)] mb-1 font-medium">Source vertex</div>
                  <VertexSearchSelect
                    graph={graph}
                    value={newEdgeSource}
                    onChange={setNewEdgeSource}
                    placeholder="Search source vertex…"
                    nodesRef={nodesRef}
                  />
                </div>
                <div>
                  <div className="text-[10px] text-[var(--text-tertiary)] mb-1 font-medium">Target vertex</div>
                  <VertexSearchSelect
                    graph={graph}
                    value={newEdgeTarget}
                    onChange={setNewEdgeTarget}
                    placeholder="Search target vertex…"
                    nodesRef={nodesRef}
                  />
                </div>
                {/* Properties */}
                <div>
                  <div className="text-[10px] text-[var(--text-tertiary)] mb-1 font-medium">Properties</div>
                  {newEdgeProps.map((p, i) => (
                    <div key={i} className="flex gap-1 mb-1">
                      <input className="flex-1 px-2 py-1 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-[10px] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] font-mono"
                        placeholder="key" value={p.k} onChange={(e) => {
                          const copy = [...newEdgeProps]; copy[i] = { ...copy[i], k: e.target.value }; setNewEdgeProps(copy);
                        }} />
                      <input className="flex-1 px-2 py-1 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-[10px] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                        placeholder="value" value={p.v} onChange={(e) => {
                          const copy = [...newEdgeProps]; copy[i] = { ...copy[i], v: e.target.value }; setNewEdgeProps(copy);
                        }} />
                      {i === newEdgeProps.length - 1 && (
                        <button className="px-1.5 text-[var(--text-tertiary)] hover:text-[var(--accent)] text-xs"
                          onClick={() => setNewEdgeProps([...newEdgeProps, { k: '', v: '' }])}>+</button>
                      )}
                    </div>
                  ))}
                </div>
              </div>
              <div className="flex gap-2 justify-end mt-4">
                <button className="px-3 py-1.5 rounded-lg bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium transition-all"
                  onClick={() => setShowAddEdge(false)}>Cancel</button>
                <button className="px-3 py-1.5 rounded-lg bg-[var(--accent)] text-white text-xs font-medium hover:opacity-80 transition-all shadow-sm"
                  onClick={async () => {
                    if (!newEdgeLabel.trim() || !newEdgeSource || !newEdgeTarget) return;
                    try {
                      const src = parseInt(newEdgeSource);
                      const tgt = parseInt(newEdgeTarget);
                      const props = Object.fromEntries(newEdgeProps.filter(p => p.k.trim()).map(p => [p.k.trim(), p.v.trim()]));
                      const res = await addEdge(newEdgeLabel.trim(), src, tgt, props, graph);
                      if (res.id) {
                        const es = edgesRef.current;
                        if (es) {
                          const exists = es.get({ filter: (e) => e.from === src && e.to === tgt });
                          if (exists.length === 0) es.add({ id: res.id, from: src, to: tgt, label: newEdgeLabel.trim(), _original: { type: 'edge', id: res.id, label: newEdgeLabel.trim(), source: src, target: tgt, properties: props } });
                        }
                        netRef.current?.fit({ animation: { duration: 300 } });
                      }
                    } catch (e) { console.error('Add edge failed:', e); }
                    setShowAddEdge(false);
                    setNewEdgeLabel(''); setNewEdgeSource(''); setNewEdgeTarget(''); setNewEdgeProps([{ k: '', v: '' }]);
                  }}>Create</button>
              </div>
            </div>
          </div>
        )}

        <div ref={containerRef} className="w-full h-full" />
      </div>
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
          onDelete={(vid, name) => setConfirmDelete({ vid, name })}
          onDeleteEdge={(eid, label) => setConfirmDeleteEdge({ eid, label })}
          readOnly={!!timeTravelAt}
        />
      )}
      {showDoc && <DocViewer docId={showDoc} onClose={() => setShowDoc(null)} />}

      {/* Edge delete confirmation modal */}
      {confirmDeleteEdge && (
        <DeleteConfirmModal
          vid={confirmDeleteEdge.eid}
          name={confirmDeleteEdge.label}
          timeTravelEnabled={timeTravelEnabled}
          onConfirm={(force) => handleConfirmDeleteEdge(force)}
          onCancel={() => setConfirmDeleteEdge(null)}
          isEdge={true}
        />
      )}

      {/* Delete confirmation modal */}
      {confirmDelete && (
        <DeleteConfirmModal
          vid={confirmDelete.vid}
          name={confirmDelete.name}
          timeTravelEnabled={timeTravelEnabled}
          onConfirm={(force) => handleConfirmDelete(force)}
          onCancel={() => setConfirmDelete(null)}
        />
      )}
    </div>
  );
});

/** Delete confirmation dialog with optional hard-delete checkbox. */
function DeleteConfirmModal({ vid, name, timeTravelEnabled, onConfirm, onCancel, isEdge }) {
  const { t } = useTranslation();
  const [hardDelete, setHardDelete] = useState(false);

  return (
    <div className="fixed inset-0 z-[200] flex items-center justify-center" onClick={onCancel}>
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
      <div className="relative bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-6 max-w-sm shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="text-sm font-semibold text-[var(--text-primary)] mb-3 tracking-tight">
          {isEdge ? t('graph.deleteEdge') || 'Delete Edge' : t('graph.deleteVertex')}
        </h3>
        <p className="text-xs text-[var(--text-secondary)] leading-relaxed mb-4">
          {isEdge
            ? `Delete edge #${vid} (${name || ''})?`
            : t('graph.confirmDelete', { id: vid, name: name || vid })}
        </p>

        {timeTravelEnabled && (
          <label className="flex items-center gap-2 cursor-pointer select-none mb-4 px-1">
            <input
              type="checkbox"
              checked={hardDelete}
              onChange={(e) => setHardDelete(e.target.checked)}
              className="w-3.5 h-3.5 rounded border-[var(--border)] bg-[var(--bg-tertiary)] checked:bg-[var(--danger)] checked:border-[var(--danger)] focus:ring-0 cursor-pointer"
            />
            <span className="text-xs text-[var(--text-secondary)]">{t('graph.hardDelete')}</span>
          </label>
        )}

        <div className="flex gap-2 justify-end">
          <button
            className="px-4 py-1.5 rounded-lg bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium transition-all"
            onClick={onCancel}
          >{t('panel.close')}</button>
          <button
            className="px-4 py-1.5 rounded-lg bg-[var(--danger)] text-white text-xs font-medium hover:opacity-80 transition-all shadow-sm"
            onClick={() => onConfirm(hardDelete)}
          >{t('graph.delete')}</button>
        </div>
      </div>
    </div>
  );
}

export default GraphViewer;
