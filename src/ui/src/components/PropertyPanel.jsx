import { useTranslation } from 'react-i18next';

export default function PropertyPanel({ item, type, onClose }) {
  const { t } = useTranslation();
  if (!item) return null;

  const props = item.properties || {};
  const labels = item.labels || [];

  return (
    <div className="w-80 bg-gray-800 border-l border-gray-700 p-4 overflow-y-auto text-sm">
      <div className="flex justify-between items-center mb-4">
        <span className="text-gray-300 font-semibold">{type === 'vertex' ? t('panel.vertex') : t('panel.edge')}</span>
        <button className="text-gray-500 hover:text-white" onClick={onClose}>✕</button>
      </div>
      <div className="space-y-2">
        <div><span className="text-gray-500">{t('panel.id')}:</span> <span className="text-gray-200 ml-2">{item.id}</span></div>
        {labels.length > 0 && (
          <div><span className="text-gray-500">{t('panel.labels')}:</span>
            <div className="flex flex-wrap gap-1 mt-1">
              {labels.map((l, i) => <span key={i} className="px-2 py-0.5 rounded bg-blue-900 text-blue-200 text-xs">{l}</span>)}
            </div>
          </div>
        )}
        <div>
          <span className="text-gray-500 block mb-1">{t('panel.properties')}:</span>
          {Object.keys(props).length === 0 && <span className="text-gray-600 italic">—</span>}
          {Object.entries(props).map(([k, v]) => (
            <div key={k} className="flex justify-between items-center py-1 border-b border-gray-700 last:border-0">
              <span className="text-gray-400 mr-2">{k}:</span>
              <span className="text-gray-200 text-right truncate max-w-[180px]">{String(v)}</span>
            </div>
          ))}
        </div>
        {type === 'edge' && (
          <>
            <div><span className="text-gray-500">source:</span> <span className="text-gray-200 ml-2">{item.source}</span></div>
            <div><span className="text-gray-500">target:</span> <span className="text-gray-200 ml-2">{item.target}</span></div>
          </>
        )}
      </div>
    </div>
  );
}
