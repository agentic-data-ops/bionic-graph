import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { keywordSearch, traverse } from '../api';

let visLoaded = false;
function loadVis(cb) {
  if (typeof window !== 'undefined' && window.vis) { cb(window.vis); return; }
  if (visLoaded) { setTimeout(() => loadVis(cb), 100); return; }
  visLoaded = true;
  const s = document.createElement('script');
  s.src = 'https://unpkg.com/vis-network/standalone/umd/vis-network.min.js';
  s.onload = () => cb(window.vis);
  document.head.appendChild(s);
}

export default function GraphViewer({ data, onSelect, graph }) {
  const { t } = useTranslation();
  const containerRef = useRef(null);
  const netRef = useRef(null);
  const nodesRef = useRef(null);
  const edgesRef = useRef(null);

  useEffect(() => {
    if (!data?.data?.length) return;
    const nds = [], eds = [];
    const vSet = new Set(), eSet = new Set();

    for (const item of data.data) {
      if (item.type === 'vertex' && !vSet.has(item.id)) {
        vSet.add(item.id);
        nds.push({ id: item.id, label: item.properties?.name || `#${item.id}`, group: item.labels?.[0] });
      } else if (item.type === 'edge') {
        const k = `${item.source}-${item.target}-${item.label}`;
        if (!eSet.has(k)) { eSet.add(k); eds.push({ id: item.id, from: item.source, to: item.target, label: item.label, arrows: 'to' }); }
      }
    }

    loadVis((vis) => {
      if (netRef.current) netRef.current.destroy();
      const container = containerRef.current;
      if (!container) return;
      const nd = new vis.DataSet(nds);
      const ed = new vis.DataSet(eds);
      nodesRef.current = nd;
      edgesRef.current = ed;
      const net = new vis.Network(container, { nodes: nd, edges: ed }, {
        nodes: { shape: 'dot', size: 18, font: { size: 14, color: '#e2e8f0' } },
        edges: { font: { size: 11, color: '#94a3b8' }, width: 1.5 },
        physics: { solver: 'forceAtlas2Based', stabilization: { iterations: 100 } },
        interaction: { hover: true },
      });
      net.on('click', (evt) => {
        if (evt.nodes.length) onSelect?.('vertex', nd.get(evt.nodes[0]));
        else if (evt.edges.length) onSelect?.('edge', ed.get(evt.edges[0]));
      });
      net.on('doubleClick', async (evt) => {
        if (!evt.nodes.length) return;
        const vid = evt.nodes[0];
        try {
          const res = await traverse(vid, null, graph);
          if (res?.data) for (const item of res.data) {
            if (item.type === 'vertex' && !nd.get(item.id)) nd.add({ id: item.id, label: item.properties?.name || `#${item.id}`, group: item.labels?.[0] });
            else if (item.type === 'edge') { const k = `${item.source}-${item.target}`; if (!ed.get(item.id)) ed.add({ id: item.id, from: item.source, to: item.target, label: item.label, arrows: 'to' }); }
          }
          net.fit();
        } catch (e) { console.error(e); }
      });
      netRef.current = net;
    });
  }, [data, onSelect, graph]);

  if (!data?.data?.length) return <div className="flex-1 flex items-center justify-center text-gray-400 text-lg">{t('graph.noData')}</div>;
  return <div ref={containerRef} className="flex-1 min-h-0" />;
}
