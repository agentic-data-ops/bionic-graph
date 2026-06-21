import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { listGraphs, createGraph, deleteGraph, compact } from '../api';

function Modal({ title, children, onClose }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center" onClick={onClose}>
      {/* Backdrop */}
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
      {/* Panel */}
      <div
        className="relative bg-[#1c1c20] border border-[#2a2a2e] rounded-2xl p-6 min-w-[520px] max-w-lg max-h-[80vh] overflow-y-auto shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="text-base font-semibold text-[#e5e5e7] mb-5 flex items-center justify-between tracking-tight">
          <span>{title}</span>
          <button className="w-7 h-7 rounded-lg bg-[#2a2a2e] hover:bg-[#3a3a3e] flex items-center justify-center text-[#636366] hover:text-white transition-all text-sm" onClick={onClose}>
            <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

const tabCls = (active) =>
  `px-4 py-2 text-sm font-medium rounded-xl transition-all duration-200 ${
    active
      ? 'bg-[#3a3a3e] text-white shadow-sm'
      : 'text-[#636366] hover:text-[#e5e5e7] hover:bg-[#2a2a2e]'
  }`;

export default function SettingsDialog({
  open,
  onClose,
  providers,
  onUpdateProviders,
  graphName,
  onGraphNameChange,
  graphs,
  onGraphsChange,
  theme,
  onThemeToggle,
  language,
  onLanguageToggle,
}) {
  const { t, i18n } = useTranslation();
  const [tab, setTab] = useState('providers');
  const [editingProvider, setEditingProvider] = useState(null);
  const [showAddGraph, setShowAddGraph] = useState(false);
  const [newGraphName, setNewGraphName] = useState('');
  const [newGraphTT, setNewGraphTT] = useState(false);
  const [showCompact, setShowCompact] = useState(false);
  const [compactDays, setCompactDays] = useState('7');

  useEffect(() => {
    if (open) {
      listGraphs().then((d) => onGraphsChange?.(d.graphs || [])).catch(() => {});
    }
  }, [open, onGraphsChange]);

  const handleAddProvider = () => {
    setEditingProvider({
      id: Date.now().toString(),
      name: '',
      apiBase: 'https://api.deepseek.com/v1',
      model: 'deepseek-v4-flash',
      apiKey: '',
    });
  };

  const handleSaveProvider = () => {
    if (!editingProvider?.name || !editingProvider?.apiBase || !editingProvider?.model) return;
    const existing = providers.findIndex((p) => p.id === editingProvider.id);
    const updated = existing >= 0
      ? [...providers.slice(0, existing), editingProvider, ...providers.slice(existing + 1)]
      : [...providers, editingProvider];
    onUpdateProviders(updated);
    setEditingProvider(null);
  };

  const handleDeleteProvider = (id) => {
    onUpdateProviders(providers.filter((p) => p.id !== id));
  };

  const handleAddGraph = async () => {
    if (!newGraphName) return;
    await createGraph(newGraphName, newGraphTT);
    const updated = await listGraphs().then((d) => d.graphs || []);
    onGraphsChange(updated);
    setShowAddGraph(false);
    setNewGraphName('');
  };

  const handleDeleteGraph = async (name) => {
    await deleteGraph(name);
    const updated = await listGraphs().then((d) => d.graphs || []);
    onGraphsChange(updated);
    if (graphName === name && updated.length > 0) onGraphNameChange(updated[0]);
  };

  const handleCompact = async () => {
    const days = parseInt(compactDays) || 7;
    const before = (Date.now() - days * 86400 * 1000) * 1000;
    await compact(before, graphName);
    setShowCompact(false);
  };

  if (!open) return null;

  return (
    <Modal title={t('settings.title')} onClose={onClose}>
      {/* Tabs */}
      <div className="flex gap-1.5 mb-5">
        <button className={tabCls(tab === 'providers')} onClick={() => setTab('providers')}>{t('settings.providers')}</button>
        <button className={tabCls(tab === 'graphs')} onClick={() => setTab('graphs')}>{t('settings.graphs')}</button>
        <button className={tabCls(tab === 'general')} onClick={() => setTab('general')}>{t('settings.general')}</button>
      </div>

      {/* ─── Providers ─── */}
      {tab === 'providers' && (
        <div>
          {editingProvider ? (
            <div className="space-y-3.5">
              {[
                { label: t('settings.providerName'), key: 'name', type: 'text' },
                { label: 'API Base URL', key: 'apiBase', type: 'text' },
                { label: t('settings.model'), key: 'model', type: 'text' },
                { label: 'API Key', key: 'apiKey', type: 'password' },
              ].map(({ label, key, type }) => (
                <div key={key}>
                  <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{label}</label>
                  <input
                    className="w-full px-3.5 py-2 rounded-xl bg-[#2a2a2e] border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] text-[#e5e5e7] text-sm transition-all placeholder-[#48484a]"
                    type={type}
                    value={editingProvider[key]}
                    onChange={(e) => setEditingProvider({ ...editingProvider, [key]: e.target.value })}
                  />
                </div>
              ))}
              <div className="flex gap-2 justify-end pt-1">
                <button className="px-4 py-2 rounded-xl bg-[#2a2a2e] text-[#86868b] hover:text-white text-sm font-medium transition-all" onClick={() => setEditingProvider(null)}>{t('panel.close')}</button>
                <button className="px-4 py-2 rounded-xl bg-[#0a84ff] text-white text-sm font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={handleSaveProvider}>{t('settings.save')}</button>
              </div>
            </div>
          ) : (
            <div>
              {providers.length === 0 && (
                <p className="text-[#636366] text-sm text-center py-8 tracking-tight">{t('settings.noProviders')}</p>
              )}
              <div className="space-y-1 max-h-48 overflow-y-auto mb-3">
                {providers.map((p) => (
                  <div key={p.id} className="flex items-center justify-between py-2.5 px-3 rounded-xl hover:bg-[#2a2a2e] transition-all group">
                    <div>
                      <div className="text-sm text-[#e5e5e7] font-medium">{p.name}</div>
                      <div className="text-xs text-[#636366] mt-0.5">{p.model} <span className="mx-1">·</span> {p.apiBase.replace(/^https?:\/\//, '').replace(/\/+$/, '')}</div>
                    </div>
                    <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button className="px-2.5 py-1 text-xs text-[#636366] hover:text-white hover:bg-[#3a3a3e] rounded-lg transition-all" onClick={() => setEditingProvider(p)}>{t('settings.edit')}</button>
                      <button className="px-2.5 py-1 text-xs text-[#ff453a] hover:bg-[#3a2a2e] rounded-lg transition-all" onClick={() => handleDeleteProvider(p.id)}>{t('settings.delete')}</button>
                    </div>
                  </div>
                ))}
              </div>
              <button className="w-full py-2.5 rounded-xl bg-[#2a2a2e] text-[#86868b] hover:text-white hover:bg-[#3a3a3e] text-sm font-medium transition-all" onClick={handleAddProvider}>
                + {t('settings.addProvider')}
              </button>
            </div>
          )}
        </div>
      )}

      {/* ─── Graphs ─── */}
      {tab === 'graphs' && (
        <div>
          <div className="mb-4">
            <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t('settings.defaultGraph')}</label>
            <select
              className="w-full px-3.5 py-2 rounded-xl bg-[#2a2a2e] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] appearance-none cursor-pointer"
              style={{ backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'%3E%3Cpath fill='%23636366' d='M0 0l5 6 5-6z'/%3E%3C/svg%3E\")", backgroundRepeat: 'no-repeat', backgroundPosition: 'right 12px center', paddingRight: '32px' }}
              value={graphName}
              onChange={(e) => onGraphNameChange(e.target.value)}
            >
              {graphs.map((g) => <option key={g} value={g}>{g}</option>)}
            </select>
          </div>

          <div className="space-y-1 max-h-36 overflow-y-auto mb-3">
            {graphs.map((g) => (
              <div key={g} className="flex items-center justify-between py-2 px-3 rounded-xl hover:bg-[#2a2a2e] transition-all group">
                <span className="text-sm text-[#e5e5e7] font-medium">{g}</span>
                <button className="px-2.5 py-1 text-xs text-[#ff453a] opacity-0 group-hover:opacity-100 hover:bg-[#3a2a2e] rounded-lg transition-all" onClick={() => handleDeleteGraph(g)}>{t('settings.delete')}</button>
              </div>
            ))}
          </div>

          {showAddGraph ? (
            <div className="space-y-3 mb-3 p-4 bg-[#2a2a2e] rounded-xl">
              <input className="w-full px-3.5 py-2 rounded-xl bg-[#1c1c20] border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] text-[#e5e5e7] text-sm placeholder-[#48484a]"
                placeholder={t('modal.addGraphName')} value={newGraphName} onChange={(e) => setNewGraphName(e.target.value)} />
              <label className="flex items-center gap-2 text-xs text-[#86868b] cursor-pointer select-none">
                <input type="checkbox" checked={newGraphTT} onChange={(e) => setNewGraphTT(e.target.checked)}
                  className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[#1c1c20] checked:bg-[#0a84ff] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
                {t('modal.addGraphTimeTravel')}
              </label>
              <div className="flex gap-2 justify-end">
                <button className="px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all" onClick={() => setShowAddGraph(false)}>{t('panel.close')}</button>
                <button className="px-3.5 py-1.5 rounded-xl bg-[#0a84ff] text-white text-xs font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={handleAddGraph}>{t('modal.addGraphConfirm')}</button>
              </div>
            </div>
          ) : (
            <button className="w-full py-2.5 rounded-xl bg-[#2a2a2e] text-[#86868b] hover:text-white hover:bg-[#3a3a3e] text-sm font-medium transition-all mb-2" onClick={() => setShowAddGraph(true)}>
              + {t('modal.addGraphTitle')}
            </button>
          )}

          {showCompact ? (
            <div className="space-y-3 p-4 bg-[#2a2a2e] rounded-xl">
              <p className="text-xs text-[#636366] tracking-tight">{t('modal.compactBefore')}</p>
              <div className="flex gap-2">
                {['1', '7', '30'].map((d) => (
                  <button key={d}
                    className={`px-3.5 py-1.5 rounded-xl text-xs font-medium transition-all ${compactDays === d ? 'bg-[#0a84ff] text-white shadow-sm' : 'bg-[#3a3a3e] text-[#86868b] hover:text-white'}`}
                    onClick={() => setCompactDays(d)}>{d}d</button>
                ))}
              </div>
              <div className="flex gap-2 justify-end">
                <button className="px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all" onClick={() => setShowCompact(false)}>{t('panel.close')}</button>
                <button className="px-3.5 py-1.5 rounded-xl bg-[#0a84ff] text-white text-xs font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={handleCompact}>{t('modal.compactRun')}</button>
              </div>
            </div>
          ) : (
            <button className="w-full py-2.5 rounded-xl bg-[#2a2a2e] text-[#86868b] hover:text-white hover:bg-[#3a3a3e] text-sm font-medium transition-all" onClick={() => setShowCompact(true)}>
              {t('nav.compact')}
            </button>
          )}
        </div>
      )}

      {/* ─── General ─── */}
      {tab === 'general' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between py-2.5 px-3 rounded-xl hover:bg-[#2a2a2e] transition-all">
            <span className="text-sm text-[#e5e5e7] font-medium">{t('nav.theme')}</span>
            <button className="w-9 h-9 rounded-xl bg-[#2a2a2e] hover:bg-[#3a3a3e] flex items-center justify-center text-sm transition-all" onClick={onThemeToggle}>
              {theme === 'dark' ? '☀️' : '🌙'}
            </button>
          </div>
          <div className="flex items-center justify-between py-2.5 px-3 rounded-xl hover:bg-[#2a2a2e] transition-all">
            <span className="text-sm text-[#e5e5e7] font-medium">{t('nav.lang')}</span>
            <button className="px-3.5 py-1.5 rounded-xl bg-[#2a2a2e] text-[#86868b] hover:text-white hover:bg-[#3a3a3e] text-sm font-medium transition-all" onClick={onLanguageToggle}>
              {language === 'zh' ? 'English' : '中文'}
            </button>
          </div>
        </div>
      )}
    </Modal>
  );
}
