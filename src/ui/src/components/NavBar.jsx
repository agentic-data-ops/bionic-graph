import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { listGraphs, createGraph, compact, extractDoc } from '../api';

function Modal({ title, children, onClose }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={onClose}>
      <div className="bg-gray-800 rounded-lg p-6 min-w-96 max-w-lg" onClick={(e) => e.stopPropagation()}>
        <div className="text-lg font-semibold text-white mb-4">{title}</div>
        {children}
      </div>
    </div>
  );
}

export default function NavBar({ graph, setGraph, onExtractDone }) {
  const { t, i18n } = useTranslation();
  const [graphs, setGraphs] = useState([]);
  const [theme, setTheme] = useState(() => localStorage.getItem('theme') || 'dark');
  const [showAdd, setShowAdd] = useState(false);
  const [showCompact, setShowCompact] = useState(false);
  const [showExtract, setShowExtract] = useState(false);
  const [extractContent, setExtractContent] = useState('');
  const [extractResult, setExtractResult] = useState(null);
  const [compactDays, setCompactDays] = useState('7');
  const [newName, setNewName] = useState('');
  const [newTT, setNewTT] = useState(false);

  useEffect(() => { listGraphs().then(d => setGraphs(d.graphs || [])).catch(() => {}); }, []);

  const toggleTheme = () => {
    const next = theme === 'dark' ? 'light' : 'dark';
    setTheme(next);
    localStorage.setItem('theme', next);
    document.documentElement.classList.toggle('dark', next === 'dark');
  };

  const switchLang = () => i18n.changeLanguage(i18n.language === 'zh' ? 'en' : 'zh');

  const handleAdd = async () => {
    if (!newName) return;
    await createGraph(newName, newTT);
    setGraphs(await listGraphs().then(d => d.graphs || []));
    setShowAdd(false); setNewName('');
  };

  const handleCompact = async () => {
    const days = parseInt(compactDays) || 7;
    const before = (Date.now() - days * 86400 * 1000) * 1000;
    await compact(before, graph);
    setShowCompact(false);
  };

  const handleExtract = async () => {
    if (!extractContent.trim()) return;
    setExtractResult({ progress: true });
    try {
      const res = await extractDoc(extractContent, graph);
      setExtractResult(res);
      onExtractDone?.(res);
    } catch (e) {
      setExtractResult({ error: e.message });
    }
  };

  return (
    <div className="bg-gray-850 border-b border-gray-700 px-4 py-2 flex items-center gap-3 flex-wrap">
      <span className="text-blue-400 font-bold text-lg mr-2">BG</span>

      {/* Graph selector */}
      <div className="flex items-center gap-1">
        <select className="bg-gray-700 text-gray-200 rounded px-2 py-1 text-sm border border-gray-600" value={graph} onChange={e => setGraph(e.target.value)}>
          {graphs.map(g => <option key={g} value={g}>{g}</option>)}
        </select>
        <button className="text-green-400 hover:text-green-300 text-sm px-1" title={t('nav.addGraph')} onClick={() => setShowAdd(true)}>+</button>
      </div>

      <span className="text-gray-600">|</span>

      {/* Compact / Extract */}
      <button className="text-sm text-gray-400 hover:text-white px-2" onClick={() => setShowCompact(true)}>{t('nav.compact')}</button>
      <button className="text-sm text-gray-400 hover:text-white px-2" onClick={() => setShowExtract(true)}>{t('nav.extract')}</button>

      <div className="flex-1" />

      {/* Theme & Lang */}
      <button className="text-lg" onClick={toggleTheme}>{theme === 'dark' ? '☀️' : '🌙'}</button>
      <button className="text-sm text-gray-400 hover:text-white px-2" onClick={switchLang}>{i18n.language === 'zh' ? 'EN' : '中文'}</button>

      {/* Modals */}
      {showAdd && <Modal title={t('modal.addGraphTitle')} onClose={() => setShowAdd(false)}>
        <input className="w-full px-3 py-2 rounded bg-gray-700 border border-gray-600 text-gray-100 mb-3" placeholder={t('modal.addGraphName')} value={newName} onChange={e => setNewName(e.target.value)} />
        <label className="flex items-center gap-2 text-gray-300 mb-4"><input type="checkbox" checked={newTT} onChange={e => setNewTT(e.target.checked)} /> {t('modal.addGraphTimeTravel')}</label>
        <div className="flex justify-end gap-2"><button className="px-4 py-2 rounded bg-gray-600 text-gray-200" onClick={() => setShowAdd(false)}>{t('modal.addGraphCancel')}</button><button className="px-4 py-2 rounded bg-blue-600 text-white" onClick={handleAdd}>{t('modal.addGraphConfirm')}</button></div>
      </Modal>}

      {showCompact && <Modal title={t('modal.compactTitle')} onClose={() => setShowCompact(false)}>
        <p className="text-gray-400 text-sm mb-3">{t('modal.compactBefore')}</p>
        <div className="flex gap-2 mb-4">
          {['1', '7', '30'].map(d => <button key={d} className={`px-3 py-1 rounded text-sm ${compactDays === d ? 'bg-blue-600 text-white' : 'bg-gray-700 text-gray-300'}`} onClick={() => setCompactDays(d)}>{d}d</button>)}
          <input className="w-20 px-2 py-1 rounded bg-gray-700 border border-gray-600 text-gray-100 text-sm" placeholder={t('modal.compactCustom')} value={compactDays} onChange={e => setCompactDays(e.target.value)} />
        </div>
        <div className="flex justify-end"><button className="px-4 py-2 rounded bg-blue-600 text-white" onClick={handleCompact}>{t('modal.compactRun')}</button></div>
      </Modal>}

      {showExtract && <Modal title={t('modal.extractTitle')} onClose={() => setShowExtract(false)}>
        <div className="flex gap-2 mb-3">
          <label className="flex-1 px-3 py-2 rounded bg-gray-700 border border-gray-600 text-gray-400 text-sm cursor-pointer hover:bg-gray-600 text-center">
            📄 Upload .md
            <input type="file" accept=".md,.markdown,.txt" className="hidden" onChange={e => {
              const file = e.target.files?.[0];
              if (file) file.text().then(t => setExtractContent(t));
            }} />
          </label>
        </div>
        <textarea className="w-full h-28 px-3 py-2 rounded bg-gray-700 border border-gray-600 text-gray-100 text-sm mb-3" placeholder={t('modal.extractDrop')} value={extractContent} onChange={e => setExtractContent(e.target.value)} />
        {extractResult?.progress && <p className="text-yellow-400 text-sm mb-2">{t('modal.extractProgress')}...</p>}
        {extractResult?.stats && <p className="text-green-400 text-sm mb-2">{t('modal.extractDone', { v: extractResult.stats.new_vertices, e: extractResult.stats.new_edges })}</p>}
        {extractResult?.error && <p className="text-red-400 text-sm mb-2">{t('modal.extractError')}: {extractResult.error}</p>}
        <div className="flex justify-end gap-2">
          <button className="px-4 py-2 rounded bg-gray-600 text-gray-200" onClick={() => { setShowExtract(false); setExtractContent(''); setExtractResult(null); }}>{t('panel.close')}</button>
          <button className="px-4 py-2 rounded bg-blue-600 text-white" onClick={handleExtract} disabled={extractResult?.progress}>{t('modal.extractRun')}</button>
        </div>
      </Modal>}
    </div>
  );
}
