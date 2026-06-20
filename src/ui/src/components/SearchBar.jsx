import { useState } from 'react';
import { useTranslation } from 'react-i18next';

export default function SearchBar({ onSearch }) {
  const { t } = useTranslation();
  const [mode, setMode] = useState('keyword');
  const [query, setQuery] = useState('');
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [vLabel, setVLabel] = useState('');
  const [eLabel, setELabel] = useState('');

  const handleSearch = () => {
    if (!query.trim()) return;
    onSearch({ mode, query: query.trim(), vLabel, eLabel });
  };

  return (
    <div className="px-4 py-2 space-y-2">
      <div className="flex gap-2 items-center">
        <input
          className="flex-1 px-3 py-2 rounded bg-gray-800 border border-gray-600 text-gray-100 placeholder-gray-500 focus:outline-none focus:border-blue-500"
          placeholder={t('search.placeholder')}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
        />
        <div className="flex rounded overflow-hidden border border-gray-600">
          <button className={`px-3 py-2 text-sm ${mode === 'keyword' ? 'bg-blue-600 text-white' : 'bg-gray-700 text-gray-300'}`} onClick={() => setMode('keyword')}>{t('search.keyword')}</button>
          <button className={`px-3 py-2 text-sm ${mode === 'semantic' ? 'bg-blue-600 text-white' : 'bg-gray-700 text-gray-300'}`} onClick={() => setMode('semantic')}>{t('search.semantic')}</button>
        </div>
        <button className="px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700" onClick={handleSearch}>Search</button>
        <button className="px-3 py-2 text-sm text-gray-400 hover:text-white" onClick={() => setShowAdvanced(!showAdvanced)}>
          {t('search.advanced')} {showAdvanced ? '▲' : '▼'}
        </button>
      </div>
      {showAdvanced && (
        <div className="flex gap-4 text-sm">
          <label className="text-gray-400">{t('search.filterVertexLabel')}: <input className="ml-1 px-2 py-1 rounded bg-gray-800 border border-gray-600 text-gray-100 w-32" value={vLabel} onChange={e => setVLabel(e.target.value)} /></label>
          <label className="text-gray-400">{t('search.filterEdgeLabel')}: <input className="ml-1 px-2 py-1 rounded bg-gray-800 border border-gray-600 text-gray-100 w-32" value={eLabel} onChange={e => setELabel(e.target.value)} /></label>
        </div>
      )}
    </div>
  );
}
