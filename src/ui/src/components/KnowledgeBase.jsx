import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import {
  listDocuments, addDocument, updateDocument, deleteDocument, getDocumentContent,
  addVertex, addEdge, deleteVertex, graphSearch, listGraphs,
  chatCompletion, parseSSEStream,
} from '../api';

function Modal({ title, children, onClose }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center" onClick={onClose}>
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
      <div className="relative bg-[#1c1c20] border border-[#2a2a2e] rounded-2xl p-6 min-w-[640px] max-w-2xl max-h-[85vh] overflow-y-auto shadow-2xl" onClick={(e) => e.stopPropagation()}>
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

async function generateTitle(provider, content) {
  const prompt = `Generate a concise title (max 30 characters) for this markdown document. Rules:
- If the document already has a heading (# Title), simplify it to ≤30 chars without punctuation
- Otherwise create a new title from the content
- Keep the original language (Chinese stays Chinese, English stays English)
- English words should be joined with hyphens
- NO punctuation, NO spaces in English titles
- Return ONLY the title, nothing else

Document content (first 500 chars):
${content.slice(0, 500)}`;
  const { response } = chatCompletion([{ role: 'user', content: prompt }], provider);
  let result = '';
  await parseSSEStream(await response, (t) => { result += t; });
  return result.trim().slice(0, 30).replace(/[。，！？、；：""''（）【】《》.,!?;:'"()\[\]{}<>\/\\@#$%^&*+=~\s]/g, '').slice(0, 30);
}

async function generateTags(provider, content) {
  const prompt = `Extract 2-4 topic tags from this markdown document. Tags should be single words or short phrases that categorize the document's content.
Rules: Tags must be in the same language as the document. Each tag should be 1-3 words. Return ONLY a JSON array of strings, no other text.
Document content (first 500 chars):
${content.slice(0, 500)}`;
  const { response } = chatCompletion([{ role: 'user', content: prompt }], provider);
  let result = '';
  await parseSSEStream(await response, (t) => { result += t; });
  try { const match = result.match(/\[[\s\S]*\]/); if (match) return JSON.parse(match[0]); } catch {}
  return [];
}

async function extractFromMarkdown(provider, content, sourceFile) {
  const systemPrompt = `You are a knowledge graph extractor. Extract entities and their relationships from the given markdown document.
Return ONLY valid JSON with this structure:
{
  "entities": [{ "name": "EntityName", "type": "person|place|organization|concept|event|object", "description": "Brief description", "keywords": ["search keyword1", "search keyword2"] }],
  "relations": [{ "source": "EntityName", "target": "EntityName", "relation": "relationship description" }]
}
- Extract 5-20 most important entities.
- For each entity, provide 0-5 search keywords that help find this entity. Do NOT include the entity name or type in keywords — they are already used as search terms automatically. Only provide ADDITIONAL keywords.
- Entity names should be in their original language.
- Relations should use clear, concise descriptions.`;
  const { response } = chatCompletion(
    [{ role: 'system', content: systemPrompt }, { role: 'user', content: `Document: ${sourceFile}\n\n${content}` }],
    provider,
  );
  let result = '';
  await parseSSEStream(await response, (t) => { result += t; });
  try { const jsonMatch = result.match(/\{[\s\S]*\}/); if (jsonMatch) return JSON.parse(jsonMatch[0]); } catch {}
  return { entities: [], relations: [] };
}

/** Progress step component — styled to match SearchStep from MessageList */
function ProgressStep({ label, status, detail }) {
  const icon = status === 'done' || status === 'completed' ? '✅'
    : status === 'running' ? '⏳'
    : status === 'failed' ? '❌'
    : '⏸';
  const color = status === 'done' || status === 'completed' ? 'text-[#30d158]'
    : status === 'running' ? 'text-[#0a84ff]'
    : status === 'failed' ? 'text-[#ff453a]'
    : 'text-[#636366]';
  return (
    <div className="py-1.5">
      <div className="flex items-center gap-2">
        <span className="text-xs">{icon}</span>
        <span className={`text-xs ${color} font-medium tracking-tight`}>{label}</span>
      </div>
      {detail && (status === 'done' || status === 'completed') && (
        <div className="mt-1.5 ml-5 text-[11px] text-[#636366] leading-relaxed font-mono whitespace-pre-wrap border-l border-[#2a2a2e] pl-3">{detail}</div>
      )}
      {detail && status === 'running' && (
        <div className="mt-1.5 ml-5 text-[11px] text-[#48484a] leading-relaxed font-mono whitespace-pre-wrap border-l border-[#2a2a2e] pl-3 max-h-20 overflow-y-auto">{detail}</div>
      )}
    </div>
  );
}

export default function KnowledgeBase({ open, onClose, providers, activeProvider, defaultGraph, theme }) {
  const { t } = useTranslation();
  const [documents, setDocuments] = useState([]);
  const [graphs, setGraphs] = useState([]);
  const [filterTag, setFilterTag] = useState('');
  const [loading, setLoading] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [filterGraph, setFilterGraph] = useState('');
  const [importContent, setImportContent] = useState('');
  const [importGraph, setImportGraph] = useState(defaultGraph);
  const [importProvider, setImportProvider] = useState(activeProvider);
  const [importSteps, setImportSteps] = useState([]);
  const [importing, setImporting] = useState(false);
  const [showEdit, setShowEdit] = useState(null);
  const [editContent, setEditContent] = useState('');
  const [editTitle, setEditTitle] = useState('');
  const [editTags, setEditTags] = useState([]);
  const [editNewTag, setEditNewTag] = useState('');

  const provider = providers.find((p) => p.id === importProvider);

  useEffect(() => {
    if (open) {
      setLoading(true);
      Promise.all([
        listDocuments().then((d) => setDocuments(d.documents || [])).catch(() => {}),
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

  const runExtraction = useCallback(async (content, providerCfg, graphName) => {
    const steps = [];
    const addStep = (s) => {
      // Replace last running step instead of appending duplicate
      const last = steps[steps.length - 1];
      if (last && last.status === 'running') {
        steps[steps.length - 1] = s;
      } else {
        steps.push(s);
      }
      setImportSteps([...steps]);
    };

    addStep({ label: 'Generating title...', status: 'running', detail: '' });
    const title = await generateTitle(providerCfg, content);
    addStep({ label: `Title: ${title}`, status: 'done', detail: title });

    addStep({ label: 'Generating tags...', status: 'running', detail: '' });
    const tags = await generateTags(providerCfg, content);
    addStep({ label: `Tags: ${tags.join(', ')}`, status: 'done', detail: tags.join(', ')});

    addStep({ label: 'Adding document...', status: 'running', detail: '' });
    const doc = await addDocument(title, content, tags, graphName);
    addStep({ label: `Document saved`, status: 'done', detail: doc.id });

    addStep({ label: 'Extracting entities and relations...', status: 'running', detail: '' });
    const extracted = await extractFromMarkdown(providerCfg, content, title);
    addStep({ label: `Extracted ${extracted.entities?.length || 0} entities, ${extracted.relations?.length || 0} relations`, status: 'done', detail: '' });

    addStep({ label: 'Creating vertices...', status: 'running', detail: '' });
    const vertexMap = {};
    let vCount = 0;
    for (const entity of (extracted.entities || [])) {
      try {
        const kw = entity.keywords || [entity.name];
        const v = await addVertex([entity.type || 'entity'], { description: entity.description || '', source_file: title, chapter_path: '' }, graphName, entity.name, kw);
        vertexMap[entity.name] = v.id;
        vCount++;
      } catch {}
    }
    addStep({ label: `${vCount} vertices created`, status: 'done', detail: '' });

    addStep({ label: 'Creating edges...', status: 'running', detail: '' });
    let eCount = 0;
    for (const rel of (extracted.relations || [])) {
      const src = vertexMap[rel.source];
      const tgt = vertexMap[rel.target];
      if (src && tgt) {
        try { await addEdge(rel.relation, src, tgt, {}, graphName); eCount++; } catch {}
      }
    }
    addStep({ label: `${eCount} edges created`, status: 'done', detail: '' });
    addStep({ label: 'Import complete', status: 'done', detail: '' });

    const docs = await listDocuments();
    setDocuments(docs.documents || []);
  }, []);

  const handleImportText = useCallback(async () => {
    if (!importContent.trim() || !provider) return;
    setImporting(true);
    setImportSteps([]);
    try {
      await runExtraction(importContent, provider, importGraph);
      setImportContent('');
    } catch (e) {
      setImportSteps((prev) => [...prev, { label: `❌ Error: ${e.message}`, status: 'failed', detail: '' }]);
    }
    setImporting(false);
  }, [importContent, provider, importGraph, runExtraction]);

  const handleFileUpload = useCallback(async (e) => {
    const file = e.target.files?.[0];
    if (!file || !provider) return;
    const content = await file.text();
    setImporting(true);
    setImportSteps([]);
    try {
      await runExtraction(content, provider, importGraph);
    } catch (err) {
      setImportSteps((prev) => [...prev, { label: `❌ Error: ${err.message}`, status: 'failed', detail: '' }]);
    }
    setImporting(false);
    e.target.value = '';
  }, [provider, importGraph, runExtraction]);

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
      setDocuments(docs.documents || []);
      setShowEdit(null);
      setEditTitle('');
      setEditTags([]);
    } catch (e) {
      console.error('Save error:', e);
    }
  }, [showEdit, editTitle, editTags]);

  const handleDelete = useCallback(async (doc) => {
    if (!confirm(`Delete "${doc.title}"?`)) return;
    try {
      const res = await graphSearch([doc.title], importGraph);
      const vertexIds = (res?.data || []).filter((item) => item.type === 'vertex' && item.properties?.source_file === doc.title).map((item) => item.id);
      for (const vid of vertexIds) { try { await deleteVertex(vid, importGraph); } catch {} }
      await deleteDocument(doc.id);
      const docs = await listDocuments(); setDocuments(docs.documents || []);
    } catch (e) { console.error('Delete error:', e); }
  }, [importGraph]);

  if (!open) return null;

  return (
    <Modal title={t('knowledgeBase.title')} onClose={onClose}>
      {!provider && <div className="text-xs text-[#ff9f0a] mb-4 text-center">{t('chat.noProvider')}</div>}

      {/* Tag filter */}
      <div className="mb-1">
        <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.tagFilter')}</label>
      </div>
      <div className="flex gap-1.5 mb-4 flex-wrap">
        <button className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${!filterTag ? 'bg-[#0a84ff] text-white' : 'bg-[#2a2a2e] text-[#86868b] hover:text-white'}`} onClick={() => setFilterTag('')}>All</button>
        {allTags.map((tag) => (<button key={tag} className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${filterTag === tag ? 'bg-[#0a84ff] text-white' : 'bg-[#2a2a2e] text-[#86868b] hover:text-white'}`} onClick={() => setFilterTag(tag)}>{tag}</button>))}
      </div>

      {/* Graph filter */}
      <div className="mb-1">
        <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.graphFilter')}</label>
      </div>
      <div className="flex gap-1.5 mb-4 flex-wrap">
        <button className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${!filterGraph ? 'bg-[#0a84ff] text-white' : 'bg-[#2a2a2e] text-[#86868b] hover:text-white'}`} onClick={() => setFilterGraph('')}>All</button>
        {allGraphs.map((g) => (<button key={g} className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${filterGraph === g ? 'bg-[#0a84ff] text-white' : 'bg-[#2a2a2e] text-[#86868b] hover:text-white'}`} onClick={() => setFilterGraph(g)}>{g}</button>))}
      </div>

      {/* Import dialog - separate modal */}
      {showImport && (
        <Modal title={t('knowledgeBase.import')} onClose={() => { setShowImport(false); setImportContent(''); setImportSteps([]); }}>
          <div className="space-y-4">
          <div>
            <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.graph')}</label>
            <select className="w-full px-3 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] appearance-none cursor-pointer"
              style={{ backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'%3E%3Cpath fill='%23636366' d='M0 0l5 6 5-6z'/%3E%3C/svg%3E\")", backgroundRepeat: 'no-repeat', backgroundPosition: 'right 12px center', paddingRight: '32px' }}
              value={importGraph} onChange={(e) => setImportGraph(e.target.value)}>
              {graphs.map((g) => <option key={g} value={g}>{g}</option>)}
            </select>
          </div>
          <div>
            <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.model')}</label>
            <select className="w-full px-3 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] appearance-none cursor-pointer"
              style={{ backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'%3E%3Cpath fill='%23636366' d='M0 0l5 6 5-6z'/%3E%3C/svg%3E\")", backgroundRepeat: 'no-repeat', backgroundPosition: 'right 12px center', paddingRight: '32px' }}
              value={importProvider} onChange={(e) => setImportProvider(e.target.value)}>
              {providers.flatMap((p) => {
                const models = p.models || [p.model];
                return models.map((m) => ({
                  key: p.id + '/' + m,
                  pid: p.id,
                  label: p.name + '/' + m,
                }));
              }).map((opt) => (
                <option key={opt.key} value={opt.pid}>{opt.label}</option>
              ))}
            </select>
          </div>

          <div>
            <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.content')}</label>
            <textarea className="w-full h-28 px-3 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] placeholder-[#48484a] resize-none" placeholder={t('knowledgeBase.import') + '...'} value={importContent} onChange={(e) => setImportContent(e.target.value)} />
          </div>

          {/* Action buttons */}
          <div className="flex gap-2 justify-between">
            <label className="px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium cursor-pointer transition-all">
              {'📄'} {t('knowledgeBase.upload')}
              <input type="file" accept=".md,.markdown,.txt" className="hidden" onChange={handleFileUpload} disabled={!provider || importing} />
            </label>
            <div className="flex gap-2">
              <button className="px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all" onClick={() => { setShowImport(false); setImportContent(''); setImportSteps([]); }}>{t('panel.close')}</button>
              <button className="px-3.5 py-1.5 rounded-xl bg-[#0a84ff] text-white text-xs font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={handleImportText} disabled={!importContent.trim() || !provider || importing}>
                {importing ? t('knowledgeBase.import') + '...' : t('knowledgeBase.import')}
              </button>
            </div>
          </div>

          {/* Progress steps */}
          {importSteps.length > 0 && (
            <div className="border border-[#2a2a2e] rounded-xl overflow-hidden mt-2">
              <div className="px-4 py-3">
                <div className="text-xs text-[#0a84ff] font-semibold mb-2 tracking-tight">📄 {t('knowledgeBase.import')}</div>
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
              <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.editTitle')}</label>
              <input className="w-full px-3.5 py-2 rounded-xl bg-[#2a2a2e] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] placeholder-[#48484a]"
                type="text" value={editTitle} onChange={(e) => setEditTitle(e.target.value)} />
            </div>
            <div>
              <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t('knowledgeBase.editTags')}</label>
              <div className="flex flex-wrap gap-1.5 mb-2">
                {editTags.map((tag, idx) => (
                  <span key={idx} className="inline-flex items-center gap-1 px-2 py-0.5 rounded-lg bg-[#3a3a3e] text-xs text-[#e5e5e7]">
                    {tag}
                    <button className="text-[#ff453a] hover:text-[#ff6961] text-[10px] font-medium" onClick={() => setEditTags(editTags.filter((_, i) => i !== idx))}>&times;</button>
                  </span>
                ))}
              </div>
              <div className="flex gap-2">
                <input className="flex-1 px-3 py-1.5 rounded-xl bg-[#2a2a2e] text-[#e5e5e7] text-xs border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] placeholder-[#48484a]"
                  type="text" placeholder={t('knowledgeBase.addTag')} value={editNewTag}
                  onChange={(e) => setEditNewTag(e.target.value)}
                  onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); if (editNewTag.trim() && !editTags.includes(editNewTag.trim())) { setEditTags([...editTags, editNewTag.trim()]); setEditNewTag(''); } } }} />
                <button className="px-3 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all"
                  onClick={() => { if (editNewTag.trim() && !editTags.includes(editNewTag.trim())) { setEditTags([...editTags, editNewTag.trim()]); setEditNewTag(''); } }}>{t('knowledgeBase.addTag')}</button>
              </div>
            </div>
            <div className="flex gap-2 justify-end">
              <button className="px-4 py-2 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-sm font-medium transition-all" onClick={() => { setShowEdit(null); setEditTitle(''); setEditTags([]); setEditNewTag(''); }}>{t('panel.close')}</button>
              <button className="px-4 py-2 rounded-xl bg-[#0a84ff] text-white text-sm font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={handleSaveEdit} disabled={!editTitle.trim()}>
                {t('settings.save')}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {loading && <div className="text-center text-[#636366] text-sm py-8">Loading...</div>}

      {!loading && (
        <>
        <div className="mb-1">
          <label className="block text-xs text-[#636366] font-medium mb-2 tracking-tight">{t('knowledgeBase.docList')}</label>
        </div>
        <div className="space-y-1 max-h-60 overflow-y-auto">
          {filteredDocs.length === 0 && <div className="text-center text-[#636366] text-sm py-8">No documents yet</div>}
          {filteredDocs.map((doc) => (
            <div key={doc.id} className="flex items-center justify-between py-2.5 px-3 rounded-xl hover:bg-[#2a2a2e] transition-all group">
              <div className="flex-1 min-w-0">
                <div className="text-sm text-[#e5e5e7] font-medium truncate">{doc.title}</div>
                <div className="text-xs text-[#636366] mt-0.5">
                  {doc.graph_name && <><span className="text-[#0a84ff]">{doc.graph_name}</span>{' · '}</>}
                  {doc.tags?.length > 0 && doc.tags.join(', ') + ' · '}
                  {new Date(doc.updated_at || doc.created_at).toLocaleDateString()}
                </div>
              </div>
              <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 ml-2">
                <button className="px-2 py-1 text-xs text-[#636366] hover:text-white hover:bg-[#3a3a3e] rounded-lg transition-all" onClick={() => handleEdit(doc)} disabled={!!importing}>Edit</button>
                <button className="px-2 py-1 text-xs text-[#ff453a] hover:bg-[#3a2a2e] rounded-lg transition-all" onClick={() => handleDelete(doc)} disabled={!!importing}>Delete</button>
              </div>
            </div>
          ))}
        </div>
        <div className="mt-4">
          <button className="w-full py-2.5 rounded-xl bg-[#0a84ff] text-white text-sm font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={() => setShowImport(true)} disabled={!provider || importing}>
            + {t('knowledgeBase.import')}
          </button>
        </div>
        </>
      )}
    </Modal>
  );
}
