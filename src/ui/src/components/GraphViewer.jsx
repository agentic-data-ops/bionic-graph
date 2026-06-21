import { useEffect, useRef, useState, forwardRef, useImperativeHandle } from 'react';
import { Network } from 'vis-network';
import { DataSet } from 'vis-data';
import { traverse } from '../api';

const DARK_OPTIONS = {
  nodes: {
    shape: 'dot', size: 18,
    font: { face: '-apple-system, BlinkMacSystemFont, "SF Pro Text", Helvetica, Arial, sans-serif', size: 13, color: '#e5e5e7', strokeWidth: 3, strokeColor: '#1a1a1e' },
    color: { background: '#3a3a3e', border: '#4a4a4e', highlight: { background: '#0a84ff', border: '#0a84ff' }, hover: { background: '#4a4a4e', border: '#5a5a5e' } },
    borderWidth: 1.5, borderWidthSelected: 2,
    shadow: { enabled: true, color: 'rgba(0,0,0,0.3)', size: 6, x: 0, y: 2 },
  },
  edges: {
    width: 1.2,
    color: { color: '#3a3a3e', highlight: '#0a84ff', hover: '#4a4a4e' },
    font: { face: '-apple-system, BlinkMacSystemFont, "SF Pro Text", Helvetica, Arial, sans-serif', size: 10, color: '#636366', strokeWidth: 2, strokeColor: '#1c1c20', align: 'middle' },
    smooth: { type: 'curvedCW', roundness: 0.15 },
    arrows: { to: { enabled: true, scaleFactor: 0.6 } },
  },
  physics: {
    solver: 'forceAtlas2Based',
    forceAtlas2Based: { gravitationalConstant: -40, centralGravity: 0.005, springLength: 180, springConstant: 0.02 },
    stabilization: { iterations: 100 },
  },
  interaction: { hover: true, tooltipDelay: 200, zoomView: true, dragView: true },
  layout: { randomSeed: 42 },
};

function InfoPanel({ item, type, onClose }) {
  if (!item) return null;
  const props = item.properties || {};
  const labels = item.labels || [];
  return (
    <div className="w-72 bg-[#1c1c20] border-l border-[#2a2a2e] flex flex-col h-full overflow-y-auto flex-shrink-0 select-text">
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
              {labels.map((l, i) => <span key={i} className="px-2 py-0.5 rounded-md bg-[#0a84ff]/15 text-[#0a84ff] text-[11px] font-medium">{l}</span>)}
            </div>
          </div>
        )}
        <div>
          <div className="text-[10px] font-semibold text-[#636366] uppercase tracking-wider mb-2">Properties</div>
          {Object.keys(props).length === 0 ? <div className="text-xs text-[#48484a] italic">—</div> : (
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
            <div className="flex items-center gap-2 text-xs"><span className="text-[#636366] font-medium w-14">source</span><span className="text-[#e5e5e7] font-mono">{item.source}</span></div>
            <div className="flex items-center gap-2 text-xs"><span className="text-[#636366] font-medium w-14">target</span><span className="text-[#e5e5e7] font-mono">{item.target}</span></div>
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
      nodes.push({ id: item.id, label: item.properties?.name || `#${item.id}`, _original: item });
    } else if (item.type === 'edge') {
      const key = `${item.source}-${item.target}`;
      if (!eSet.has(key)) { eSet.add(key); edges.push({ id: item.id, from: item.source, to: item.target, label: item.label || '', _original: item }); }
    }
  }
  return { nodes, edges };
}

const GraphViewer = forwardRef(({ data, graph, className }, ref) => {
  const containerRef = useRef(null);
  const netRef = useRef(null);
  const nodesRef = useRef(null);
  const edgesRef = useRef(null);
  const [selected, setSelected] = useState(null);
  const dataRef = useRef(data); // track latest data for event handlers

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

    const { nodes: nds, edges: eds } = buildFromData(data.data);
    const nodes = new DataSet(nds);
    const edges = new DataSet(eds);
    nodesRef.current = nodes;
    edgesRef.current = edges;

    netRef.current?.destroy();
    const container = containerRef.current;
    if (!container) return;

    const net = new Network(container, { nodes, edges }, DARK_OPTIONS);

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
            nodes.add({ id: item.id, label: item.properties?.name || `#${item.id}`, _original: item });
          } else if (item.type === 'edge') {
            const existing = edges.get({ filter: (e) => e.from === item.source && e.to === item.target });
            if (existing.length === 0) edges.add({ id: item.id, from: item.source, to: item.target, label: item.label || '', _original: item });
          }
        }
        net.fit({ animation: { duration: 300, easingFunction: 'easeInOutQuad' } });
      } catch (e) { console.error('Expand error:', e); }
    });

    netRef.current = net;
  }, [data, graph]);

  if (!data?.data?.length) {
    return <div className="flex items-center justify-center text-[#636366] text-sm min-h-[200px]">No graph data</div>;
  }

  return (
    <div className={`flex-1 flex min-h-0 ${className || ''}`} style={{ height: '100%', width: '100%' }}>
      <div ref={containerRef} className="flex-1 min-h-0" />
      {selected && <InfoPanel item={selected.item} type={selected.type} onClose={() => setSelected(null)} />}
    </div>
  );
});

export default GraphViewer;
