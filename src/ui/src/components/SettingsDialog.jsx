import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { listGraphs, createGraph, deleteGraph, compact } from '../api';

function Modal({ title, children, onClose }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center" onClick={onClose}>
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
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
  activeProvider,
  onProviderChange,
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
      apiKey: '',
      models: ['deepseek-v4-flash'],
      defaultModel: 'deepseek-v4-flash',
      newModelInput: '',
    });
  };

  const handleSaveProvider = () => {
    if (!editingProvider?.name || !editingProvider?.apiBase || !editingProvider?.models?.length) return;
    const provider = {
      ...editingProvider,
      model: editingProvider.defaultModel || editingProvider.models[0],
    };
    delete provider.newModelInput;
    const existing = providers.findIndex((p) => p.id === provider.id);
    const updated = existing >= 0
      ? [...providers.slice(0, existing), provider, ...providers.slice(existing + 1)]
      : [...providers, provider];
    onUpdateProviders(updated);
    setEditingProvider(null);
  };

  const handleAddModel = () => {
    const name = editingProvider.newModelInput?.trim();
    if (!name) return;
    const models = [...(editingProvider.models || []), name];
    setEditingProvider({ ...editingProvider, models, newModelInput: '' });
  };

  const handleRemoveModel = (idx) => {
    const models = editingProvider.models.filter((_, i) => i !== idx);
    const removed = editingProvider.models[idx];
    const defaultModel = editingProvider.defaultModel === removed
      ? (models[0] || '')
      : editingProvider.defaultModel;
    setEditingProvider({ ...editingProvider, models, defaultModel });
  };

  const handleSetDefaultModel = (m) => {
    setEditingProvider({ ...editingProvider, defaultModel: m });
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

  if (!open) return null;

  return (
    <Modal title={t('settings.title')} onClose={onClose}>
      {/* Tabs */}
      <div className="flex gap-1.5 mb-5">
        <button className={tabCls(tab === 'providers')} onClick={() => setTab('providers')}>{t('settings.model')}</button>
        <button className={tabCls(tab === 'graphs')} onClick={() => setTab('graphs')}>{t('settings.graphs')}</button>
      </div>

      {/* ─── Providers ─── */}
      {tab === 'providers' && (
        <div>
          {editingProvider ? (
            <div className="space-y-3.5">
              {/* Provider name */}
              <div>
                <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t('settings.providerName')}</label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-[#2a2a2e] border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] text-[#e5e5e7] text-sm placeholder-[#48484a]"
                  type="text" value={editingProvider.name}
                  onChange={(e) => setEditingProvider({ ...editingProvider, name: e.target.value })} />
              </div>

              {/* API Base URL */}
              <div>
                <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">API Base URL</label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-[#2a2a2e] border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] text-[#e5e5e7] text-sm placeholder-[#48484a]"
                  type="text" value={editingProvider.apiBase}
                  onChange={(e) => setEditingProvider({ ...editingProvider, apiBase: e.target.value })} />
              </div>

              {/* API Key */}
              <div>
                <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">API Key</label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-[#2a2a2e] border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] text-[#e5e5e7] text-sm placeholder-[#48484a]"
                  type="password" value={editingProvider.apiKey}
                  onChange={(e) => setEditingProvider({ ...editingProvider, apiKey: e.target.value })} />
              </div>

              {/* Models list */}
              <div>
                <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">Models</label>
                <div className="space-y-1.5 mb-2">
                  {(editingProvider.models || []).map((m, idx) => (
                    <div key={idx} className="flex items-center gap-2 bg-[#2a2a2e] rounded-lg px-3 py-1.5">
                      <span className="flex-1 text-xs text-[#e5e5e7] font-mono">{m}</span>
                      <button className="text-[10px] text-[#ff453a] hover:text-[#ff6961] font-medium ml-1" onClick={() => handleRemoveModel(idx)}>{'\u2715'}</button>
                    </div>
                  ))}
                </div>
                {/* Add model input */}
                <div className="flex gap-2">
                  <input className="flex-1 px-3 py-1.5 rounded-xl bg-[#2a2a2e] border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] text-[#e5e5e7] text-xs placeholder-[#48484a]"
                    type="text" placeholder="Model name..." value={editingProvider.newModelInput || ''}
                    onChange={(e) => setEditingProvider({ ...editingProvider, newModelInput: e.target.value })}
                    onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); handleAddModel(); } }} />
                  <button className="px-3 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all" onClick={handleAddModel}>+ Add</button>
                </div>
              </div>

              <div className="flex gap-2 justify-end pt-1">
                <button className="px-4 py-2 rounded-xl bg-[#2a2a2e] text-[#86868b] hover:text-white text-sm font-medium transition-all" onClick={() => setEditingProvider(null)}>{t('panel.close')}</button>
                <button className="px-4 py-2 rounded-xl bg-[#0a84ff] text-white text-sm font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={handleSaveProvider}>{t('settings.save')}</button>
              </div>
            </div>
          ) : (
            <div>
              <label className="block text-xs text-[#636366] font-medium mb-2 tracking-tight">{t('settings.providers')}</label>
              {providers.length === 0 && (
                <p className="text-[#636366] text-sm text-center py-8 tracking-tight">{t('settings.noProviders')}</p>
              )}
              <div className="space-y-1 max-h-48 overflow-y-auto mb-3">
                {providers.map((p) => (
                  <div key={p.id} className="flex items-center justify-between py-2.5 px-3 rounded-xl hover:bg-[#2a2a2e] transition-all group">
                    <div>
                      <div className="text-sm text-[#e5e5e7] font-medium">{p.name}</div>
                      <div className="text-xs text-[#636366] mt-0.5">
                        {(p.defaultModel || p.model)} <span className="mx-1">{'\u00b7'}</span> {p.apiBase.replace(/^https?:\/\//, '').replace(/\/+$/, '')}
                        {p.models?.length > 1 && <span className="ml-1 text-[#48484a]">(+{p.models.length - 1} more)</span>}
                      </div>
                    </div>
                    <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button className="px-2.5 py-1 text-xs text-[#636366] hover:text-white hover:bg-[#3a3a3e] rounded-lg transition-all" onClick={() => {
                        setEditingProvider({ ...p, newModelInput: '' });
                      }}>{t('settings.edit')}</button>
                      <button className="px-2.5 py-1 text-xs text-[#ff453a] hover:bg-[#3a2a2e] rounded-lg transition-all" onClick={() => handleDeleteProvider(p.id)}>{t('settings.delete')}</button>
                    </div>
                  </div>
                ))}
              </div>
              <button className="w-full py-2.5 rounded-xl bg-[#2a2a2e] text-[#86868b] hover:text-white hover:bg-[#3a3a3e] text-sm font-medium transition-all" onClick={handleAddProvider}>
                + {t('settings.addProvider')}
              </button>

              {/* Default model selector */}
              <div className="mt-4">
                <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t('settings.defaultModel')}</label>
                <select className="w-full px-3 py-2 rounded-xl bg-[#2a2a2e] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] appearance-none cursor-pointer"
                  style={{ backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'%3E%3Cpath fill='%23636366' d='M0 0l5 6 5-6z'/%3E%3C/svg%3E\")", backgroundRepeat: 'no-repeat', backgroundPosition: 'right 12px center', paddingRight: '32px' }}
                  value={(() => {
                    const ap = providers.find(p => p.id === activeProvider);
                    return ap ? ap.name + '/' + (ap.defaultModel || ap.model || '') : '';
                  })()}
                  onChange={(e) => {
                    const parts = e.target.value.split('/');
                    const provName = parts[0];
                    const modelName = parts.slice(1).join('/');
                    const idx = providers.findIndex((p) => p.name === provName);
                    if (idx >= 0) {
                      const provId = providers[idx].id;
                      onUpdateProviders(providers.map((p) => p.id === provId ? { ...p, defaultModel: modelName, model: modelName } : p));
                      if (provId !== activeProvider) {
                        onProviderChange(provId);
                      }
                    }
                  }}>
                  {providers.flatMap((p) => {
                    const models = p.models || [p.model];
                    return models.map((m) => ({
                      key: p.name + '/' + m,
                      label: p.name + '/' + m,
                    }));
                  }).map((opt) => (
                    <option key={opt.key} value={opt.key}>{opt.label}</option>
                  ))}
                </select>
              </div>
            </div>
          )}
        </div>
      )}

      {/* ─── Graphs ─── */}
      {tab === 'graphs' && (
        <div>

          <div className="space-y-1 max-h-36 overflow-y-auto mb-3">
            {graphs.map((g) => (
              <div key={g} className="flex items-center justify-between py-2 px-3 rounded-xl hover:bg-[#2a2a2e] transition-all group">
                <div className="flex items-center gap-2 min-w-0">
                  <span className="text-sm text-[#e5e5e7] font-medium truncate">{g}</span>
                  {g === graphName && (
                    <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-[#0a84ff]/20 text-[#0a84ff] border border-[#0a84ff]/30 flex-shrink-0">默认</span>
                  )}
                </div>
                <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 ml-2">
                  {g !== graphName && (
                    <button className="px-2 py-1 text-xs text-[#636366] hover:text-white hover:bg-[#3a3a3e] rounded-lg transition-all" onClick={() => onGraphNameChange(g)}>设为默认</button>
                  )}
                  <button className="px-2 py-1 text-xs text-[#ff9f0a] hover:bg-[#3a3020] rounded-lg transition-all" onClick={async () => {
                    const days = parseInt(prompt('Compaction days (default: 7):', '7') || '7');
                    if (days > 0) { const before = (Date.now() - days * 86400 * 1000) * 1000; await compact(before, g); }
                  }}>归档</button>
                  <button className="px-2 py-1 text-xs text-[#ff453a] hover:bg-[#3a2a2e] rounded-lg transition-all" onClick={() => handleDeleteGraph(g)}>删除</button>
                </div>
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

          
        </div>
      )}
    </Modal>

  );
}