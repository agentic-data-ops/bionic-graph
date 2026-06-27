import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { fetchNeuralConfig, updateNeuralConfig } from '../api';

function Modal({ title, children, onClose }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center" onClick={onClose}>
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
      ? 'bg-[var(--bg-hover)] text-white shadow-sm'
      : 'text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]'
  }`;

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
  const [neuralConfig, setNeuralConfig] = useState(null);
  const [neuralSaving, setNeuralSaving] = useState(false);
  const [neuralMessage, setNeuralMessage] = useState('');
  const f3 = (v) => v !== undefined && v !== null ? Number(v).toFixed(3) : '';
  useEffect(() => {
    if (open) {
      fetchNeuralConfig().then((d) => {
        const nc = d.neural || {};
        setNeuralConfig({ ...nc.activate, ...nc.search, ...nc.learn });
      }).catch(() => {});
      setNeuralMessage('');
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

  return (
    <Modal title={t('settings.title')} onClose={onClose}>
      {/* Tabs */}
      <div className="flex gap-1.5 mb-5">
        <button className={tabCls(tab === 'providers')} onClick={() => setTab('providers')}>{t('settings.model')}</button>
        <button className={tabCls(tab === 'search')} onClick={() => setTab('search')}>神经元</button>
      </div>

      {/* ─── Providers ─── */}
      {tab === 'providers' && (
        <div>
          {editingProvider ? (
            <div className="space-y-3.5">
              {/* Provider name */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('settings.providerName')}</label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm placeholder-[var(--text-muted)]"
                  type="text" value={editingProvider.name}
                  onChange={(e) => setEditingProvider({ ...editingProvider, name: e.target.value })} />
              </div>

              {/* API Base URL */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">API Base URL</label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm placeholder-[var(--text-muted)]"
                  type="text" value={editingProvider.apiBase}
                  onChange={(e) => setEditingProvider({ ...editingProvider, apiBase: e.target.value })} />
              </div>

              {/* API Key */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">API Key</label>
                <input className="w-full px-3.5 py-2 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm placeholder-[var(--text-muted)]"
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
                  <input className="flex-1 px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-xs placeholder-[var(--text-muted)]"
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
                <select className="w-full px-3 py-2 rounded-xl bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-sm border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] appearance-none cursor-pointer"
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
          {neuralConfig ? (
            <div className="space-y-4">
              {/* ── Activation ── */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">激活参数</label>
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">Max Ticks</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" min="1" max="100" value={neuralConfig.max_ticks}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, max_ticks: parseInt(e.target.value) || 20 })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">Hot Threshold</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.hot_threshold)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, hot_threshold: parseFloat(e.target.value) || 0 })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">Min Synapse</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.min_synapse_strength)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, min_synapse_strength: parseFloat(e.target.value) || 0 })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">默认阈值</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.default_threshold)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, default_threshold: parseFloat(e.target.value) || 0 })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">衰减率</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.default_decay_rate)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, default_decay_rate: parseFloat(e.target.value) || 0 })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">不应期(ticks)</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" min="0" max="20" value={neuralConfig.default_refractory_ticks}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, default_refractory_ticks: parseInt(e.target.value) || 0 })} />
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-2 mb-2">
                <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none">
                  <input type="checkbox" checked={neuralConfig.auto_stabilize} onChange={(e) => setNeuralConfig({ ...neuralConfig, auto_stabilize: e.target.checked })}
                    className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
                  自动稳定 (Auto Stabilize)
                </label>
              </div>

              {/* ── Search Mode ── */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">搜索模式 &amp; 分数</label>
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">默认搜索模式</label>
                    <select className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm appearance-none cursor-pointer"
                      value={neuralConfig.default_search_mode}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, default_search_mode: e.target.value })}>
                      <option value="greedy">Greedy (贪婪)</option>
                      <option value="exact">Exact (精确)</option>
                    </select>
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">贪婪精确匹配分数</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.greedy_exact_score)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, greedy_exact_score: parseFloat(e.target.value) || 0 })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">贪婪部分匹配分数</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.greedy_partial_score)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, greedy_partial_score: parseFloat(e.target.value) || 0 })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">精确模式最低分数</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.exact_min_score)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, exact_min_score: parseFloat(e.target.value) || 0 })} />
                  </div>
                </div>
              </div>

              {/* ── Mode Thresholds ── */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">搜索模式激活阈值</label>
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">贪婪模式 (Greedy)</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.greedy_threshold)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, greedy_threshold: parseFloat(e.target.value) || 0 })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">精确模式 (Exact)</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.exact_threshold)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, exact_threshold: parseFloat(e.target.value) || 0 })} />
                  </div>
                </div>
              </div>

              {/* ── Fuzzy Matching ── */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">模糊匹配</label>
                <div className="flex items-center gap-2 mb-2">
                  <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none">
                    <input type="checkbox" checked={neuralConfig.fuzzy_match_enabled} onChange={(e) => setNeuralConfig({ ...neuralConfig, fuzzy_match_enabled: e.target.checked })}
                      className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
                    启用模糊匹配 (Levenshtein)
                  </label>
                </div>
                <div className="grid grid-cols-1 gap-3">
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">模糊匹配阈值 (0=严格, 1=宽松)</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.fuzzy_match_threshold)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, fuzzy_match_threshold: parseFloat(e.target.value) || 0 })} />
                  </div>
                </div>
              </div>

              {/* ── Learning ── */}
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">Hebbian 学习</label>
                <div className="flex items-center gap-2 mb-2">
                  <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none">
                    <input type="checkbox" checked={neuralConfig.learning_enabled} onChange={(e) => setNeuralConfig({ ...neuralConfig, learning_enabled: e.target.checked })}
                      className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
                    启用学习
                  </label>
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">共火窗口 (ticks)</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" min="1" max="50" value={neuralConfig.co_fire_window}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, co_fire_window: parseInt(e.target.value) || 5 })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">最小可塑性</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.min_plasticity)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, min_plasticity: parseFloat(e.target.value) || 0 })} />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-muted)]">突触衰减率</label>
                    <input className="w-full px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-sm"
                      type="number" step="0.001" min="0" max="1" value={f3(neuralConfig.synaptic_decay)}
                      onChange={(e) => setNeuralConfig({ ...neuralConfig, synaptic_decay: parseFloat(e.target.value) || 0 })} />
                  </div>
                </div>
              </div>

              {/* Save button */}
              <div className="flex items-center gap-3 pt-1">
                <button className="px-4 py-2 rounded-xl bg-[var(--accent)] text-white text-sm font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm disabled:opacity-40"
                  disabled={neuralSaving}
                  onClick={async () => {
                    setNeuralSaving(true);
                    setNeuralMessage('');
                    try {
                      await updateNeuralConfig(neuralConfig);
                      setNeuralMessage('✅ 保存成功 — 重启服务后生效');
                    } catch (e) {
                      setNeuralMessage('❌ 保存失败: ' + e.message);
                    } finally {
                      setNeuralSaving(false);
                    }
                  }}>
                  {neuralSaving ? '保存中...' : '保存配置'}
                </button>
                {neuralMessage && (
                  <span className="text-xs text-[var(--text-secondary)]">{neuralMessage}</span>
                )}
              </div>
            </div>
          ) : (
            <p className="text-[var(--text-tertiary)] text-sm text-center py-8 tracking-tight">加载配置中...</p>
          )}
        </div>
      )}
    </Modal>

  );
}