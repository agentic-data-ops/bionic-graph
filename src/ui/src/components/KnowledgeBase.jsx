import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import {
  listDocuments, addDocument, deleteDocument, getDocumentContent,
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
  "entities": [{ "name": "EntityName", "type": "person|place|organization|concept|event|object", "description": "Brief description" }],
  "relations": [{ "source": "EntityName", "target": "EntityName", "relation": "relationship description" }]
}
- Extract 5-20 most important entities. Entity names should be in their original language. Relations should use clear, concise descriptions.`;
  const { response } = chatCompletion(
    [{ role: 'system', content: systemPrompt }, { role: 'user', content: `Document: ${sourceFile}\n\n${content}` }],
    provider,
  );
  let result = '';
  await parseSSEStream(await response, (t) => { result += t; });
  try { const jsonMatch = result.match(/\{[\s\S]*\}/); if (jsonMatch) return JSON.parse(jsonMatch[0]); } catch {}
  return { entities: [], relations: [] };
}

/** Progress step component */
function ProgressStep({ label, status, detail }) {
  const icon = status === 'done' ? '✅' : status === 'running' ? '⏳' : status === 'failed' ? '❌' : '⏸';
  const color = status === 'done' ? 'text-[#30d158]' : status === 'running' ? 'text-[#0a84ff]' : status === 'failed' ? 'text-[#ff453a]' : 'text-[#636366]';
  return (
    <div className="py-1">
      <div className="flex items-center gap-2">
        <span className="text-xs">{icon}</span>
        <span className={`text-xs ${color} font-medium`}>{label}</span>
        {status === 'running' && <span className="inline-flex gap-0.5"><span className="w-1 h-1 rounded-full bg-[#0a84ff] pulse-dot" /><span className="w-1 h-1 rounded-full bg-[#0a84ff] pulse-dot" style={{ animationDelay: '0.2s' }} /><span className="w-1 h-1 rounded-full bg-[#0a84ff] pulse-dot" style={{ animationDelay: '0.4s' }} /></span>}
      </div>
      {detail && status === 'done' && <div className="mt-1 ml-5 text-[11px] text-[#636366] leading-relaxed font-mono whitespace-pre-wrap border-l border-[#2a2a2e] pl-3">{detail}</div>}
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
  const [importContent, setImportContent] = useState('');
  const [importGraph, setImportGraph] = useState(defaultGraph);
  const [importProvider, setImportProvider] = useState(activeProvider);
  const [importSteps, setImportSteps] = useState([]);
  const [importing, setImporting] = useState(false);
  const [showEdit, setShowEdit] = useState(null);
  const [editContent, setEditContent] = useState('');

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
  const filteredDocs = filterTag ? documents.filter((d) => (d.tags || []).includes(filterTag)) : documents;

  const runExtraction = useCallback(async (content, providerCfg, graphName) => {
    const steps = [];
    const addStep = (s) => { steps.push(s); setImportSteps([...steps]); };

    addStep({ label: 'Generating title...', status: 'running', detail: '' });
    const title = await generateTitle(providerCfg, content);
    addStep({ label: `Title: ${title}`, status: 'done', detail: title });

    addStep({ label: 'Generating tags...', status: 'running', detail: '' });
    const tags = await generateTags(providerCfg, content);
    addStep({ label: `Tags: ${tags.join(', ')}`, status: 'done', detail: tags.join(', ')});

    addStep({ label: 'Adding document...', status: 'running', detail: '' });
    const doc = await addDocument(title, content, tags);
    addStep({ label: `Document saved`, status: 'done', detail: doc.id });

    addStep({ label: 'Extracting entities and relations...', status: 'running', detail: '' });
    const extracted = await extractFromMarkdown(providerCfg, content, title);
    addStep({ label: `Extracted ${extracted.entities?.length || 0} entities, ${extracted.relations?.length || 0} relations`, status: 'done', detail: '' });

    addStep({ label: 'Creating vertices...', status: 'running', detail: '' });
    const vertexMap = {};
    let vCount = 0;
    for (const entity of (extracted.entities || [])) {
      try {
        const v = await addVertex([entity.type || 'entity'], { name: entity.name, description: entity.description || '', source_file: title, chapter_path: '' }, graphName);
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
    addStep({ label: '✅ Import complete', status: 'done', detail: '' });

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
    try { const content = await getDocumentContent(doc.id); setEditContent(content); setShowEdit(doc.id); } catch {}
  }, []);

  const handleSaveEdit = useCallback(async () => {
    if (!showEdit || !editContent.trim() || !provider) return;
    const doc = documents.find((d) => d.id === showEdit);
    if (!doc) return;
    setImporting(true);
    setImportSteps([]);
    try {
      const res = await graphSearch([doc.title], importGraph);
      const vertexIds = (res?.data || []).filter((item) => item.type === 'vertex' && item.properties?.source_file === doc.title).map((item) => item.id);
      for (const vid of vertexIds) { try { await deleteVertex(vid, importGraph); } catch {} }
      await runExtraction(editContent, provider, importGraph);
      setShowEdit(null); setEditContent('');
    } catch (e) {
      setImportSteps((prev) => [...prev, { label: `❌ Error: ${e.message}`, status: 'failed', detail: '' }]);
    }
    setImporting(false);
  }, [showEdit, editContent, provider, importGraph, documents, runExtraction]);

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
      <div className="flex gap-1.5 mb-4 flex-wrap">
        <button className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${!filterTag ? 'bg-[#0a84ff] text-white' : 'bg-[#2a2a2e] text-[#86868b] hover:text-white'}`} onClick={() => setFilterTag('')}>All</button>
        {allTags.map((tag) => (<button key={tag} className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${filterTag === tag ? 'bg-[#0a84ff] text-white' : 'bg-[#2a2a2e] text-[#86868b] hover:text-white'}`} onClick={() => setFilterTag(tag)}>{tag}</button>))}
      </div>

      {/* Import button */}
      <div className="mb-4">
        <button className="px-3.5 py-2 rounded-xl bg-[#0a84ff] text-white text-sm font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={() => setShowImport(true)} disabled={!provider || importing}>
          + {t('knowledgeBase.import')}
        </button>
      </div>

      {/* Import dialog */}
      {showImport && (
        <div className="mb-4 p-4 bg-[#2a2a2e] rounded-xl space-y-3">
          {/* Graph selector */}
          <div className="flex gap-3 items-center">
            <select className="flex-1 px-3 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] appearance-none cursor-pointer" value={importGraph} onChange={(e) => setImportGraph(e.target.value)} style={{ backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'%3E%3Cpath fill='%23636366' d='M0 0l5 6 5-6z'/%3E%3C/svg%3E\")", backgroundRepeat: 'no-repeat', backgroundPosition: 'right 12px center', paddingRight: '32px' }}>
              {graphs.map((g) => <option key={g} value={g}>{g}</option>)}
            </select>
            <select className="flex-1 px-3 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] appearance-none cursor-pointer" value={importProvider} onChange={(e) => setImportProvider(e.target.value)} style={{ backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'%3E%3Cpath fill='%23636366' d='M0 0l5 6 5-6z'/%3E%3C/svg%3E\")", backgroundRepeat: 'no-repeat', backgroundPosition: 'right 12px center', paddingRight: '32px' }}>
              {providers.map((p) => <option key={p.id} value={p.id}>{p.name} ({p.model})</option>)}
            </select>
          </div>

          {/* Text area */}
          <textarea className="w-full h-28 px-3 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] placeholder-[#48484a] resize-none" placeholder="Paste markdown content or drag a .md file..." value={importContent} onChange={(e) => setImportContent(e.target.value)} />

          {/* Action buttons */}
          <div className="flex gap-2 justify-between">
            <label className="px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium cursor-pointer transition-all">
              📄 Upload .md
              <input type="file" accept=".md,.markdown,.txt" className="hidden" onChange={handleFileUpload} disabled={!provider || importing} />
            </label>
            <div className="flex gap-2">
              <button className="px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all" onClick={() => { setShowImport(false); setImportContent(''); setImportSteps([]); }}>Cancel</button>
              <button className="px-3.5 py-1.5 rounded-xl bg-[#0a84ff] text-white text-xs font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={handleImportText} disabled={!importContent.trim() || !provider || importing}>
                {importing ? 'Importing...' : 'Import'}
              </button>
            </div>
          </div>

          {/* Progress steps */}
          {importSteps.length > 0 && (
            <div className="bg-[#1c1c20] rounded-xl p-3 mt-2">
              {importSteps.map((step, i) => <ProgressStep key={i} {...step} />)}
            </div>
          )}
        </div>
      )}

      {/* Edit dialog */}
      {showEdit && (
        <div className="mb-4 p-4 bg-[#2a2a2e] rounded-xl space-y-3">
          <textarea className="w-full h-32 px-3 py-2 rounded-xl bg-[#1c1c20] border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] text-[#e5e5e7] text-sm placeholder-[#48484a] resize-none" value={editContent} onChange={(e) => setEditContent(e.target.value)} />
          <div className="flex gap-2 justify-end">
            <button className="px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all" onClick={() => { setShowEdit(null); setEditContent(''); }}>Cancel</button>
            <button className="px-3.5 py-1.5 rounded-xl bg-[#0a84ff] text-white text-xs font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={handleSaveEdit} disabled={!editContent.trim() || !provider || importing}>
              {importing ? 'Re-extracting...' : 'Save & Re-extract'}
            </button>
          </div>
          {importSteps.length > 0 && (
            <div className="bg-[#1c1c20] rounded-xl p-3 mt-2">
              {importSteps.map((step, i) => <ProgressStep key={i} {...step} />)}
            </div>
          )}
        </div>
      )}

      {loading && <div className="text-center text-[#636366] text-sm py-8">Loading...</div>}

      {!loading && (
        <div className="space-y-1 max-h-60 overflow-y-auto">
          {filteredDocs.length === 0 && <div className="text-center text-[#636366] text-sm py-8">No documents yet</div>}
          {filteredDocs.map((doc) => (
            <div key={doc.id} className="flex items-center justify-between py-2.5 px-3 rounded-xl hover:bg-[#2a2a2e] transition-all group">
              <div className="flex-1 min-w-0">
                <div className="text-sm text-[#e5e5e7] font-medium truncate">{doc.title}</div>
                <div className="text-xs text-[#636366] mt-0.5">
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
      )}
    </Modal>
  );
}
