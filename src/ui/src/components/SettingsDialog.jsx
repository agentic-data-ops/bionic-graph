import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { fetchSearchConfig, updateSearchConfig, fetchRankConfig, updateRankConfig, fetchWebSearchConfig, updateWebSearchConfig } from '../api';

function Modal({ title, children, onClose }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
      <div
        className="relative bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-6 min-w-[520px] max-w-lg max-h-[80vh] overflow-y-auto shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="text-base font-semibold text-[var(--text-primary)] mb-5 flex items-center justify-between tracking-tight">
          <span>{title}</span>
          <button className="w-7 h-7 rounded-lg bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-all text-sm" onClick={onClose}>
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
      ? 'bg-[var(--accent-bg)] text-[var(--accent)] shadow-sm'
      : 'text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]'
  }`;

// ── Time unit conversion helpers ────────────────────────────────────────────
const TIME_UNITS = [
  { label: '秒', value: 1 },
  { label: '分钟', value: 60 },
  { label: '小时', value: 3600 },
  { label: '天', value: 86400 },
];

/** Convert seconds to best-fit display value + unit. */
function secondsToDisplay(secs) {
  if (secs == null || secs <= 0) return { value: 0, unit: 1 };
  for (let i = TIME_UNITS.length - 1; i >= 0; i--) {
    const u = TIME_UNITS[i];
    if (secs >= u.value && secs % u.value === 0) {
      return { value: secs / u.value, unit: u.value };
    }
  }
  return { value: secs, unit: 1 };
}

/** Convert display value + unit back to seconds. */
function displayToSeconds(value, unit) {
  return Math.round(value * unit);
}

export default function SettingsDialog({
  open,
  onClose,
  providers,
  onUpdateProviders,
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
  const [searchConfig, setSearchConfig] = useState(null);
  const [searchSaving, setSearchSaving] = useState(false);
  const [searchMessage, setSearchMessage] = useState('');
  const [rankConfig, setRankConfig] = useState(null);
  const [rankSaving, setRankSaving] = useState(false);
  const [rankMessage, setRankMessage] = useState('');
  const [rankThreshold, setRankThreshold] = useState({ value: 15, unit: 86400 });
  const [rankPeriod, setRankPeriod] = useState({ value: 1, unit: 86400 });
  const [defaultModelOpen, setDefaultModelOpen] = useState(false);
  const [defaultWebProviderOpen, setDefaultWebProviderOpen] = useState(false);
  const [timeUnitOpen, setTimeUnitOpen] = useState(false);
  const [periodUnitOpen, setPeriodUnitOpen] = useState(false);
  const [webSearchConfig, setWebSearchConfig] = useState(null);
  const [editingWebProvider, setEditingWebProvider] = useState(null);
  const [webSearchSaving, setWebSearchSaving] = useState(false);
  const [webSearchMessage, setWebSearchMessage] = useState('');
  const f3 = (v) => v !== undefined && v !== null ? Number(v).toFixed(3) : '';
  useEffect(() => {
    if (open) {
      fetchSearchConfig().then((d) => {
        setSearchConfig(d);
        // Also sync to localStorage so ChatArea can read it
        try {
          const stored = JSON.parse(localStorage.getItem('bgraph-settings') || '{}');
          stored.greedy = d.greedy;
          stored.exact = d.exact;
          localStorage.setItem('bgraph-settings', JSON.stringify(stored));
        } catch (e) {}
      }).catch(() => {});
      fetchRankConfig().then((d) => {
        setRankConfig(d);
        setRankThreshold(secondsToDisplay(d.inactive_after_accessed_secs));
        setRankPeriod(secondsToDisplay(d.inactive_rank_update_period));
      }).catch(() => {});
      fetchWebSearchConfig().then((d) => {
        setWebSearchConfig(d);
      }).catch(() => {});
      setSearchMessage('');
      setRankMessage('');
      setWebSearchMessage('');
    }
  }, [open]);

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

  if (!open) return null;

  // Compute default model options for the custom dropdown.
  const modelOptions = providers.flatMap((p) => {
    const models = p.models || [p.model];
    return models.map((m) => ({ key: p.name + '/' + m, label: p.name + '/' + m, provName: p.name, modelName: m, provId: p.id }));
  });
  const ap = providers.find(p => p.id === activeProvider);
  const currentModelVal = ap ? ap.name + '/' + (ap.defaultModel || ap.model || '') : '';

  return (
    <Modal title={t('settings.title')} onClose={onClose}>
      {/* Tabs */}
      <div className="flex gap-1.5 mb-5">
        <button className={tabCls(tab === 'providers')} onClick={() => setTab('providers')}>{t('settings.model')}</button>
        <button className={tabCls(tab === 'search')} onClick={() => setTab('search')}>图谱搜索</button>
        <button className={tabCls(tab === 'rank')} onClick={() => setTab('rank')}>排序</button>
        <button className={tabCls(tab === 'websearch')} onClick={() => setTab('websearch')}>联网搜索</button>
      </div>

      {/* ─── Providers ─── */}
      {tab === 'providers' && (
        <div>
          {editingProvider ? (
            <div className="space-y-3.5">
              {/* Provider name */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('settings.providerName')}</label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-transparent border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm placeholder-[var(--text-muted)]"
                  type="text" value={editingProvider.name}
                  onChange={(e) => setEditingProvider({ ...editingProvider, name: e.target.value })} />
              </div>

              {/* API Base URL */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">API Base URL</label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-transparent border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm placeholder-[var(--text-muted)]"
                  type="text" value={editingProvider.apiBase}
                  onChange={(e) => setEditingProvider({ ...editingProvider, apiBase: e.target.value })} />
              </div>

              {/* API Key */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">API Key</label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-transparent border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm placeholder-[var(--text-muted)]"
                  type="password" value={editingProvider.apiKey}
                  onChange={(e) => setEditingProvider({ ...editingProvider, apiKey: e.target.value })} />
              </div>

              {/* Models list */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">Models</label>
                <div className="space-y-1.5 mb-2">
                  {(editingProvider.models || []).map((m, idx) => (
                    <div key={idx} className="flex items-center gap-2 bg-[var(--bg-tertiary)] rounded-lg px-3 py-1.5">
                      <span className="flex-1 text-xs text-[var(--text-primary)] font-mono">{m}</span>
                      <button className="text-[10px] text-[var(--danger)] hover:text-[#ff6961] font-medium ml-1" onClick={() => handleRemoveModel(idx)}>{'\u2715'}</button>
                    </div>
                  ))}
                </div>
                {/* Add model input */}
                <div className="flex gap-2">
                  <input className="flex-1 px-3 py-1.5 rounded-xl bg-transparent border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-xs placeholder-[var(--text-muted)]"
                    type="text" placeholder="Model name..." value={editingProvider.newModelInput || ''}
                    onChange={(e) => setEditingProvider({ ...editingProvider, newModelInput: e.target.value })}
                    onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); handleAddModel(); } }} />
                  <button className="px-3 py-1.5 rounded-xl bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium transition-all" onClick={handleAddModel}>+ Add</button>
                </div>
              </div>

              <div className="flex gap-2 justify-end pt-1">
                <button className="px-4 py-2 rounded-xl bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-sm font-medium transition-all" onClick={() => setEditingProvider(null)}>{t('panel.close')}</button>
                <button className="px-4 py-2 rounded-xl bg-[var(--accent)] text-white text-sm font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm" onClick={handleSaveProvider}>{t('settings.save')}</button>
              </div>
            </div>
          ) : (
            <div>
              {/* Default model selector */}
              <div className="mb-4">
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('settings.defaultModel')}</label>
                <div className="relative">
                  <button
                    className="w-full px-3 py-2 rounded-xl bg-transparent text-[var(--text-primary)] text-sm border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] transition-all font-medium flex items-center gap-1 text-left"
                    onClick={(e) => { e.stopPropagation(); setDefaultModelOpen(!defaultModelOpen); }}
                  >
                    <span className="flex-1 truncate">{currentModelVal || t('chat.selectModel')}</span>
                    <svg className={`w-3 h-3 flex-shrink-0 transition-transform ${defaultModelOpen ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}><path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" /></svg>
                  </button>
                  {defaultModelOpen && (
                    <>
                      <div className="fixed inset-0 z-40" onClick={() => setDefaultModelOpen(false)} />
                      <div className="absolute left-0 top-full mt-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden w-full max-h-[300px] overflow-y-auto">
                        {modelOptions.map((opt) => (
                          <button
                            key={opt.key}
                            className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap truncate transition-all ${opt.key === currentModelVal ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                            onClick={() => {
                              onUpdateProviders(providers.map((p) => p.id === opt.provId ? { ...p, defaultModel: opt.modelName, model: opt.modelName } : p));
                              if (opt.provId !== activeProvider) onProviderChange(opt.provId);
                              setDefaultModelOpen(false);
                            }}
                          >{opt.label}</button>
                        ))}
                      </div>
                    </>
                  )}
                </div>
              </div>

              <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-2 tracking-tight">{t('settings.providers')}</label>
              {providers.length === 0 && (
                <p className="text-[var(--text-tertiary)] text-sm text-center py-8 tracking-tight">{t('settings.noProviders')}</p>
              )}
              <div className="space-y-1 max-h-48 overflow-y-auto mb-3">
                {providers.map((p) => (
                  <div key={p.id} className="flex items-center justify-between py-2.5 px-3 rounded-xl hover:bg-[var(--bg-tertiary)] transition-all group">
                    <div>
                      <div className="text-sm text-[var(--text-primary)] font-medium">{p.name}</div>
                      <div className="text-xs text-[var(--text-tertiary)] mt-0.5">
                        {(p.defaultModel || p.model)} <span className="mx-1">{'\u00b7'}</span> {p.apiBase.replace(/^https?:\/\//, '').replace(/\/+$/, '')}
                        {p.models?.length > 1 && <span className="ml-1 text-[var(--text-muted)]">(+{p.models.length - 1} more)</span>}
                      </div>
                    </div>
                    <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button className="px-2.5 py-1 text-xs text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] rounded-lg transition-all" onClick={() => {
                        setEditingProvider({ ...p, apiKey: '', newModelInput: '' });
                      }}>{t('settings.edit')}</button>
                      <button className="px-2.5 py-1 text-xs text-[var(--danger)] hover:bg-[color-mix(in srgb, var(--bg-hover), var(--danger) 30%)] rounded-lg transition-all" onClick={() => handleDeleteProvider(p.id)}>{t('settings.delete')}</button>
                    </div>
                  </div>
                ))}
              </div>
              <button className="w-full py-2.5 rounded-xl bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] text-sm font-medium transition-all" onClick={handleAddProvider}>
                + {t('settings.addProvider')}
              </button>
            </div>
          )}
        </div>
      )}

      {/* ─── Search Config ─── */}
      {tab === 'search' && (
        <div>
          {searchConfig ? (
            <div className="space-y-4">


              { /* ── Greedy ── */ }
              <div className="p-3 rounded-xl bg-[var(--bg-tertiary)]/50">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-xs font-medium text-[var(--text-secondary)]">Greedy (贪婪模式)</span>
                </div>
                <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none mb-2">
                  <input type="checkbox" checked={searchConfig.greedy.traverse}
                    onChange={(e) => setSearchConfig({ ...searchConfig, greedy: { ...searchConfig.greedy, traverse: e.target.checked } })}
                    className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
                  启用数据遍历
                </label>
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">激活传播阈值</label>
                    <div className="flex items-center gap-2">
                      <input className="flex-1 accent-[var(--accent)]" type="range" min="0" max="1" step="0.05"
                        value={searchConfig.greedy.activate}
                        onChange={(e) => setSearchConfig({ ...searchConfig, greedy: { ...searchConfig.greedy, activate: parseFloat(e.target.value) } })} />
                      <span className="text-xs text-[var(--text-secondary)] w-8 text-right">{searchConfig.greedy.activate.toFixed(2)}</span>
                    </div>
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">激活衰减值</label>
                    <div className="flex items-center gap-2">
                      <input className="flex-1 accent-[var(--accent)]" type="range" min="0" max="1" step="0.05"
                        value={searchConfig.greedy.decay}
                        onChange={(e) => setSearchConfig({ ...searchConfig, greedy: { ...searchConfig.greedy, decay: parseFloat(e.target.value) } })} />
                      <span className="text-xs text-[var(--text-secondary)] w-8 text-right">{searchConfig.greedy.decay.toFixed(2)}</span>
                    </div>
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">探索深度</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-transparent border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" min="1" max="255" value={searchConfig.greedy.depth}
                      onChange={(e) => setSearchConfig({ ...searchConfig, greedy: { ...searchConfig.greedy, depth: parseInt(e.target.value) || 1 } })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">探索结果分值</label>
                    <div className="flex items-center gap-2">
                      <input className="flex-1 accent-[var(--accent)]" type="range" min="0" max="1" step="0.05"
                        value={searchConfig.greedy.score}
                        onChange={(e) => setSearchConfig({ ...searchConfig, greedy: { ...searchConfig.greedy, score: parseFloat(e.target.value) } })} />
                      <span className="text-xs text-[var(--text-secondary)] w-8 text-right">{searchConfig.greedy.score.toFixed(2)}</span>
                    </div>
                  </div>
                </div>
              </div>

              { /* ── Exact ── */ }
              <div className="p-3 rounded-xl bg-[var(--bg-tertiary)]/50">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-xs font-medium text-[var(--text-secondary)]">Exact (精确模式)</span>
                </div>
                <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none mb-2">
                  <input type="checkbox" checked={searchConfig.exact.traverse}
                    onChange={(e) => setSearchConfig({ ...searchConfig, exact: { ...searchConfig.exact, traverse: e.target.checked } })}
                    className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
                  启用数据遍历
                </label>
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">激活传播阈值</label>
                    <div className="flex items-center gap-2">
                      <input className="flex-1 accent-[var(--accent)]" type="range" min="0" max="1" step="0.05"
                        value={searchConfig.exact.activate}
                        onChange={(e) => setSearchConfig({ ...searchConfig, exact: { ...searchConfig.exact, activate: parseFloat(e.target.value) } })} />
                      <span className="text-xs text-[var(--text-secondary)] w-8 text-right">{searchConfig.exact.activate.toFixed(2)}</span>
                    </div>
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">激活衰减值</label>
                    <div className="flex items-center gap-2">
                      <input className="flex-1 accent-[var(--accent)]" type="range" min="0" max="1" step="0.05"
                        value={searchConfig.exact.decay}
                        onChange={(e) => setSearchConfig({ ...searchConfig, exact: { ...searchConfig.exact, decay: parseFloat(e.target.value) } })} />
                      <span className="text-xs text-[var(--text-secondary)] w-8 text-right">{searchConfig.exact.decay.toFixed(2)}</span>
                    </div>
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">探索深度</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-transparent border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" min="1" max="255" value={searchConfig.exact.depth}
                      onChange={(e) => setSearchConfig({ ...searchConfig, exact: { ...searchConfig.exact, depth: parseInt(e.target.value) || 1 } })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">探索结果分值</label>
                    <div className="flex items-center gap-2">
                      <input className="flex-1 accent-[var(--accent)]" type="range" min="0" max="1" step="0.05"
                        value={searchConfig.exact.score}
                        onChange={(e) => setSearchConfig({ ...searchConfig, exact: { ...searchConfig.exact, score: parseFloat(e.target.value) } })} />
                      <span className="text-xs text-[var(--text-secondary)] w-8 text-right">{searchConfig.exact.score.toFixed(2)}</span>
                    </div>
                  </div>
                </div>
              </div>

              {/* Save button */}
              <div className="flex items-center gap-3 pt-1">
                <button className="px-4 py-2 rounded-xl bg-[var(--accent)] text-white text-sm font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm disabled:opacity-40"
                  disabled={searchSaving}
                  onClick={async () => {
                    setSearchSaving(true);
                    setSearchMessage('');
                    try {
                      await updateSearchConfig(searchConfig);
                      // Sync to localStorage
                      try {
                        const stored = JSON.parse(localStorage.getItem('bgraph-settings') || '{}');
                        stored.greedy = searchConfig.greedy;
                        stored.exact = searchConfig.exact;
                        localStorage.setItem('bgraph-settings', JSON.stringify(stored));
                      } catch (e) {}
                      setSearchMessage('✅ 保存成功');
                    } catch (e) {
                      setSearchMessage('❌ 保存失败: ' + e.message);
                    } finally {
                      setSearchSaving(false);
                    }
                  }}>
                  {searchSaving ? '保存中...' : '保存配置'}
                </button>
                {searchMessage && (
                  <span className="text-xs text-[var(--text-secondary)]">{searchMessage}</span>
                )}
              </div>
            </div>
          ) : (
            <p className="text-[var(--text-tertiary)] text-sm text-center py-8 tracking-tight">加载配置中...</p>
          )}
        </div>
      )}
      {tab === 'rank' && (
        <div className="space-y-4">
          {rankConfig ? (
            <>
              <div className="grid grid-cols-2 gap-4">
                <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none">
                  <input type="checkbox" checked={rankConfig.auto_inc_rank_when_update}
                    onChange={(e) => setRankConfig({ ...rankConfig, auto_inc_rank_when_update: e.target.checked })}
                    className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)]" />
                  更新时自动增加排序值
                </label>
                <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none">
                  <input type="checkbox" checked={rankConfig.auto_inc_rank_when_read}
                    onChange={(e) => setRankConfig({ ...rankConfig, auto_inc_rank_when_read: e.target.checked })}
                    className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)]" />
                  读取时自动增加排序值
                </label>
                <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none">
                  <input type="checkbox" checked={rankConfig.auto_dec_rank_when_inactive}
                    onChange={(e) => setRankConfig({ ...rankConfig, auto_dec_rank_when_inactive: e.target.checked })}
                    className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)]" />
                  不活跃时自动降低排序值
                </label>
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div className="min-w-0">
                  <label className="block text-xs text-[var(--text-tertiary)] mb-1">不活跃阈值</label>
                  <div className="flex gap-1">
                    <input type="number" min="1" value={rankThreshold.value}
                      onChange={(e) => setRankThreshold({ ...rankThreshold, value: Number(e.target.value) })}
                      className="w-0 flex-1 px-3 py-1.5 rounded-lg bg-transparent border border-[var(--border)] text-xs text-[var(--text-primary)]" />
                    <div className="relative flex-shrink-0">
                      <button
                        className="px-2 py-1.5 text-xs rounded-lg bg-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] transition-all font-medium flex items-center gap-1 w-14"
                        onClick={(e) => { e.stopPropagation(); setTimeUnitOpen(!timeUnitOpen); }}
                      >
                        <span className="flex-1 text-center">{TIME_UNITS.find(u => u.value === rankThreshold.unit)?.label}</span>
                        <svg className={`w-2.5 h-2.5 flex-shrink-0 transition-transform ${timeUnitOpen ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}><path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" /></svg>
                      </button>
                      {timeUnitOpen && (
                        <>
                          <div className="fixed inset-0 z-40" onClick={() => setTimeUnitOpen(false)} />
                          <div className="absolute right-0 bottom-full mb-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden w-full">
                            {TIME_UNITS.map((u) => (
                              <button
                                key={u.value}
                                className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap truncate transition-all ${u.value === rankThreshold.unit ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                                onClick={() => { setRankThreshold({ ...rankThreshold, unit: u.value }); setTimeUnitOpen(false); }}
                              >{u.label}</button>
                            ))}
                          </div>
                        </>
                      )}
                    </div>
                  </div>
                </div>
                <div className="min-w-0">
                  <label className="block text-xs text-[var(--text-tertiary)] mb-1">扫描间隔</label>
                  <div className="flex gap-1">
                    <input type="number" min="1" value={rankPeriod.value}
                      onChange={(e) => setRankPeriod({ ...rankPeriod, value: Number(e.target.value) })}
                      className="w-0 flex-1 px-3 py-1.5 rounded-lg bg-transparent border border-[var(--border)] text-xs text-[var(--text-primary)]" />
                    <div className="relative flex-shrink-0">
                      <button
                        className="px-2 py-1.5 text-xs rounded-lg bg-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] transition-all font-medium flex items-center gap-1 w-14"
                        onClick={(e) => { e.stopPropagation(); setPeriodUnitOpen(!periodUnitOpen); }}
                      >
                        <span className="flex-1 text-center">{TIME_UNITS.find(u => u.value === rankPeriod.unit)?.label}</span>
                        <svg className={`w-2.5 h-2.5 flex-shrink-0 transition-transform ${periodUnitOpen ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}><path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" /></svg>
                      </button>
                      {periodUnitOpen && (
                        <>
                          <div className="fixed inset-0 z-40" onClick={() => setPeriodUnitOpen(false)} />
                          <div className="absolute right-0 bottom-full mb-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden w-full">
                            {TIME_UNITS.map((u) => (
                              <button
                                key={u.value}
                                className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap truncate transition-all ${u.value === rankPeriod.unit ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                                onClick={() => { setRankPeriod({ ...rankPeriod, unit: u.value }); setPeriodUnitOpen(false); }}
                              >{u.label}</button>
                            ))}
                          </div>
                        </>
                      )}
                    </div>
                  </div>
                </div>
              </div>
              <div className="flex items-center gap-3 pt-1">
                <button className="px-4 py-1.5 rounded-lg bg-[var(--accent)] text-white text-xs font-medium hover:opacity-90 transition-all"
                  disabled={rankSaving}
                  onClick={async () => {
                    setRankSaving(true);
                    try {
                      const payload = {
                        ...rankConfig,
                        inactive_after_accessed_secs: displayToSeconds(rankThreshold.value, rankThreshold.unit),
                        inactive_rank_update_period: displayToSeconds(rankPeriod.value, rankPeriod.unit),
                      };
                      await updateRankConfig(payload);
                      setRankMessage('✅ 保存成功');
                    } catch (e) {
                      setRankMessage('❌ 保存失败: ' + e.message);
                    }
                    setRankSaving(false);
                    setTimeout(() => setRankMessage(''), 2000);
                  }}>
                  {rankSaving ? '保存中...' : '保存配置'}
                </button>
                {rankMessage && (
                  <span className="text-xs text-[var(--text-secondary)]">{rankMessage}</span>
                )}
              </div>
            </>
          ) : (
            <p className="text-[var(--text-tertiary)] text-sm text-center py-8">加载配置中...</p>
          )}
        </div>
      )}

      {/* ─── Web Search ─── */}
      {tab === 'websearch' && (
        <div>
          {editingWebProvider ? (
            <div className="space-y-3.5">
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">名称</label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-transparent border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm"
                  type="text" value={editingWebProvider.name}
                  onChange={(e) => setEditingWebProvider({ ...editingWebProvider, name: e.target.value })} />
              </div>
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">Search URL <span className="text-[var(--text-muted)]">(用 <code>{'{text}'}</code> 表示搜索词位置)</span></label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-transparent border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm"
                  type="text" value={editingWebProvider.search_url}
                  onChange={(e) => setEditingWebProvider({ ...editingWebProvider, search_url: e.target.value })} />
              </div>
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">请求方法 <span className="text-[var(--text-muted)]">(GET 或 POST)</span></label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-transparent border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm"
                  type="text" value={editingWebProvider.method || 'GET'}
                  onChange={(e) => setEditingWebProvider({ ...editingWebProvider, method: e.target.value })} />
              </div>
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">请求体模板 <span className="text-[var(--text-muted)]">(POST 时使用，用 <code>{'{text}'}</code> 表示搜索词)</span></label>
                <textarea className="w-full h-24 px-3 py-2 rounded-xl bg-transparent border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-xs font-mono resize-none"
                  value={editingWebProvider.body_template || ''}
                  onChange={(e) => setEditingWebProvider({ ...editingWebProvider, body_template: e.target.value || null })} />
              </div>
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">请求参数</label>
                <div className="space-y-1">
                  {Object.entries(editingWebProvider.params || {}).length === 0 && <div className="text-xs text-[var(--text-muted)] italic">—</div>}
                  {Object.entries(editingWebProvider.params || {}).map(([k, v], idx) => (
                    <div key={idx} className="flex items-start gap-1 py-1 px-2 rounded-lg bg-[var(--accent-bg)]">
                      <input className="flex-1 px-2 py-1 rounded-md bg-transparent text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                        value={k} placeholder="key"
                        onChange={(e) => {
                          const { [k]: _, ...rest } = editingWebProvider.params;
                          setEditingWebProvider({ ...editingWebProvider, params: { ...rest, [e.target.value]: v } });
                        }} />
                      <input className="flex-1 px-2 py-1 rounded-md bg-transparent text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                        value={v} placeholder="value"
                        onChange={(e) => setEditingWebProvider({ ...editingWebProvider, params: { ...editingWebProvider.params, [k]: e.target.value } })} />
                      <button className="flex-shrink-0 w-5 h-5 rounded-md bg-[var(--bg-hover)] hover:bg-[var(--danger)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-white text-[10px]"
                        onClick={() => {
                          const { [k]: _, ...rest } = editingWebProvider.params;
                          setEditingWebProvider({ ...editingWebProvider, params: rest });
                        }}>✕</button>
                    </div>
                  ))}
                  <button className="w-full py-1 rounded-lg border border-dashed border-[var(--border)] text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:border-[var(--accent)] text-xs font-medium transition-all"
                    onClick={() => setEditingWebProvider({ ...editingWebProvider, params: { ...editingWebProvider.params, '': '' } })}>+ 添加参数</button>
                </div>
              </div>
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">请求头</label>
                <div className="space-y-1">
                  {Object.entries(editingWebProvider.headers || {}).length === 0 && <div className="text-xs text-[var(--text-muted)] italic">—</div>}
                  {Object.entries(editingWebProvider.headers || {}).map(([k, v], idx) => (
                    <div key={idx} className="flex items-start gap-1 py-1 px-2 rounded-lg bg-[var(--accent-bg)]">
                      <input className="flex-1 px-2 py-1 rounded-md bg-transparent text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                        value={k} placeholder="key"
                        onChange={(e) => {
                          const { [k]: _, ...rest } = editingWebProvider.headers;
                          setEditingWebProvider({ ...editingWebProvider, headers: { ...rest, [e.target.value]: v } });
                        }} />
                      <input className="flex-1 px-2 py-1 rounded-md bg-transparent text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                        value={v} placeholder="value"
                        onChange={(e) => setEditingWebProvider({ ...editingWebProvider, headers: { ...editingWebProvider.headers, [k]: e.target.value } })} />
                      <button className="flex-shrink-0 w-5 h-5 rounded-md bg-[var(--bg-hover)] hover:bg-[var(--danger)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-white text-[10px]"
                        onClick={() => {
                          const { [k]: _, ...rest } = editingWebProvider.headers;
                          setEditingWebProvider({ ...editingWebProvider, headers: rest });
                        }}>✕</button>
                    </div>
                  ))}
                  <button className="w-full py-1 rounded-lg border border-dashed border-[var(--border)] text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:border-[var(--accent)] text-xs font-medium transition-all"
                    onClick={() => setEditingWebProvider({ ...editingWebProvider, headers: { ...editingWebProvider.headers, '': '' } })}>+ 添加请求头</button>
                </div>
              </div>
              <div className="flex gap-2 justify-end pt-1">
                <button className="px-4 py-2 rounded-xl bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-sm font-medium transition-all" onClick={() => setEditingWebProvider(null)}>取消</button>
                <button className="px-4 py-2 rounded-xl bg-[var(--accent)] text-white text-sm font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm" onClick={() => {
                  if (!editingWebProvider.name || !editingWebProvider.search_url) return;
                  const existing = webSearchConfig.providers.findIndex((p) => p.id === editingWebProvider.id);
                  const updated = existing >= 0
                    ? [...webSearchConfig.providers.slice(0, existing), editingWebProvider, ...webSearchConfig.providers.slice(existing + 1)]
                    : [...webSearchConfig.providers, editingWebProvider];
                  setWebSearchConfig({ ...webSearchConfig, providers: updated });
                  setEditingWebProvider(null);
                }}>保存</button>
              </div>
            </div>
          ) : (
            <div>
              <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-2 tracking-tight">默认供应商</label>
              <div className="relative">
                <button
                  className="w-full px-3 py-2 rounded-xl bg-transparent text-[var(--text-primary)] text-sm border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] transition-all font-medium flex items-center gap-1 text-left"
                  onClick={(e) => { e.stopPropagation(); setDefaultWebProviderOpen(!defaultWebProviderOpen); }}
                >
                  <span className="flex-1 truncate">{webSearchConfig?.providers?.find(p => p.id === webSearchConfig?.default_provider)?.name || '(无)'}</span>
                  <svg className={`w-3 h-3 flex-shrink-0 transition-transform ${defaultWebProviderOpen ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}><path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" /></svg>
                </button>
                {defaultWebProviderOpen && (
                  <>
                    <div className="fixed inset-0 z-40" onClick={() => setDefaultWebProviderOpen(false)} />
                    <div className="absolute left-0 top-full mt-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden w-full max-h-[300px] overflow-y-auto">
                      <button
                        className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap truncate transition-all ${!webSearchConfig?.default_provider ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                        onClick={() => { setWebSearchConfig({ ...webSearchConfig, default_provider: '' }); setDefaultWebProviderOpen(false); }}
                      >(无)</button>
                      {(webSearchConfig?.providers || []).map((p) => (
                        <button
                          key={p.id}
                          className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap truncate transition-all ${p.id === webSearchConfig?.default_provider ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                          onClick={() => { setWebSearchConfig({ ...webSearchConfig, default_provider: p.id }); setDefaultWebProviderOpen(false); }}
                        >{p.name}</button>
                      ))}
                    </div>
                  </>
                )}
              </div>

              <label className="block text-xs text-[var(--text-tertiary)] font-medium mt-4 mb-2 tracking-tight">供应商列表</label>
              {(!webSearchConfig?.providers || webSearchConfig.providers.length === 0) && (
                <p className="text-[var(--text-tertiary)] text-sm text-center py-8 tracking-tight">暂无供应商</p>
              )}
              <div className="space-y-1 max-h-48 overflow-y-auto mb-3">
                {(webSearchConfig?.providers || []).map((p) => (
                  <div key={p.id} className="flex items-center justify-between py-2.5 px-3 rounded-xl hover:bg-[var(--bg-tertiary)] transition-all group">
                    <div>
                      <div className="text-sm text-[var(--text-primary)] font-medium">{p.name}</div>
                      <div className="text-xs text-[var(--text-tertiary)] mt-0.5 truncate max-w-[300px]">{p.search_url}</div>
                    </div>
                    <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button className="px-2.5 py-1 text-xs text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] rounded-lg transition-all" onClick={() => setEditingWebProvider({ ...p })}>编辑</button>
                      <button className="px-2.5 py-1 text-xs text-[var(--danger)] hover:bg-[color-mix(in srgb, var(--bg-hover), var(--danger) 30%)] rounded-lg transition-all" onClick={() => {
                        setWebSearchConfig({ ...webSearchConfig, providers: webSearchConfig.providers.filter((x) => x.id !== p.id) });
                      }}>删除</button>
                    </div>
                  </div>
                ))}
              </div>
              <button className="w-full py-2.5 rounded-xl bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] text-sm font-medium transition-all" onClick={() => {
                setEditingWebProvider({
                  id: Date.now().toString(),
                  name: '',
                  search_url: '',
                  method: 'GET',
                  body_template: null,
                  params: {},
                  headers: {},
                });
              }}>+ 添加供应商</button>

              <div className="flex items-center gap-3 pt-3">
                <button className="px-4 py-2 rounded-xl bg-[var(--accent)] text-white text-sm font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm disabled:opacity-40"
                  disabled={webSearchSaving}
                  onClick={async () => {
                    if (!webSearchConfig) return;
                    setWebSearchSaving(true);
                    setWebSearchMessage('');
                    try {
                      await updateWebSearchConfig(webSearchConfig);
                      setWebSearchMessage('✅ 保存成功');
                    } catch (e) {
                      setWebSearchMessage('❌ 保存失败: ' + e.message);
                    } finally {
                      setWebSearchSaving(false);
                      setTimeout(() => setWebSearchMessage(''), 2000);
                    }
                  }}>
                  {webSearchSaving ? '保存中...' : '保存配置'}
                </button>
                {webSearchMessage && (
                  <span className="text-xs text-[var(--text-secondary)]">{webSearchMessage}</span>
                )}
              </div>
            </div>
          )}
        </div>
      )}
    </Modal>

  );
}