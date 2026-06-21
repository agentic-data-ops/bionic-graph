import { useTranslation } from 'react-i18next';

export default function PropertyPanel({ item, type, onClose }) {
  const { t } = useTranslation();
  if (!item) return null;

  const props = item.properties || {};
  const labels = item.labels || [];

  return (
    <div className="w-80 bg-[#1c1c20] border border-[#2a2a2e] rounded-2xl p-4 overflow-y-auto text-sm shadow-xl">
      {/* Header */}
      <div className="flex justify-between items-center mb-4">
        <span className="text-[#e5e5e7] font-semibold tracking-tight text-sm">
          {type === 'vertex' ? t('panel.vertex') : t('panel.edge')}
        </span>
        <button className="w-6 h-6 rounded-lg bg-[#2a2a2e] hover:bg-[#3a3a3e] flex items-center justify-center text-[#636366] hover:text-white transition-all text-xs" onClick={onClose}>
          <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>

      <div className="space-y-3">
        {/* ID */}
        <div className="flex items-center gap-2">
          <span className="text-xs text-[#636366] font-medium w-12">{t('panel.id')}</span>
          <span className="text-sm text-[#e5e5e7] font-mono">{item.id}</span>
        </div>

        {/* Labels */}
        {labels.length > 0 && (
          <div>
            <span className="text-xs text-[#636366] font-medium block mb-1.5">{t('panel.labels')}</span>
            <div className="flex flex-wrap gap-1.5">
              {labels.map((l, i) => (
                <span key={i} className="px-2.5 py-0.5 rounded-lg bg-[#0a84ff]/15 text-[#0a84ff] text-xs font-medium">
                  {l}
                </span>
              ))}
            </div>
          </div>
        )}

        {/* Properties */}
        <div>
          <span className="text-xs text-[#636366] font-medium block mb-1.5">{t('panel.properties')}</span>
          {Object.keys(props).length === 0 ? (
            <span className="text-xs text-[#48484a] italic">—</span>
          ) : (
            <div className="space-y-0.5 bg-[#2a2a2e] rounded-xl p-2.5">
              {Object.entries(props).map(([k, v]) => (
                <div key={k} className="flex justify-between items-center py-1 px-1.5 rounded-lg hover:bg-[#3a3a3e] transition-all">
                  <span className="text-xs text-[#636366] font-medium mr-3">{k}</span>
                  <span className="text-xs text-[#e5e5e7] text-right truncate max-w-[160px] font-mono">{String(v)}</span>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Edge source/target */}
        {type === 'edge' && (
          <>
            <div className="flex items-center gap-2">
              <span className="text-xs text-[#636366] font-medium w-12">source</span>
              <span className="text-sm text-[#e5e5e7] font-mono">{item.source}</span>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-xs text-[#636366] font-medium w-12">target</span>
              <span className="text-sm text-[#e5e5e7] font-mono">{item.target}</span>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
