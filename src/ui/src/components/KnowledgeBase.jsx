import { useState, useEffect, useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import {
  listDocuments, addDocument, updateDocument, deleteDocument, getDocumentContent,
  listGraphs,
  startDocumentExtraction, getExtractionTask, fetchModels,
} from '../api';

function Modal({ title, children, onClose }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
      <div className="relative bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-6 min-w-[640px] max-w-2xl max-h-[85vh] overflow-y-auto shadow-2xl" onClick={(e) => e.stopPropagation()}>
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

// Extraction now uses backend POST /documents/:id/extract — no frontend LLM calls needed.

/** Model selector that fetches model list from backend MaaS proxy */
function ModelSelector({ value, onChange }) {
  const { t } = useTranslation();
  const [models, setModels] = useState(null);
  const [defaultModel, setDefaultModel] = useState('');
  const [modelOpen, setModelOpen] = useState(false);
  const initialised = useRef(false);

  useEffect(() => {
    if (!models) {
      fetchModels().then(({ models: m, defaultModel: dm }) => {
        const list = m?.data || [];
        setModels(list);
        setDefaultModel(dm || '');
        if (!initialised.current) {
          initialised.current = true;
          if (!value) {
            onChange(dm || list[0]?.id || '');
          }
        }
      }).catch(() => {});
    }
  }, []);

  if (!models) {
    return <div className="text-xs text-[var(--text-tertiary)]">{t('graph.loading')}</div>;
  }

  const options = models.map(entry => ({
    key: entry.id,
    isDefault: entry.id === defaultModel,
  }));

  return (
    <div className="relative">
      <button
        className="w-full px-3 py-2 rounded-xl bg-transparent text-[var(--text-primary)] text-sm border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] transition-all font-medium flex items-center gap-1 text-left"
        onClick={(e) => { e.stopPropagation(); setModelOpen(!modelOpen); }}
      >
        <span className="flex-1 truncate">{value || t('chat.selectModel')}</span>
        <svg className={`w-3 h-3 flex-shrink-0 transition-transform ${modelOpen ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}><path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" /></svg>
      </button>
      {modelOpen && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setModelOpen(false)} />
          <div className="absolute left-0 top-full mt-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden w-full max-h-[300px] overflow-y-auto">
            {options.map((opt) => (
              <button
                key={opt.key}
                className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap truncate transition-all ${opt.key === value ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                onClick={() => { onChange(opt.key); setModelOpen(false); }}
              >
                <span>{opt.key}</span>
                {opt.isDefault && <span className="ml-1.5 text-[var(--text-muted)]">({t('chat.default')})</span>}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

/** Progress step component — styled to match SearchStep from MessageList */
function ProgressStep({ label, status, detail }) {
  const icon = status === 'done' || status === 'completed' ? '✅'
    : status === 'running' ? '⏳'
    : status === 'failed' ? '❌'
    : '⏸';
  const color = status === 'done' || status === 'completed' ? 'text-[var(--success)]'
    : status === 'running' ? 'text-[var(--accent)]'
    : status === 'failed' ? 'text-[var(--danger)]'
    : 'text-[var(--text-tertiary)]';
  return (
    <div className="py-1.5">
      <div className="flex items-center gap-2">
        <span className="text-xs">{icon}</span>
        <span className={`text-xs ${color} font-medium tracking-tight`}>{label}</span>
      </div>
      {detail && (status === 'done' || status === 'completed') && (
        <div className="mt-1.5 ml-5 text-[11px] text-[var(--text-tertiary)] leading-relaxed font-mono whitespace-pre-wrap border-l border-[var(--border)] pl-3">{detail}</div>
      )}
      {detail && status === 'running' && (
        <div className="mt-1.5 ml-5 text-[11px] text-[var(--text-muted)] leading-relaxed font-mono whitespace-pre-wrap border-l border-[var(--border)] pl-3 max-h-20 overflow-y-auto">{detail}</div>
      )}
    </div>
  );
}

export default function KnowledgeBase({ open, onClose, providers, activeProvider, defaultGraph, theme, initialContent, initialGraph }) {
  const { t } = useTranslation();
  const [documents, setDocuments] = useState([]);
  const [graphs, setGraphs] = useState([]);
  const [filterTag, setFilterTag] = useState('');
  const [loading, setLoading] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [filterGraph, setFilterGraph] = useState('');
  const [importContent, setImportContent] = useState('');
  const [importTitle, setImportTitle] = useState('');
  const [importGraph, setImportGraph] = useState(defaultGraph);
  const [importProvider, setImportProvider] = useState(activeProvider);
  const [importModel, setImportModel] = useState(null);
  const [importModelKey, setImportModelKey] = useState('');
  const [importSteps, setImportSteps] = useState([]);
  const [importing, setImporting] = useState(false);
  const [importGraphOpen, setImportGraphOpen] = useState(false);
  const [showEdit, setShowEdit] = useState(null);
  const [editContent, setEditContent] = useState('');
  const [editTitle, setEditTitle] = useState('');
  const [editTags, setEditTags] = useState([]);
  const [editNewTag, setEditNewTag] = useState('');
  const [deleteConfirm, setDeleteConfirm] = useState(null);
  const [deleteGraphData, setDeleteGraphData] = useState(true);
  const [docView, setDocView] = useState(null);
  const [docViewContent, setDocViewContent] = useState('');

  const provider = (() => {
    const p = providers.find((p) => p.id === importProvider);
    if (!p) return p;
    if (importModel) {
      return { ...p, model: importModel };
    }
    return p;
  })();

  useEffect(() => {
    if (open) {
      setLoading(true);
      if (initialContent) {
        setImportContent(initialContent);
        setShowImport(true);
      }
      setImportTitle('');
      setImportSteps([]);
      if (initialGraph) setImportGraph(initialGraph);
      Promise.all([
        listDocuments().then((d) => setDocuments(Array.isArray(d) ? d : (d.documents || []))).catch(() => {}),
        listGraphs().then((d) => setGraphs(d.graphs || [])).catch(() => {}),
      ]).then(() => setLoading(false));
    }
  }, [open]);

  const allTags = [...new Set(documents.flatMap((d) => d.tags || []))];
  const allGraphs = [...new Set(documents.map((d) => d.graph_name).filter(Boolean))];
  const filteredDocs = documents.filter((d) => {
    if (filterTag && !(d.tags || []).includes(filterTag)) return false;
    if (filterGraph && d.graph_name !== filterGraph) return false;
    return true;
  });

  const runExtraction = useCallback(async (content, title, graphName, modelKey) => {
    const steps = [];
    const addStep = (s) => {
      const last = steps[steps.length - 1];
      if (last && last.status === 'running') {
        steps[steps.length - 1] = s;
      } else {
        steps.push(s);
      }
      setImportSteps([...steps]);
    };

    addStep({ label: 'Adding document...', status: 'running', detail: '' });
    let docId;
    try {
      const doc = await addDocument(title, content, []);
      docId = doc.id;
      addStep({ label: `Document saved: ${title}`, status: 'done', detail: docId });
    } catch (e) {
      addStep({ label: 'Failed to save document', status: 'failed', detail: e.message });
      setImporting(false);
      return;
    }

    addStep({ label: 'Starting extraction...', status: 'running', detail: '' });
    let taskId;
    try {
      const task = await startDocumentExtraction(docId, graphName, modelKey);
      taskId = task.task_id;
      addStep({ label: 'Extraction task submitted', status: 'running', detail: taskId });
    } catch (e) {
      addStep({ label: 'Failed to start extraction', status: 'failed', detail: e.message });
      setImporting(false);
      return;
    }

    // Poll task until completion
    addStep({ label: 'Extracting entities and relations...', status: 'running', detail: '' });
    const poll = async () => {
      for (let i = 0; i < 120; i++) {
        await new Promise(r => setTimeout(r, 3000));
        const task = await getExtractionTask(taskId);
        const runStep = task.steps?.find(s => s.status === 'running');
        if (runStep) {
          addStep({ label: runStep.label, status: 'running', detail: runStep.detail || '' });
        }
        if (task.status === 'completed') {
          const stats = task.stats || {};
          addStep({ label: `Extraction complete: ${stats.new_vertices || 0} vertices, ${stats.new_edges || 0} edges`, status: 'done', detail: '' });
          addStep({ label: 'Import complete', status: 'done', detail: '' });
          const docs = await listDocuments();
          setDocuments(Array.isArray(docs) ? docs : (docs.documents || []));
          setImporting(false);
          return;
        }
        if (task.status === 'failed') {
          addStep({ label: 'Extraction failed', status: 'failed', detail: task.error || 'Unknown error' });
          setImporting(false);
          return;
        }
      }
      addStep({ label: 'Extraction timed out', status: 'failed', detail: '' });
      setImporting(false);
    };
    poll();
  }, []);

  const handleImportText = useCallback(async () => {
    if (!importContent.trim() || !importTitle.trim()) return;
    setImporting(true);
    setImportSteps([]);
    try {
      await runExtraction(importContent, importTitle.trim(), importGraph, importModelKey);
      setImportContent('');
      setImportTitle('');
    } catch (e) {
      setImportSteps((prev) => [...prev, { label: `❌ Error: ${e.message}`, status: 'failed', detail: '' }]);
      setImporting(false);
    }
  }, [importContent, importTitle, importGraph, runExtraction]);

  const handleFileUpload = useCallback(async (e) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const content = await file.text();
    const fileName = file.name.replace(/\.(md|markdown|txt)$/i, '');
    setImportContent(content);
    setImportTitle(fileName);
    setShowImport(true);
    e.target.value = '';
  }, []);

  const handleEdit = useCallback(async (doc) => {
    setEditTitle(doc.title);
    setEditTags(doc.tags || []);
    setEditNewTag('');
    setShowEdit(doc.id);
  }, []);

  const handleSaveEdit = useCallback(async () => {
    if (!showEdit || !editTitle.trim()) return;
    try {
      await updateDocument(showEdit, editTitle, editTags);
      const docs = await listDocuments();
      setDocuments(Array.isArray(docs) ? docs : (docs.documents || []));
      setShowEdit(null);
      setEditTitle('');
      setEditTags([]);
    } catch (e) {
      console.error('Save error:', e);
    }
  }, [showEdit, editTitle, editTags]);

  const handleDelete = useCallback(async () => {
    const doc = deleteConfirm;
    if (!doc) return;
    setDeleteConfirm(null);
    try {
      await deleteDocument(doc.id, deleteGraphData);
      const docs = await listDocuments(); setDocuments(Array.isArray(docs) ? docs : (docs.documents || []));
    } catch (e) { console.error('Delete error:', e); }
  }, [deleteConfirm, deleteGraphData]);

  if (!open) return null;

  return (
    <Modal title={t('knowledgeBase.title')} onClose={onClose}>
      {!provider && <div className="text-xs text-[#ff9f0a] mb-4 text-center">{t('chat.noProvider')}</div>}

      {/* Tag filter */}
      <div className="mb-1">
        <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.tagFilter')}</label>
      </div>
      <div className="flex gap-1.5 mb-4 flex-wrap">
        <button className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${!filterTag ? 'bg-[var(--accent)] text-white' : 'bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]'}`} onClick={() => setFilterTag('')}>{t('knowledgeBase.all')}</button>
        {allTags.map((tag) => (<button key={tag} className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${filterTag === tag ? 'bg-[var(--accent)] text-white' : 'bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]'}`} onClick={() => setFilterTag(tag)}>{tag}</button>))}
      </div>

      {/* Graph filter */}
      <div className="mb-1">
        <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.graphFilter')}</label>
      </div>
      <div className="flex gap-1.5 mb-4 flex-wrap">
        <button className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${!filterGraph ? 'bg-[var(--accent)] text-white' : 'bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]'}`} onClick={() => setFilterGraph('')}>{t('knowledgeBase.all')}</button>
        {allGraphs.map((g) => (<button key={g} className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${filterGraph === g ? 'bg-[var(--accent)] text-white' : 'bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]'}`} onClick={() => setFilterGraph(g)}>{g}</button>))}
      </div>

      {/* Import dialog - separate modal */}
      {showImport && (
        <Modal title={t('knowledgeBase.import')} onClose={() => { setShowImport(false); setImportContent(''); setImportSteps([]); }}>
          <div className="space-y-4">
          <div>
            <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.graph')}</label>
            <div className="relative">
              <button
                className="w-full px-3 py-2 rounded-xl bg-transparent text-[var(--text-primary)] text-sm border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] transition-all font-medium flex items-center gap-1 text-left"
                onClick={(e) => { e.stopPropagation(); setImportGraphOpen(!importGraphOpen); }}
              >
                <span className="flex-1 truncate">{importGraph}</span>
                <svg className={`w-3 h-3 flex-shrink-0 transition-transform ${importGraphOpen ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}><path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" /></svg>
              </button>
              {importGraphOpen && (
                <>
                  <div className="fixed inset-0 z-40" onClick={() => setImportGraphOpen(false)} />
                  <div className="absolute left-0 top-full mt-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden w-full">
                    {graphs.filter(g => g.name).map((g) => (
                      <button
                        key={g.name}
                        className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap truncate transition-all ${g.name === importGraph ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                        onClick={() => { setImportGraph(g.name); setImportGraphOpen(false); }}
                      >{g.name}</button>
                    ))}
                  </div>
                </>
              )}
            </div>
          </div>
          <div>
            <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.model')}</label>
            <ModelSelector
              value={importModelKey}
              onChange={setImportModelKey}
            />
          </div>

          <div>
            <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.editTitle')}</label>
            <input className="w-full px-3.5 py-2 rounded-xl bg-[var(--bg-secondary)] text-[var(--text-primary)] text-sm border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] placeholder-[var(--text-muted)]"
              type="text" placeholder={t('knowledgeBase.docTitlePlaceholder')} value={importTitle} onChange={(e) => setImportTitle(e.target.value)} />
          </div>
          <div>
            <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.docContent')}</label>
            <textarea className="w-full h-28 px-3 py-2 rounded-xl bg-[var(--bg-secondary)] text-[var(--text-primary)] text-sm border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] placeholder-[var(--text-muted)] resize-none" placeholder={t('knowledgeBase.import') + '...'} value={importContent} onChange={(e) => setImportContent(e.target.value)} />
          </div>

          {/* Action buttons */}
          <div className="flex gap-2 justify-between">
            <label className="px-3.5 py-1.5 rounded-xl bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium cursor-pointer transition-all">
              {'📄'} {t('knowledgeBase.upload')}
              <input type="file" accept=".md,.markdown,.txt" className="hidden" onChange={handleFileUpload} disabled={!provider || importing} />
            </label>
            <div className="flex gap-2">
              <button className="px-3.5 py-1.5 rounded-xl bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium transition-all" onClick={() => { setShowImport(false); setImportContent(''); setImportSteps([]); }}>{t('panel.close')}</button>
              <button className="px-3.5 py-1.5 rounded-xl bg-[var(--accent)] text-white text-xs font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm" onClick={handleImportText} disabled={!importContent.trim() || !importTitle.trim() || importing}>
                {importing ? t('knowledgeBase.import') + '...' : t('knowledgeBase.import')}
              </button>
            </div>
          </div>

          {/* Progress steps */}
          {importSteps.length > 0 && (
            <div className="border border-[var(--border)] rounded-xl overflow-hidden mt-2">
              <div className="px-4 py-3">
                <div className="text-xs text-[var(--accent)] font-semibold mb-2 tracking-tight">📄 {t('knowledgeBase.import')}</div>
                <div className="space-y-0">
                  {importSteps.map((step, i) => <ProgressStep key={i} {...step} />)}
                </div>
              </div>
            </div>
          )}
          </div>
        </Modal>
      )}

      {/* Edit dialog - modal popup */}
      {showEdit && (
        <Modal title={t('knowledgeBase.editTitle')} onClose={() => { setShowEdit(null); setEditTitle(''); setEditTags([]); setEditNewTag(''); }}>
          <div className="space-y-4">
            <div>
              <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.editTitle')}</label>
              <input className="w-full px-3.5 py-2 rounded-xl bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-sm border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] placeholder-[var(--text-muted)]"
                type="text" value={editTitle} onChange={(e) => setEditTitle(e.target.value)} />
            </div>
            <div>
              <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.editTags')}</label>
              <div className="flex flex-wrap gap-1.5 mb-2">
                {editTags.map((tag, idx) => (
                  <span key={idx} className="inline-flex items-center gap-1 px-2 py-0.5 rounded-lg bg-[var(--bg-hover)] text-xs text-[var(--text-primary)]">
                    {tag}
                    <button className="text-[var(--danger)] hover:text-[#ff6961] text-[10px] font-medium" onClick={() => setEditTags(editTags.filter((_, i) => i !== idx))}>&times;</button>
                  </span>
                ))}
              </div>
              <div className="flex gap-2">
                <input className="flex-1 px-3 py-1.5 rounded-xl bg-[var(--bg-tertiary)] text-[var(--text-primary)] text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] placeholder-[var(--text-muted)]"
                  type="text" placeholder={t('knowledgeBase.addTag')} value={editNewTag}
                  onChange={(e) => setEditNewTag(e.target.value)}
                  onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); if (editNewTag.trim() && !editTags.includes(editNewTag.trim())) { setEditTags([...editTags, editNewTag.trim()]); setEditNewTag(''); } } }} />
                <button className="px-3 py-1.5 rounded-xl bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-xs font-medium transition-all"
                  onClick={() => { if (editNewTag.trim() && !editTags.includes(editNewTag.trim())) { setEditTags([...editTags, editNewTag.trim()]); setEditNewTag(''); } }}>{t('knowledgeBase.addTag')}</button>
              </div>
            </div>
            <div className="flex gap-2 justify-end">
              <button className="px-4 py-2 rounded-xl bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-sm font-medium transition-all" onClick={() => { setShowEdit(null); setEditTitle(''); setEditTags([]); setEditNewTag(''); }}>{t('panel.close')}</button>
              <button className="px-4 py-2 rounded-xl bg-[var(--accent)] text-white text-sm font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm" onClick={handleSaveEdit} disabled={!editTitle.trim()}>
                {t('settings.save')}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {/* Document detail dialog */}
      {docView && (
        <Modal title={docView.title} onClose={() => setDocView(null)}>
          <div className="space-y-4 max-h-[70vh] overflow-y-auto">
            {docView.tags?.length > 0 && (
              <div className="flex flex-wrap gap-1.5">
                {docView.tags.map((tag, i) => (
                  <span key={i} className="px-2 py-0.5 rounded-md bg-[var(--accent)]/15 text-[var(--accent)] text-xs font-medium">{tag}</span>
                ))}
              </div>
            )}
            <div className="text-xs text-[var(--text-primary)] font-mono whitespace-pre-wrap border border-[var(--border)] rounded-xl p-4 bg-[var(--bg-tertiary)] leading-relaxed max-h-96 overflow-y-auto">{docViewContent || t('graph.loading')}</div>
          </div>
        </Modal>
      )}

      {/* Delete confirmation dialog */}
      {deleteConfirm && (
        <Modal title={t('knowledgeBase.deleteDocTitle')} onClose={() => setDeleteConfirm(null)}>
          <div className="space-y-4">
            <p className="text-sm text-[var(--text-primary)]">{t('knowledgeBase.confirmDeleteDoc', { title: deleteConfirm.title })}</p>
            <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer select-none">
              <input type="checkbox" checked={deleteGraphData} onChange={(e) => setDeleteGraphData(e.target.checked)}
                className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-secondary)] checked:bg-[var(--accent)] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
              {t('knowledgeBase.cleanGraphData')}
            </label>
            <div className="flex gap-2 justify-end">
              <button className="px-4 py-2 rounded-xl bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] text-sm font-medium transition-all" onClick={() => setDeleteConfirm(null)}>{t('graph.cancel')}</button>
              <button className="px-4 py-2 rounded-xl bg-[var(--danger)] text-white text-sm font-medium hover:bg-[color-mix(in srgb, var(--danger), black 10%)] transition-all shadow-sm" onClick={handleDelete}>
                {t('graph.delete')}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {loading && <div className="text-center text-[var(--text-tertiary)] text-sm py-8">{t('graph.loading')}</div>}

      {!loading && (
        <>
        <div className="mb-1">
          <label className="block text-xs text-[var(--text-tertiary)] font-medium mb-2 tracking-tight">{t('knowledgeBase.docList')}</label>
        </div>
        <div className="space-y-1 max-h-60 overflow-y-auto">
          {filteredDocs.length === 0 && <div className="text-center text-[var(--text-tertiary)] text-sm py-8">{t('knowledgeBase.noDocs')}</div>}
          {filteredDocs.map((doc) => (
            <div key={doc.id} className="flex items-center justify-between py-2.5 px-3 rounded-xl hover:bg-[var(--bg-tertiary)] transition-all group">
              <div className="flex-1 min-w-0 cursor-pointer" onClick={async () => {
                setDocView(doc);
                try { const c = await getDocumentContent(doc.id); setDocViewContent(c); } catch { setDocViewContent(''); }
              }}>
                <div className="text-sm text-[var(--text-primary)] font-medium truncate hover:text-[var(--accent)] transition-colors">{doc.title}</div>
                <div className="text-xs text-[var(--text-tertiary)] mt-0.5">
                  {doc.graph_name && <><span className="text-[var(--accent)]">{doc.graph_name}</span>{' · '}</>}
                  {doc.tags?.length > 0 && doc.tags.join(', ') + ' · '}
                  {new Date(doc.updated_at || doc.created_at).toLocaleDateString()}
                </div>
              </div>
              <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 ml-2">
                <button className="px-2 py-1 text-xs text-[var(--text-tertiary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] rounded-lg transition-all" onClick={() => handleEdit(doc)} disabled={!!importing}>{t('panel.edit')}</button>
                <button className="px-2 py-1 text-xs text-[var(--danger)] hover:bg-[color-mix(in srgb, var(--bg-hover), var(--danger) 30%)] rounded-lg transition-all" onClick={() => { setDeleteConfirm(doc); setDeleteGraphData(false); }} disabled={!!importing}>{t('settings.delete')}</button>
              </div>
            </div>
          ))}
        </div>
        <div className="mt-4">
          <button className="w-full py-2.5 rounded-xl bg-[var(--accent)] text-white text-sm font-medium hover:bg-[color-mix(in srgb, var(--accent), black 10%)] transition-all shadow-sm" onClick={() => setShowImport(true)} disabled={!provider || importing}>
            + {t('knowledgeBase.import')}
          </button>
        </div>
        </>
      )}
    </Modal>
  );
}
