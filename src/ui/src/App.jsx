import { useState, useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import NavBar from './components/NavBar';
import SearchBar from './components/SearchBar';
import GraphViewer from './components/GraphViewer';
import PropertyPanel from './components/PropertyPanel';
import { keywordSearch, semanticSearch, getVertex, semanticSearchAsync, getSearchTaskStatus } from './api';

export default function App() {
  const { t } = useTranslation();
  const [graph, setGraph] = useState('default');
  const [searchResult, setSearchResult] = useState(null);
  const [selected, setSelected] = useState(null);
  const [selectedType, setSelectedType] = useState(null);
  const [loading, setLoading] = useState(false);
  const [searchTask, setSearchTask] = useState(null); // { task_id, status, steps, ... }
  const searchPollRef = useRef(null);

  useEffect(() => {
    const saved = localStorage.getItem('theme');
    if (saved === 'light') {
      document.documentElement.classList.remove('dark');
    } else {
      document.documentElement.classList.add('dark');
    }
  }, []);

  const handleSearch = useCallback(async ({ mode, query, vLabel, eLabel }) => {
    if (mode === 'keyword') {
      setLoading(true);
      try {
        let res = await keywordSearch(query.split(/\s+/).filter(Boolean), graph);
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
    } else {
      // Semantic search — async with progress
      try {
        const { task_id } = await semanticSearchAsync(query, graph);
        setSearchTask({ task_id, status: 'pending', steps: [], results: null });
        // Start polling
        if (searchPollRef.current) clearInterval(searchPollRef.current);
        searchPollRef.current = setInterval(async () => {
          try {
            const task = await getSearchTaskStatus(task_id);
            setSearchTask(task);
            if (task.status === 'completed') {
              clearInterval(searchPollRef.current);
              searchPollRef.current = null;
              let res = task.results;
              if (res?.data && (vLabel || eLabel)) {
                res.data = res.data.filter(item => {
                  if (item.type === 'vertex' && vLabel) return item.labels?.includes(vLabel);
                  if (item.type === 'edge' && eLabel) return item.label === eLabel;
                  return true;
                });
              }
              setSearchResult(res);
              setSearchTask(null);
              setSelected(null);
            } else if (task.status === 'failed') {
              clearInterval(searchPollRef.current);
              searchPollRef.current = null;
              setSearchResult({ success: false, data: [], error: task.error });
              setSearchTask(null);
            }
          } catch (e) {
            console.error('Search poll error:', e);
          }
        }, 1000);
      } catch (e) {
        console.error(e);
        setSearchResult({ success: false, data: [], error: e.message });
      }
    }
  }, [graph]);

  // Cleanup poll on unmount
  useEffect(() => {
    return () => { if (searchPollRef.current) clearInterval(searchPollRef.current); };
  }, []);

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
      {searchTask && searchTask.status !== 'completed' && searchTask.status !== 'failed' && (
        <div className="mx-4 mt-1 px-4 py-2 bg-gray-800 border border-gray-700 rounded shadow-lg">
          <div className="flex items-center gap-2 mb-1">
            <span className="text-blue-400 font-semibold text-sm">{t('nav.extract')} 搜索</span>
            <span className="text-xs text-gray-500 ml-auto">#{searchTask.task_id?.slice(0,8)}</span>
          </div>
          <div className="space-y-1">
            {(searchTask.steps || []).map((step, i) => (
              <div key={i} className="flex items-center gap-2 text-xs">
                {step.status === 'running' && <span className="w-3 h-3 rounded-full bg-blue-500 animate-pulse" />}
                {step.status === 'done' && <span className="w-3 h-3 rounded-full bg-green-500" />}
                {step.status === 'pending' && <span className="w-3 h-3 rounded-full bg-gray-600" />}
                {step.status === 'failed' && <span className="w-3 h-3 rounded-full bg-red-500" />}
                <span className={step.status === 'running' ? 'text-blue-300' : step.status === 'done' ? 'text-green-400' : 'text-gray-500'}>
                  {step.name}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
      <div className="flex-1 flex min-h-0">
        <GraphViewer data={searchResult} onSelect={handleSelect} graph={graph} />
        {selected && <PropertyPanel item={selected} type={selectedType} onClose={() => setSelected(null)} />}
      </div>
    </div>
  );
}
