import { useState, useCallback, useMemo, useRef, useEffect } from 'react';
import { GraphCanvas, darkTheme } from 'reagraph';
import { traverse } from '../api';

function isWebGLSupported() {
  try {
    const c = document.createElement('canvas');
    return !!(c.getContext('webgl') || c.getContext('webgl2'));
  } catch { return false; }
}
const webglSupported = isWebGLSupported();

function toGraphData(items) {
  if (!items?.length) return { nodes: [], edges: [] };
  const nodes = [];
  const edges = [];
  const vSet = new Set();
  const eSet = new Set();
  for (const item of items) {
    if (item.type === 'vertex' && !vSet.has(item.id)) {
      vSet.add(item.id);
      nodes.push({
        id: String(item.id),
        label: item.properties?.name || `#${item.id}`,
        data: { ...item },
      });
    } else if (item.type === 'edge') {
      const key = `${item.source}-${item.target}-${item.label}`;
      if (!eSet.has(key)) {
        eSet.add(key);
        edges.push({
          id: `${item.source}->${item.target}`,
          source: String(item.source),
          target: String(item.target),
          label: item.label || '',
          data: { ...item },
        });
      }
    }
  }
  return { nodes, edges };
}

function InfoPanel({ item, type, visible, onClose }) {
  const props = item?.properties || {};
  const labels = item?.labels || [];

  return (
    <div
      className={`bg-[#1c1c20] border-l border-[#2a2a2e] flex flex-col h-full overflow-hidden flex-shrink-0 transition-all duration-300 ease-in-out ${
        visible ? 'opacity-100 translate-x-0 w-72' : 'opacity-0 translate-x-full w-0 border-l-0'
      }`}
    >
      {visible && item && <>
      <div className="flex items-center justify-between px-4 py-3 border-b border-[#2a2a2e] flex-shrink-0">
        <span className="text-xs font-semibold text-[#98989d] uppercase tracking-wider">
          {type === 'vertex' ? 'Vertex' : 'Edge'}
          <span className="text-[#48484a] font-mono ml-2 normal-case">#{item.id}</span>
        </span>
        <button className="w-5 h-5 rounded-md bg-[#2a2a2e] hover:bg-[#3a3a3e] flex items-center justify-center text-[#636366] hover:text-white" onClick={onClose}>
          <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>
      <div className="p-4 space-y-4">
        {labels.length > 0 && (
          <div>
            <div className="text-[10px] font-semibold text-[#636366] uppercase tracking-wider mb-2">Labels</div>
            <div className="flex flex-wrap gap-1.5">
              {labels.map((l, i) => (
                <span key={i} className="px-2 py-0.5 rounded-md bg-[#0a84ff]/15 text-[#0a84ff] text-[11px] font-medium">{l}</span>
              ))}
            </div>
          </div>
        )}
        <div>
          <div className="text-[10px] font-semibold text-[#636366] uppercase tracking-wider mb-2">Properties</div>
          {Object.keys(props).length === 0 ? (
            <div className="text-xs text-[#48484a] italic">—</div>
          ) : (
            <div className="space-y-1">
              {Object.entries(props).map(([k, v]) => (
                <div key={k} className="flex justify-between items-start py-1.5 px-2.5 rounded-lg bg-[#2a2a2e]">
                  <span className="text-[11px] text-[#636366] font-medium mr-3 whitespace-nowrap">{k}</span>
                  <span className="text-[11px] text-[#e5e5e7] text-right break-all max-w-[160px] font-mono">{String(v)}</span>
                </div>
              ))}
            </div>
          )}
        </div>
        {type === 'edge' && (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-xs">
              <span className="text-[#636366] font-medium w-14">source</span>
              <span className="text-[#e5e5e7] font-mono">{item.source}</span>
            </div>
            <div className="flex items-center gap-2 text-xs">
              <span className="text-[#636366] font-medium w-14">target</span>
              <span className="text-[#e5e5e7] font-mono">{item.target}</span>
            </div>
          </div>
        )}
      </div>
      </>}
    </div>
  );
}

export default function GraphViewer({ data, graph, className }) {
  const [selected, setSelected] = useState(null); // { item, type }

  const baseKeyRef = useRef(null);
  const [extraNodes, setExtraNodes] = useState([]);
  const [extraEdges, setExtraEdges] = useState([]);

  const baseData = useMemo(() => toGraphData(data?.data), [data]);

  const currentKey = data?.data?.length
    ? `${data.data.length}-${data.data[0]?.id ?? ''}`
    : null;

  useEffect(() => {
    if (baseKeyRef.current !== null && baseKeyRef.current !== currentKey) {
      setExtraNodes([]);
      setExtraEdges([]);
      setSelected(null);
    }
    baseKeyRef.current = currentKey;
  }, [currentKey]);

  const allNodes = useMemo(() => [...baseData.nodes, ...extraNodes], [baseData.nodes, extraNodes]);
  const allEdges = useMemo(() => [...baseData.edges, ...extraEdges], [baseData.edges, extraEdges]);

  // ── Click: use node.data directly (as reagraph docs suggest) ──
  const handleNodeClick = useCallback((node) => {
    setSelected({ item: node.data, type: 'vertex' });
  }, []);

  const handleEdgeClick = useCallback((edge) => {
    setSelected({ item: edge.data, type: 'edge' });
  }, []);

  // ── Double-click: expand ──
  const handleNodeDoubleClick = useCallback(async (node) => {
    try {
      const res = await traverse(Number(node.id), null, graph);
      if (!res?.data) return;
      const existingIds = new Set(allNodes.map((n) => n.id));
      const existingEdgeKeys = new Set(allEdges.map((e) => `${e.source}-${e.target}`));
      const newNodes = [];
      const newEdges = [];
      for (const item of res.data) {
        if (item.type === 'vertex' && !existingIds.has(String(item.id))) {
          newNodes.push({ id: String(item.id), label: item.properties?.name || `#${item.id}`, data: { ...item } });
        } else if (item.type === 'edge') {
          const k = `${item.source}-${item.target}`;
          if (!existingEdgeKeys.has(k)) {
            newEdges.push({ id: `${item.source}->${item.target}`, source: String(item.source), target: String(item.target), label: item.label || '', data: { ...item } });
          }
        }
      }
      if (newNodes.length > 0) setExtraNodes((prev) => [...prev, ...newNodes]);
      if (newEdges.length > 0) setExtraEdges((prev) => [...prev, ...newEdges]);
    } catch (e) {
      console.error('Expand error:', e);
    }
  }, [graph, allNodes, allEdges]);

  if (!data?.data?.length) {
    return <div className="flex-1 flex items-center justify-center text-[#636366] text-sm min-h-[200px]">No graph data</div>;
  }

  if (!webglSupported) {
    return <div className="flex-1 flex items-center justify-center text-[#48484a] text-xs min-h-[200px]">WebGL is not available. Graph visualization requires WebGL.</div>;
  }

  return (
    <div className={`flex-1 flex min-h-0 ${className || ''}`} style={{ height: '100%', width: '100%' }}>
      <div className="flex-1 min-w-0 relative" style={{ minHeight: 0 }}>
        <GraphCanvas
          nodes={allNodes}
          edges={allEdges}
          layoutType="forceDirected2d"
          theme={darkTheme}
          defaultNodeSize={8}
          minNodeSize={4}
          maxNodeSize={16}
          animated={false}
          draggable={true}
          edgeInterpolation="curved"
          edgeArrowPosition="end"
          labelType="all"
          onNodeClick={handleNodeClick}
          onEdgeClick={handleEdgeClick}
          onNodeDoubleClick={handleNodeDoubleClick}
        />
      </div>
      <InfoPanel
        item={selected?.item}
        type={selected?.type}
        visible={!!selected}
        onClose={() => setSelected(null)}
      />
    </div>
  );
}
