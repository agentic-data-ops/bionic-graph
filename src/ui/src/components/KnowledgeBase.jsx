import { useState, useEffect, useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import {
  listDocuments, addDocument, deleteDocument, getDocumentContent,
  addVertex, addEdge, deleteVertex, graphSearch,
  chatCompletion, parseSSEStream,
} from '../api';

function Modal({ title, children, onClose }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center" onClick={onClose}>
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" />
      <div className="relative bg-[#1c1c20] border border-[#2a2a2e] rounded-2xl p-6 min-w-[600px] max-w-2xl max-h-[85vh] overflow-y-auto shadow-2xl" onClick={(e) => e.stopPropagation()}>
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

/**
 * Generate a document title from content via LLM.
 * ≤30 chars, no punctuation, original language, English uses hyphens.
 */
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

  const { response } = chatCompletion(
    [{ role: 'user', content: prompt }],
    provider,
  );
  let result = '';
  await parseSSEStream(await response, (t) => { result += t; });
  return result.trim().slice(0, 30).replace(/[。，！？、；：""''（）【】《》.,!?;:'"()\[\]{}<>\/\\@#$%^&*+=~\s]/g, '').slice(0, 30);
}

/**
 * Generate tags from document content via LLM.
 */
async function generateTags(provider, content) {
  const prompt = `Extract 2-4 topic tags from this markdown document. Tags should be single words or short phrases that categorize the document's content.

Rules:
- Tags must be in the same language as the document
- Each tag should be 1-3 words
- Return ONLY a JSON array of strings, no other text

Document content (first 500 chars):
${content.slice(0, 500)}`;

  const { response } = chatCompletion(
    [{ role: 'user', content: prompt }],
    provider,
  );
  let result = '';
  await parseSSEStream(await response, (t) => { result += t; });
  try {
    const match = result.match(/\[[\s\S]*\]/);
    if (match) return JSON.parse(match[0]);
  } catch {}
  return [];
}

/**
 * Extract entities and relations from markdown via LLM.
 * Returns { entities: [{name, type, description}], relations: [{source, target, relation}] }
 */
async function extractFromMarkdown(provider, content, sourceFile) {
  const systemPrompt = `You are a knowledge graph extractor. Extract entities and their relationships from the given markdown document.

Return ONLY valid JSON with this structure:
{
  "entities": [
    { "name": "EntityName", "type": "person|place|organization|concept|event|object", "description": "Brief description" }
  ],
  "relations": [
    { "source": "EntityName", "target": "EntityName", "relation": "relationship description" }
  ]
}

- Extract 5-20 most important entities
- Entity names should be in their original language
- Relations should use clear, concise descriptions`;

  const { response } = chatCompletion(
    [{ role: 'system', content: systemPrompt }, { role: 'user', content: `Document: ${sourceFile}\n\n${content}` }],
    provider,
  );
  let result = '';
  await parseSSEStream(await response, (t) => { result += t; });

  try {
    // Find JSON in the response
    const jsonMatch = result.match(/\{[\s\S]*\}/);
    if (jsonMatch) return JSON.parse(jsonMatch[0]);
  } catch {}
  return { entities: [], relations: [] };
}

export default function KnowledgeBase({ open, onClose, providers, activeProvider, defaultGraph, theme }) {
  const { t } = useTranslation();

  const [documents, setDocuments] = useState([]);
  const [filterTag, setFilterTag] = useState('');
  const [loading, setLoading] = useState(false);
  const [extracting, setExtracting] = useState(null); // doc id being extracted
  const [showAdd, setShowAdd] = useState(false);
  const [pasteContent, setPasteContent] = useState('');
  const [showEdit, setShowEdit] = useState(null); // doc id being edited
  const [editContent, setEditContent] = useState('');

  const provider = providers.find((p) => p.id === activeProvider);

  // Load documents
  useEffect(() => {
    if (open) {
      setLoading(true);
      listDocuments().then((d) => {
        setDocuments(d.documents || []);
        setLoading(false);
      }).catch(() => setLoading(false));
    }
  }, [open]);

  // All unique tags
  const allTags = [...new Set(documents.flatMap((d) => d.tags || []))];
  const filteredDocs = filterTag
    ? documents.filter((d) => (d.tags || []).includes(filterTag))
    : documents;

  // ── Add document ──
  const handleAddText = useCallback(async () => {
    if (!pasteContent.trim() || !provider) return;
    const newDocId = 'doc-' + Date.now();
    setExtracting(newDocId);

    try {
      const title = await generateTitle(provider, pasteContent);
      const tags = await generateTags(provider, pasteContent);
      const addResult = await addDocument(title, pasteContent, tags);
      const doc = addResult || { id: newDocId, title, tags };

      // Extract entities/relations
      const extracted = await extractFromMarkdown(provider, pasteContent, doc.title || title);

      // Create vertices
      const vertexMap = {};
      for (const entity of (extracted.entities || [])) {
        try {
          const v = await addVertex(
            [entity.type || 'entity'],
            {
              name: entity.name,
              description: entity.description || '',
              source_file: doc.title || title,
              chapter_path: '',
            },
            defaultGraph,
          );
          vertexMap[entity.name] = v.id;
        } catch {}
      }

      // Create edges
      for (const rel of (extracted.relations || [])) {
        const src = vertexMap[rel.source];
        const tgt = vertexMap[rel.target];
        if (src && tgt) {
          try {
            await addEdge(rel.relation, src, tgt, {}, defaultGraph);
          } catch {}
        }
      }

      // Refresh list
      const docs = await listDocuments();
      setDocuments(docs.documents || []);
      setPasteContent('');
      setShowAdd(false);
    } catch (e) {
      console.error('Extraction error:', e);
    }
    setExtracting(null);
  }, [pasteContent, provider, defaultGraph]);

  // ── Upload file ──
  const handleFileUpload = useCallback(async (e) => {
    const file = e.target.files?.[0];
    if (!file || !provider) return;
    const content = await file.text();
    const newDocId = 'doc-' + Date.now();
    setExtracting(newDocId);

    try {
      const title = await generateTitle(provider, content);
      const tags = await generateTags(provider, content);
      const addResult = await addDocument(title, content, tags);
      const doc = addResult || { id: newDocId, title, tags };

      const extracted = await extractFromMarkdown(provider, content, doc.title || title);
      const vertexMap = {};
      for (const entity of (extracted.entities || [])) {
        try {
          const v = await addVertex(
            [entity.type || 'entity'],
            { name: entity.name, description: entity.description || '', source_file: doc.title || title, chapter_path: '' },
            defaultGraph,
          );
          vertexMap[entity.name] = v.id;
        } catch {}
      }
      for (const rel of (extracted.relations || [])) {
        const src = vertexMap[rel.source];
        const tgt = vertexMap[rel.target];
        if (src && tgt) {
          try { await addEdge(rel.relation, src, tgt, {}, defaultGraph); } catch {}
        }
      }

      const docs = await listDocuments();
      setDocuments(docs.documents || []);
    } catch (e) {
      console.error('Extraction error:', e);
    }
    setExtracting(null);
    e.target.value = '';
  }, [provider, defaultGraph]);

  // ── Edit document ──
  const handleEdit = useCallback(async (doc) => {
    try {
      const content = await getDocumentContent(doc.id);
      setEditContent(content);
      setShowEdit(doc.id);
    } catch {}
  }, []);

  const handleSaveEdit = useCallback(async () => {
    if (!showEdit || !editContent.trim() || !provider) return;
    const doc = documents.find((d) => d.id === showEdit);
    if (!doc) return;

    setExtracting(showEdit);
    try {
      // Delete all vertices with this source_file
      const res = await graphSearch([doc.title], defaultGraph);
      const vertexIds = (res?.data || [])
        .filter((item) => item.type === 'vertex' && item.properties?.source_file === doc.title)
        .map((item) => item.id);

      for (const vid of vertexIds) {
        try { await deleteVertex(vid, defaultGraph); } catch {}
      }

      // Re-generate title and tags
      const title = await generateTitle(provider, editContent);
      const tags = await generateTags(provider, editContent);

      // Re-extract
      const extracted = await extractFromMarkdown(provider, editContent, title);
      const vertexMap = {};
      for (const entity of (extracted.entities || [])) {
        try {
          const v = await addVertex(
            [entity.type || 'entity'],
            { name: entity.name, description: entity.description || '', source_file: title, chapter_path: '' },
            defaultGraph,
          );
          vertexMap[entity.name] = v.id;
        } catch {}
      }
      for (const rel of (extracted.relations || [])) {
        const src = vertexMap[rel.source];
        const tgt = vertexMap[rel.target];
        if (src && tgt) {
          try { await addEdge(rel.relation, src, tgt, {}, defaultGraph); } catch {}
        }
      }

      // Update document
      await addDocument(title, editContent, tags);

      const docs = await listDocuments();
      setDocuments(docs.documents || []);
      setShowEdit(null);
      setEditContent('');
    } catch (e) {
      console.error('Edit error:', e);
    }
    setExtracting(null);
  }, [showEdit, editContent, provider, defaultGraph, documents]);

  // ── Delete document ──
  const handleDelete = useCallback(async (doc) => {
    if (!confirm(`Delete "${doc.title}"?`)) return;
    try {
      // Delete related vertices
      const res = await graphSearch([doc.title], defaultGraph);
      const vertexIds = (res?.data || [])
        .filter((item) => item.type === 'vertex' && item.properties?.source_file === doc.title)
        .map((item) => item.id);
      for (const vid of vertexIds) {
        try { await deleteVertex(vid, defaultGraph); } catch {}
      }
      // Delete document
      await deleteDocument(doc.id);
      const docs = await listDocuments();
      setDocuments(docs.documents || []);
    } catch (e) {
      console.error('Delete error:', e);
    }
  }, [defaultGraph]);

  if (!open) return null;

  return (
    <Modal title={t('knowledgeBase.title')} onClose={onClose}>
      {/* Provider warning */}
      {!provider && (
        <div className="text-xs text-[#ff9f0a] mb-4 text-center">{t('chat.noProvider')}</div>
      )}

      {/* Tag filter */}
      <div className="flex gap-1.5 mb-4 flex-wrap">
        <button className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${!filterTag ? 'bg-[#0a84ff] text-white' : 'bg-[#2a2a2e] text-[#86868b] hover:text-white'}`} onClick={() => setFilterTag('')}>All</button>
        {allTags.map((tag) => (
          <button key={tag} className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-all ${filterTag === tag ? 'bg-[#0a84ff] text-white' : 'bg-[#2a2a2e] text-[#86868b] hover:text-white'}`} onClick={() => setFilterTag(tag)}>{tag}</button>
        ))}
      </div>

      {/* Add button */}
      <div className="flex gap-2 mb-4">
        <button className="px-3.5 py-2 rounded-xl bg-[#0a84ff] text-white text-sm font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={() => setShowAdd(true)} disabled={!provider || !!extracting}>
          + {t('knowledgeBase.add')}
        </button>
        <label className="px-3.5 py-2 rounded-xl bg-[#2a2a2e] text-[#86868b] hover:text-white text-sm font-medium cursor-pointer transition-all">
          📄 {t('knowledgeBase.upload')}
          <input type="file" accept=".md,.markdown,.txt" className="hidden" onChange={handleFileUpload} disabled={!provider || !!extracting} />
        </label>
      </div>

      {/* Add text dialog */}
      {showAdd && (
        <div className="mb-4 p-4 bg-[#2a2a2e] rounded-xl space-y-3">
          <textarea className="w-full h-32 px-3 py-2 rounded-xl bg-[#1c1c20] border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] text-[#e5e5e7] text-sm placeholder-[#48484a] resize-none" placeholder="Paste markdown content..." value={pasteContent} onChange={(e) => setPasteContent(e.target.value)} />
          <div className="flex gap-2 justify-end">
            <button className="px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all" onClick={() => { setShowAdd(false); setPasteContent(''); }}>Cancel</button>
            <button className="px-3.5 py-1.5 rounded-xl bg-[#0a84ff] text-white text-xs font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={handleAddText} disabled={!pasteContent.trim() || !!extracting}>
              {extracting ? 'Extracting...' : 'Add & Extract'}
            </button>
          </div>
        </div>
      )}

      {/* Edit dialog */}
      {showEdit && (
        <div className="mb-4 p-4 bg-[#2a2a2e] rounded-xl space-y-3">
          <textarea className="w-full h-32 px-3 py-2 rounded-xl bg-[#1c1c20] border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] text-[#e5e5e7] text-sm placeholder-[#48484a] resize-none" value={editContent} onChange={(e) => setEditContent(e.target.value)} />
          <div className="flex gap-2 justify-end">
            <button className="px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all" onClick={() => { setShowEdit(null); setEditContent(''); }}>Cancel</button>
            <button className="px-3.5 py-1.5 rounded-xl bg-[#0a84ff] text-white text-xs font-medium hover:bg-[#0a6ed9] transition-all shadow-sm" onClick={handleSaveEdit} disabled={!editContent.trim() || !!extracting}>
              {extracting ? 'Re-extracting...' : 'Save & Re-extract'}
            </button>
          </div>
        </div>
      )}

      {/* Loading */}
      {loading && <div className="text-center text-[#636366] text-sm py-8">Loading...</div>}

      {/* Document list */}
      {!loading && (
        <div className="space-y-1 max-h-60 overflow-y-auto">
          {filteredDocs.length === 0 && (
            <div className="text-center text-[#636366] text-sm py-8">No documents yet</div>
          )}
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
                <button className="px-2 py-1 text-xs text-[#636366] hover:text-white hover:bg-[#3a3a3e] rounded-lg transition-all" onClick={() => handleEdit(doc)} disabled={!!extracting}>Edit</button>
                <button className="px-2 py-1 text-xs text-[#ff453a] hover:bg-[#3a2a2e] rounded-lg transition-all" onClick={() => handleDelete(doc)} disabled={!!extracting}>Delete</button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Extracting indicator */}
      {extracting && (
        <div className="mt-3 text-xs text-[#0a84ff] font-medium text-center flex items-center justify-center gap-2">
          <span className="w-2 h-2 rounded-full bg-[#0a84ff] animate-pulse" />
          Extracting knowledge graph from document...
        </div>
      )}
    </Modal>
  );
}
