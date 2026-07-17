import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { getDocument, getDocumentContent } from '../api';

function DocViewer({ docId, onClose }) {
  const { t } = useTranslation();
  const [doc, setDoc] = useState(null);
  const [content, setContent] = useState('');
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!docId) return;
    setLoading(true);
    Promise.all([
      getDocument(docId).catch(() => null),
      getDocumentContent(docId).catch(() => ''),
    ]).then(([d, c]) => {
      setDoc(d);
      setContent(c || '');
      setLoading(false);
    });
  }, [docId]);

  return (
    <div className="fixed inset-0 z-[200] flex items-center justify-center" onClick={onClose}>
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
      <div className="relative bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl max-w-2xl max-h-[80vh] overflow-y-auto shadow-2xl min-w-[500px]"
        onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between p-4 border-b border-[var(--border)]">
          <span className="text-sm font-semibold text-[var(--text-primary)]">
            {loading ? t('graph.loading') : (doc?.title || 'Document')}
          </span>
          <button className="w-6 h-6 rounded-lg bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-all text-xs" onClick={onClose}>✕</button>
        </div>
        <div className="p-4">
          {doc?.tags?.length > 0 && (
            <div className="flex flex-wrap gap-1.5 mb-4">
              {doc.tags.map((tag, i) => (
                <span key={i} className="px-2 py-0.5 rounded-md bg-[var(--accent)]/15 text-[var(--accent)] text-xs font-medium">{tag}</span>
              ))}
            </div>
          )}
          <div className="text-sm text-[var(--text-primary)] whitespace-pre-wrap leading-relaxed">
            {loading ? t('graph.loading') : (content || '—')}
          </div>
        </div>
      </div>
    </div>
  );
}

export default function PropertyPanel({ item, type, onClose }) {
  const { t } = useTranslation();
  const [sourceDocName, setSourceDocName] = useState('');
  const [showDoc, setShowDoc] = useState(null);

  if (!item) return null;

  const props = item.properties || {};
  const labels = item.labels || [];
  const sourceDocId = props._source_doc_id;
  const displayProps = Object.fromEntries(
    Object.entries(props).filter(([k]) => k !== '_source_doc_id')
  );

  // Fetch source document name
  useEffect(() => {
    if (!sourceDocId) { setSourceDocName(''); return; }
    getDocument(sourceDocId).then((doc) => {
      if (doc && doc.title) setSourceDocName(doc.title);
    }).catch(() => {});
  }, [sourceDocId]);

  return (
    <div className="w-80 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-4 overflow-y-auto text-sm shadow-xl">
      {/* Header */}
      <div className="flex justify-between items-center mb-4">
        <span className="text-[var(--text-primary)] font-semibold tracking-tight text-sm">
          {type === 'vertex' ? t('panel.vertex') : t('panel.edge')}
        </span>
        <button className="w-6 h-6 rounded-lg bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-all text-xs" onClick={onClose}>
          <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>

      <div className="space-y-3">
        {/* ID */}
        <div className="flex items-center gap-2">
          <span className="text-xs text-[var(--text-tertiary)] font-medium w-12">{t('panel.id')}</span>
          <span className="text-sm text-[var(--text-primary)] font-mono">{item.id}</span>
        </div>

        {/* Name */}
        <div className="flex items-center gap-2">
          <span className="text-xs text-[var(--text-tertiary)] font-medium w-12">{t('panel.name')}</span>
          <span className="text-sm text-[var(--text-primary)] font-medium">{item.name || '—'}</span>
        </div>

        {/* Labels */}
        {labels.length > 0 && (
          <div>
            <span className="text-xs text-[var(--text-tertiary)] font-medium block mb-1.5">{t('panel.labels')}</span>
            <div className="flex flex-wrap gap-1.5">
              {labels.map((l, i) => (
                <span key={i} className="px-2.5 py-0.5 rounded-lg bg-[var(--accent)]/15 text-[var(--accent)] text-xs font-medium">
                  {l}
                </span>
              ))}
            </div>
          </div>
        )}

        {/* Keywords */}
        {(item.keywords || []).length > 0 && (
          <div>
            <span className="text-xs text-[var(--text-tertiary)] font-medium block mb-1.5">{t('panel.keywords')}</span>
            <div className="flex flex-wrap gap-1.5">
              {item.keywords.map((kw, i) => (
                <span key={i} className="px-2.5 py-0.5 rounded-lg bg-[var(--success-bg)] text-[var(--success)] text-xs font-medium">
                  {kw}
                </span>
              ))}
            </div>
          </div>
        )}

        {/* Source Document (from _source_doc_id) */}
        {sourceDocId && (
          <div>
            <span className="text-xs text-[var(--text-tertiary)] font-medium block mb-1.5">{t('panel.sourceDocument')}</span>
            <button
              className="text-xs text-[var(--accent)] hover:underline text-left break-all"
              onClick={() => setShowDoc(sourceDocId)}
            >{sourceDocName || `#${sourceDocId.slice(0,8)}…`}</button>
          </div>
        )}

        {/* Properties */}
        <div>
          <span className="text-xs text-[var(--text-tertiary)] font-medium block mb-1.5">{t('panel.properties')}</span>
          {Object.keys(displayProps).length === 0 ? (
            <span className="text-xs text-[var(--text-muted)] italic">—</span>
          ) : (
            <div className="space-y-0.5 bg-[var(--bg-tertiary)] rounded-xl p-2.5">
              {Object.entries(displayProps).map(([k, v]) => (
                <div key={k} className="flex justify-between items-center py-1 px-1.5 rounded-lg hover:bg-[var(--bg-hover)] transition-all">
                  <span className="text-xs text-[var(--text-tertiary)] font-medium mr-3">{k}</span>
                  <span className="text-xs text-[var(--text-primary)] text-right truncate max-w-[160px]">{String(v)}</span>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Edge source/target */}
        {type === 'edge' && (
          <>
            <div className="flex items-center gap-2">
              <span className="text-xs text-[var(--text-tertiary)] font-medium w-12">{t('panel.strength')}</span>
              <span className="text-sm text-[var(--text-primary)] font-mono">{item.strength ?? 1.0}</span>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-xs text-[var(--text-tertiary)] font-medium w-12 uppercase">{t('panel.source')}</span>
              <span className="text-sm text-[var(--text-primary)] font-mono">{item.source}</span>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-xs text-[var(--text-tertiary)] font-medium w-12 uppercase">{t('panel.target')}</span>
              <span className="text-sm text-[var(--text-primary)] font-mono">{item.target}</span>
            </div>
          </>
        )}
      </div>

      {/* Doc Viewer Modal */}
      {showDoc && <DocViewer docId={showDoc} onClose={() => setShowDoc(null)} />}
    </div>
  );
}
