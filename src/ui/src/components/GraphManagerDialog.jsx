import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { listGraphs, createGraph, deleteGraph, compact } from '../api';

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

export default function GraphManagerDialog({
  open,
  onClose,
  graphName,
  onGraphNameChange,
  graphs,
  onGraphsChange,
}) {
  const { t } = useTranslation();
  const [showAddGraph, setShowAddGraph] = useState(false);
  const [newGraphName, setNewGraphName] = useState('');
  const [newGraphTT, setNewGraphTT] = useState(false);
  const [timeTravelGraphs, setTimeTravelGraphs] = useState({});

  useEffect(() => {
    if (open) {
      listGraphs().then((d) => {
        onGraphsChange?.(d.graphs || []);
        setTimeTravelGraphs(d.time_travel || {});
      }).catch(() => {});
    }
  }, [open, onGraphsChange]);

  const handleAddGraph = async () => {
    if (!newGraphName) return;
    await createGraph(newGraphName, newGraphTT);
    const updated = await listGraphs().then((d) => {
      setTimeTravelGraphs(d.time_travel || {});
      return d.graphs || [];
    });
    onGraphsChange(updated);
    setShowAddGraph(false);
    setNewGraphName('');
  };

  const handleDeleteGraph = async (name) => {
    await deleteGraph(name);
    const updated = await listGraphs().then((d) => {
      setTimeTravelGraphs(d.time_travel || {});
      return d.graphs || [];
    });
    onGraphsChange(updated);
    if (graphName === name && updated.length > 0) onGraphNameChange(updated[0]);
  };

  if (!open) return null;

  return (
    <Modal title={t('settings.graphs')} onClose={onClose}>
      <div>
        <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-2 tracking-tight">{t('settings.graphList')}</label>

        <div className="space-y-1 max-h-48 overflow-y-auto mb-3">
          {graphs.length === 0 && (
            <p className="text-[var(--text-tertiary)] text-sm text-center py-8">暂无图库</p>
          )}
          {graphs.map((g) => (
            <div key={g} className="flex items-center justify-between py-2 px-3 rounded-xl hover:bg-[var(--bg-tertiary)] transition-all group">
              <div className="flex items-center gap-2 min-w-0">
                <span className="text-sm text-[var(--text-primary)] font-medium truncate">{g}</span>
                {g === graphName && (
                  <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-[var(--accent)]/20 text-[var(--accent)] border border-[#0a84ff]/30 flex-shrink-0">{t('settings.defaultGraph')}</span>
                )}
                {timeTravelGraphs[g] && (
                  <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-[#ff9f0a]/20 text-[#ff9f0a] border border-[#ff9f0a]/30 flex-shrink-0">时光</span>
                )}
              </div>
              <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 ml-2">
                {g !== graphName && (
                  <button className="px-2 py-1 text-xs text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] rounded-lg transition-all" onClick={() => onGraphNameChange(g)}>{t('settings.defaultGraph')}</button>
                )}
                {timeTravelGraphs[g] && (
                  <button className="px-2 py-1 text-xs text-[#ff9f0a] hover:bg-[color-mix(in srgb, var(--bg-hover), orange 20%)] rounded-lg transition-all" onClick={async () => {
                    const days = parseInt(prompt('Compaction days (default: 7):', '7') || '7');
                    if (days > 0) { const before = (Date.now() - days * 86400 * 1000) * 1000; await compact(before, g); }
                  }}>归档</button>
                )}
                <button className="px-2 py-1 text-xs text-[var(--danger)] hover:bg-[color-mix(in srgb, var(--bg-hover), var(--danger) 30%)] rounded-lg transition-all" onClick={() => handleDeleteGraph(g)}>{t('settings.delete')}</button>
              </div>
            </div>
          ))}
        </div>

        {showAddGraph ? (
          <div className="space-y-3 mb-3 p-4 bg-[var(--bg-tertiary)] rounded-xl">
            <input className="w-full px-3.5 py-2 rounded-xl bg-[var(--bg-secondary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] text-[var(--text-primary)] text-sm placeholder-[var(--text-muted)]"
              placeholder={t('modal.addGraphName')} value={newGraphName} onChange={(e) => setNewGraphName(e.target.value)} />
            <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none">
              <input type="checkbox" checked={newGraphTT} onChange={(e) => setNewGraphTT(e.target.checked)}
                className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
              {t('modal.addGraphTimeTravel')}
            </label>
            <div className="flex gap-2 justify-end">
              <button className="px-3.5 py-1.5 rounded-xl bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium transition-all" onClick={() => setShowAddGraph(false)}>{t('panel.close')}</button>
              <button className="px-3.5 py-1.5 rounded-xl bg-[var(--accent)] text-white text-xs font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm" onClick={handleAddGraph}>{t('modal.addGraphConfirm')}</button>
            </div>
          </div>
        ) : (
          <button className="w-full py-2.5 rounded-xl bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] text-sm font-medium transition-all" onClick={() => setShowAddGraph(true)}>
            + {t('modal.addGraphTitle')}
          </button>
        )}
      </div>
    </Modal>
  );
}
