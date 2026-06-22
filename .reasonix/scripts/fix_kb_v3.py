with open('src/ui/src/components/KnowledgeBase.jsx', 'r') as f:
    c = f.read()

# Update addDocument to pass graphName
c = c.replace(
    "const doc = await addDocument(title, content, tags);",
    "const doc = await addDocument(title, content, tags, graphName);"
)

# Remove docGraphs localStorage
c = c.replace(
    "const [filterGraph, setFilterGraph] = useState('');\n  const [docGraphs, setDocGraphs] = useState(() => {\n    try { return JSON.parse(localStorage.getItem('bgraph-doc-graphs') || '{}'); } catch { return {}; }\n  });",
    "const [filterGraph, setFilterGraph] = useState('');"
)

c = c.replace(
    "  useEffect(() => { localStorage.setItem('bgraph-doc-graphs', JSON.stringify(docGraphs)); }, [docGraphs]);\n\n",
    ""
)

# allGraphs from backend
c = c.replace(
    "const allGraphs = [...new Set(Object.values(docGraphs))];",
    "const allGraphs = [...new Set(documents.map((d) => d.graph_name).filter(Boolean))];"
)

c = c.replace(
    "if (filterGraph && docGraphs[d.id] !== filterGraph) return false;",
    "if (filterGraph && d.graph_name !== filterGraph) return false;"
)

# Update doc info line
c = c.replace(
    'docGraphs[doc.id]',
    'doc.graph_name'
)

# Add tag filter label
c = c.replace(
    "{/* Tag filter */}\n      <div className=\"flex gap-1.5 mb-4 flex-wrap\">",
    "{/* Tag filter */}\n      <div className=\"mb-1\">\n        <label className=\"block text-xs text-[#636366] font-medium mb-1.5 tracking-tight\">{t('knowledgeBase.tagFilter')}</label>\n      </div>\n      <div className=\"flex gap-1.5 mb-4 flex-wrap\">"
)

# Add graph filter label
c = c.replace(
    "{/* Graph filter */}\n      <div className=\"flex gap-1.5 mb-4 flex-wrap\">",
    "{/* Graph filter */}\n      <div className=\"mb-1\">\n        <label className=\"block text-xs text-[#636366] font-medium mb-1.5 tracking-tight\">{t('knowledgeBase.graphFilter')}</label>\n      </div>\n      <div className=\"flex gap-1.5 mb-4 flex-wrap\">"
)

# Add editTitle/editTags states
c = c.replace(
    "const [editContent, setEditContent] = useState('');",
    "const [editContent, setEditContent] = useState('');\n  const [editTitle, setEditTitle] = useState('');\n  const [editTags, setEditTags] = useState([]);"
)

# Update handleEdit
c = c.replace(
    "const handleEdit = useCallback(async (doc) => {\n    try {\n      const content = await getDocumentContent(doc.id);\n      setEditContent(content);\n      setShowEdit(doc.id);\n    } catch {}\n  }, []);",
    "const handleEdit = useCallback(async (doc) => {\n    try {\n      setEditTitle(doc.title);\n      setEditTags(doc.tags || []);\n      setShowEdit(doc.id);\n    } catch {}\n  }, []);"
)

# Update handleSaveEdit
old_save = "const handleSaveEdit = useCallback(async () => {\n    if (!showEdit || !editContent.trim() || !provider) return;\n    const doc = documents.find((d) => d.id === showEdit);\n    if (!doc) return;\n    setShowEdit(null);\n    await runBackendExtraction(editContent, provider, importGraph);\n  }, [showEdit, editContent, provider, importGraph, documents, runBackendExtraction]);"
new_save = "const handleSaveEdit = useCallback(async () => {\n    if (!showEdit || !editTitle.trim() || !provider) return;\n    const doc = documents.find((d) => d.id === showEdit);\n    if (!doc) return;\n    await updateDocument(showEdit, editTitle, editTags);\n    const docs = await listDocuments();\n    setDocuments(docs.documents || []);\n    setShowEdit(null);\n  }, [showEdit, editTitle, editTags, provider]);"

c = c.replace(old_save, new_save)

# Update edit dialog
old_edit = "{/* Edit dialog */}\n      {showEdit && (\n        <div className=\"mb-4 p-4 bg-[#2a2a2e] rounded-xl space-y-3\">\n          <textarea className=\"w-full h-32 px-3 py-2 rounded-xl bg-[#1c1c20] border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] text-[#e5e5e7] text-sm placeholder-[#48484a] resize-none\" value={editContent} onChange={(e) => setEditContent(e.target.value)} />\n          <div className=\"flex gap-2 justify-end\">\n            <button className=\"px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all\" onClick={() => { setShowEdit(null); setEditContent(''); }}>Cancel</button>\n            <button className=\"px-3.5 py-1.5 rounded-xl bg-[#0a84ff] text-white text-xs font-medium hover:bg-[#0a6ed9] transition-all shadow-sm\" onClick={handleSaveEdit} disabled={!editContent.trim() || !provider || importing}>\n              {importing ? 'Re-extracting...' : 'Save & Re-extract'}\n            </button>\n          </div>\n          {importSteps.length > 0 && (\n            <div className=\"bg-[#1c1c20] rounded-xl p-3 mt-2\">\n              {importSteps.map((step, i) => <ProgressStep key={i} {...step} />)}\n            </div>\n          )}\n        </div>\n      )}"

new_edit = "{/* Edit dialog */}\n      {showEdit && (\n        <div className=\"mb-4 p-4 bg-[#2a2a2e] rounded-xl space-y-3\">\n          <div>\n            <label className=\"block text-xs text-[#636366] font-medium mb-1.5 tracking-tight\">Title</label>\n            <input className=\"w-full px-3.5 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] placeholder-[#48484a]\"\n              type=\"text\" value={editTitle} onChange={(e) => setEditTitle(e.target.value)} />\n          </div>\n          <div>\n            <label className=\"block text-xs text-[#636366] font-medium mb-1.5 tracking-tight\">Tags</label>\n            <input className=\"w-full px-3.5 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] placeholder-[#48484a]\"\n              type=\"text\" value={editTags.join(', ')} onChange={(e) => setEditTags(e.target.value.split(',').map((s) => s.trim()).filter(Boolean))} />\n          </div>\n          <div className=\"flex gap-2 justify-end\">\n            <button className=\"px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all\" onClick={() => { setShowEdit(null); setEditTitle(''); setEditTags([]); }}>{t('panel.close')}</button>\n            <button className=\"px-3.5 py-1.5 rounded-xl bg-[#0a84ff] text-white text-xs font-medium hover:bg-[#0a6ed9] transition-all shadow-sm\" onClick={handleSaveEdit} disabled={!editTitle.trim() || !provider || importing}>\n              {importing ? 'Saving...' : t('settings.save')}\n            </button>\n          </div>\n          {importSteps.length > 0 && (\n            <div className=\"bg-[#1c1c20] rounded-xl p-3 mt-2\">\n              {importSteps.map((step, i) => <ProgressStep key={i} {...step} />)}\n            </div>\n          )}\n        </div>\n      )}"

c = c.replace(old_edit, new_edit)

# Also add import for updateDocument and listDocuments if missing
# Check if updateDocument is imported
if 'updateDocument' not in c:
    c = c.replace(
        'listDocuments, addDocument, deleteDocument, getDocumentContent,',
        'listDocuments, addDocument, updateDocument, deleteDocument, getDocumentContent,'
    )

with open('src/ui/src/components/KnowledgeBase.jsx', 'w') as f:
    f.write(c)

print("Done")
