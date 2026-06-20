import { useState, useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import NavBar from './components/NavBar';
import SearchBar from './components/SearchBar';
import GraphViewer from './components/GraphViewer';
import PropertyPanel from './components/PropertyPanel';
import { keywordSearch, semanticSearch, getVertex } from './api';

export default function App() {
  const { t } = useTranslation();
  const [graph, setGraph] = useState('default');
  const [searchResult, setSearchResult] = useState(null);
  const [selected, setSelected] = useState(null);
  const [selectedType, setSelectedType] = useState(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    const saved = localStorage.getItem('theme');
    if (saved === 'light') {
      document.documentElement.classList.remove('dark');
    } else {
      document.documentElement.classList.add('dark');
    }
  }, []);

  const handleSearch = useCallback(async ({ mode, query, vLabel, eLabel }) => {
    setLoading(true);
    try {
      let res;
      if (mode === 'keyword') {
        res = await keywordSearch(query.split(/\s+/).filter(Boolean), graph);
      } else {
        res = await semanticSearch(query, graph);
      }
      // Apply label filters client-side if set
      if (res?.data && (vLabel || eLabel)) {
        res.data = res.data.filter(item => {
          if (item.type === 'vertex' && vLabel) return item.labels?.includes(vLabel);
          if (item.type === 'edge' && eLabel) return item.label === eLabel;
          return true;
        });
      }
      setSearchResult(res);
      setSelected(null);
    } catch (e) {
      console.error(e);
      setSearchResult({ success: false, data: [], error: e.message });
    }
    setLoading(false);
  }, [graph]);

  const handleSelect = useCallback(async (type, item) => {
    // Enrich vertex with full data
    if (type === 'vertex') {
      try {
        const res = await getVertex(item.id, graph);
        if (res?.data?.[0]) { setSelected(res.data[0]); setSelectedType('vertex'); return; }
      } catch {}
    }
    setSelected(item);
    setSelectedType(type);
  }, [graph]);

  const handleExtractDone = useCallback((res) => {
    // Refresh graph data
    if (res?.stats?.new_vertices > 0) {
      keywordSearch([], graph).then(setSearchResult).catch(() => {});
    }
  }, [graph]);

  return (
    <div className="h-screen flex flex-col bg-gray-900 text-gray-100">
      <NavBar graph={graph} setGraph={setGraph} onExtractDone={handleExtractDone} />
      <SearchBar onSearch={handleSearch} />
      {loading && (
        <div className="absolute top-20 left-1/2 -translate-x-1/2 z-10 px-4 py-2 bg-blue-600 text-white rounded shadow-lg text-sm">
          {t('graph.loading')}
        </div>
      )}
      <div className="flex-1 flex min-h-0">
        <GraphViewer data={searchResult} onSelect={handleSelect} graph={graph} />
        {selected && <PropertyPanel item={selected} type={selectedType} onClose={() => setSelected(null)} />}
      </div>
    </div>
  );
}
