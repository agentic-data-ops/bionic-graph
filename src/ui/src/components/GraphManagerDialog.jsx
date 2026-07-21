import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { listGraphs, createGraph, deleteGraph, setDefaultGraph, updateGraphMeta } from '../api';

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

export default function GraphManagerDialog({
  open,
  onClose,
  graphName,
  onGraphNameChange,
  graphMetas,
  onGraphMetasChange,
}) {
  const { t } = useTranslation();
  const [showAddGraph, setShowAddGraph] = useState(false);
  const [newGraphName, setNewGraphName] = useState('');
  const [newGraphDesc, setNewGraphDesc] = useState('');
  const [newGraphTT, setNewGraphTT] = useState(true);
  const [editingMeta, setEditingMeta] = useState(null); // { name, description, time_travel }
  const [backendDefault, setBackendDefault] = useState(''); // true default from backend

  // Reload metas + backend default from backend when dialog opens.
  useEffect(() => {
    if (open) {
      listGraphs().then((d) => {
        onGraphMetasChange?.(d.graphs || []);
        setBackendDefault(d.default || '');
      }).catch(() => {});
    }
  }, [open, onGraphMetasChange]);

  const handleAddGraph = async () => {
    if (!newGraphName) return;
    await createGraph(newGraphName, newGraphDesc, newGraphTT);
    const d = await listGraphs();
    onGraphMetasChange(d.graphs || []);
    setShowAddGraph(false);
    setNewGraphName('');
    setNewGraphDesc('');
    setNewGraphTT(false);
  };

  const handleDeleteGraph = async (name) => {
    await deleteGraph(name);
    const d = await listGraphs();
    const metas = d.graphs || [];
    onGraphMetasChange(metas);
    if (graphName === name && metas.length > 0) onGraphNameChange(metas[0].name);
  };

  const handleSetDefault = async (name) => {
    await setDefaultGraph(name);
    setBackendDefault(name);
    const d = await listGraphs();
    onGraphMetasChange(d.graphs || []);
  };

  const handleSaveMeta = async () => {
    if (!editingMeta) return;
    await updateGraphMeta(editingMeta.name, editingMeta.description, editingMeta.time_travel);
    setEditingMeta(null);
    const d = await listGraphs();
    onGraphMetasChange(d.graphs || []);
  };

  if (!open) return null;

  return (
    <Modal title={t('settings.graphs')} onClose={onClose}>
      <div>
        <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-2 tracking-tight">{t('settings.graphList')}</label>

        <div className="space-y-1 max-h-48 overflow-y-auto mb-3">
          {(graphMetas || []).length === 0 && (
            <p className="text-[var(--text-tertiary)] text-sm text-center py-8">暂无图库</p>
          )}
          {(graphMetas || []).map((meta) => (
            <div key={meta.name} className="flex items-center justify-between py-2 px-3 rounded-xl hover:bg-[var(--bg-tertiary)] transition-all group">
              <div className="flex items-center gap-2 min-w-0 flex-1">
                <span className="text-sm text-[var(--text-primary)] font-medium truncate">{meta.name}</span>
                {meta.description && (
                  <span className="text-xs text-[var(--text-tertiary)] truncate max-w-[120px]">{meta.description}</span>
                )}
                {meta.time_travel && (
                  <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-[#ff9f0a]/20 text-[#ff9f0a] border border-[#ff9f0a]/30 flex-shrink-0">时光</span>
                )}
                {meta.name === backendDefault && (
                  <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-[var(--accent)]/20 text-[var(--accent)] border border-[#0a84ff]/30 flex-shrink-0">{t('settings.defaultGraph')}</span>
                )}
              </div>
              <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 ml-2">
                {meta.name !== backendDefault && (
                  <button className="px-2 py-1 text-xs text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] rounded-lg transition-all cursor-pointer"
                    onClick={() => handleSetDefault(meta.name)}>设为默认</button>
                )}
                <button className="px-2 py-1 text-xs text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] rounded-lg transition-all cursor-pointer"
                  onClick={() => setEditingMeta({ name: meta.name, description: meta.description || '', time_travel: meta.time_travel || false })}>
                  编辑
                </button>
                <button className="px-2 py-1 text-xs text-[var(--danger)] hover:bg-[color-mix(in srgb, var(--bg-hover), var(--danger) 30%)] rounded-lg transition-all cursor-pointer"
                  onClick={() => {
                    const msg = `确定要删除图库「${meta.name}」吗？\n此操作不可恢复，所有数据将被永久清除。`;
                    if (window.confirm(msg)) handleDeleteGraph(meta.name);
                  }}>{t('settings.delete')}</button>
              </div>
            </div>
          ))}
        </div>

        {/* Edit meta dialog */}
        {editingMeta && (
          <div className="space-y-3 mb-3 p-4 bg-[var(--bg-tertiary)] rounded-xl">
            <div className="text-xs font-medium text-[var(--text-primary)]">编辑图库：{editingMeta.name}</div>
            <input className="w-full px-3.5 py-2 rounded-xl bg-[var(--bg-secondary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm placeholder-[var(--text-muted)]"
              placeholder="描述信息" value={editingMeta.description}
              onChange={(e) => setEditingMeta({ ...editingMeta, description: e.target.value })} />
            <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none">
              <input type="checkbox" checked={editingMeta.time_travel}
                onChange={(e) => setEditingMeta({ ...editingMeta, time_travel: e.target.checked })}
                className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
              启用时间旅行
            </label>
            <div className="flex gap-2 justify-end">
              <button className="px-3.5 py-1.5 rounded-xl bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium transition-all"
                onClick={() => setEditingMeta(null)}>{t('panel.close')}</button>
              <button className="px-3.5 py-1.5 rounded-xl bg-[var(--accent)] text-white text-xs font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm"
                onClick={handleSaveMeta}>保存</button>
            </div>
          </div>
        )}

        {/* Add graph form */}
        {showAddGraph ? (
          <div className="space-y-3 mb-3 p-4 bg-[var(--bg-tertiary)] rounded-xl">
            <input className="w-full px-3.5 py-2 rounded-xl bg-[var(--bg-secondary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm placeholder-[var(--text-muted)]"
              placeholder={t('modal.addGraphName')} value={newGraphName} onChange={(e) => setNewGraphName(e.target.value)} />
            <input className="w-full px-3.5 py-2 rounded-xl bg-[var(--bg-secondary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm placeholder-[var(--text-muted)]"
              placeholder="描述（可选）" value={newGraphDesc} onChange={(e) => setNewGraphDesc(e.target.value)} />
            <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none">
              <input type="checkbox" checked={newGraphTT} onChange={(e) => setNewGraphTT(e.target.checked)}
                className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
              {t('modal.addGraphTimeTravel')}
            </label>
            <div className="flex gap-2 justify-end">
              <button className="px-3.5 py-1.5 rounded-xl bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium transition-all"
                onClick={() => setShowAddGraph(false)}>{t('panel.close')}</button>
              <button className="px-3.5 py-1.5 rounded-xl bg-[var(--accent)] text-white text-xs font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm"
                onClick={handleAddGraph}>{t('modal.addGraphConfirm')}</button>
            </div>
          </div>
        ) : (
          <button className="w-full py-2.5 rounded-xl bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] text-sm font-medium transition-all"
            onClick={() => setShowAddGraph(true)}>
            + {t('modal.addGraphTitle')}
          </button>
        )}
      </div>
    </Modal>
  );
}
