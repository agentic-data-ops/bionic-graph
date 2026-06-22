with open('src/ui/src/components/KnowledgeBase.jsx', 'r') as f:
    c = f.read()

# 1. Add label before document list
c = c.replace(
    "{!loading && (\n        <div className=\"space-y-1 max-h-60 overflow-y-auto\">",
    "{!loading && (\n        <div className=\"mb-1\">\n          <label className=\"block text-xs text-[#636366] font-medium mb-2 tracking-tight\">{t('knowledgeBase.docList')}</label>\n        </div>\n        <div className=\"space-y-1 max-h-60 overflow-y-auto\">"
)

# 2. Add i18n keys
import json
for lang, vals in [('zh', {'docList':'文档列表','editTitle':'标题','editTags':'标签','addTag':'添加标签'}), ('en', {'docList':'Documents','editTitle':'Title','editTags':'Tags','addTag':'Add tag'})]:
    with open(f'src/ui/src/locales/{lang}.json') as f:
        d = json.load(f)
    for k, v in vals.items():
        d[f'knowledgeBase.{k}'] = v
    with open(f'src/ui/src/locales/{lang}.json', 'w') as f:
        json.dump(d, f, ensure_ascii=False, indent=2)
print("i18n keys added")

# 3. Update handleEdit to set editTitle/editTags
old_handle_edit = "const handleEdit = useCallback(async (doc) => {\n    try { const content = await getDocumentContent(doc.id); setEditContent(content); setShowEdit(doc.id); } catch {}\n  }, []);"
new_handle_edit = "const handleEdit = useCallback(async (doc) => {\n    setEditTitle(doc.title);\n    setEditTags(doc.tags || []);\n    setEditNewTag('');\n    setShowEdit(doc.id);\n  }, []);"
c = c.replace(old_handle_edit, new_handle_edit)

# 4. Add editNewTag state
c = c.replace(
    "const [editTitle, setEditTitle] = useState('');\n  const [editTags, setEditTags] = useState([]);",
    "const [editTitle, setEditTitle] = useState('');\n  const [editTags, setEditTags] = useState([]);\n  const [editNewTag, setEditNewTag] = useState('');"
)

# 5. Update handleSaveEdit to use updateDocument
old_save = "const handleSaveEdit = useCallback(async () => {\n    if (!showEdit || !editContent.trim() || !provider) return;\n    const doc = documents.find((d) => d.id === showEdit);\n    if (!doc) return;\n    setImporting(true);\n    setImportSteps([]);\n    try {\n      const res = await graphSearch([doc.title], importGraph);\n      const vertexIds = (res?.data || []).filter((item) => item.type === 'vertex' && item.properties?.source_file === doc.title).map((item) => item.id);\n      for (const vid of vertexIds) { try { await deleteVertex(vid, importGraph); } catch {} }\n      await runExtraction(editContent, provider, importGraph);\n      setShowEdit(null); setEditContent('');\n    } catch (e) {\n      setImportSteps((prev) => [...prev, { label: `❌ Error: ${e.message}`, status: 'failed', detail: '' }]);\n    }\n    setImporting(false);\n  }, [showEdit, editContent, provider, importGraph, documents, runExtraction]);"
new_save = "const handleSaveEdit = useCallback(async () => {\n    if (!showEdit || !editTitle.trim()) return;\n    try {\n      await updateDocument(showEdit, editTitle, editTags);\n      const docs = await listDocuments();\n      setDocuments(docs.documents || []);\n      setShowEdit(null);\n      setEditTitle('');\n      setEditTags([]);\n    } catch (e) {\n      console.error('Save error:', e);\n    }\n  }, [showEdit, editTitle, editTags]);"
c = c.replace(old_save, new_save)

# 6. Replace edit dialog with Modal popup + tag list
old_edit_dialog = """      {/* Edit dialog */}
      {showEdit && (
        <div className=\"mb-4 p-4 bg-[#2a2a2e] rounded-xl space-y-3\">
          <div>
            <label className=\"block text-xs text-[#636366] font-medium mb-1.5 tracking-tight\">Title</label>
            <input className=\"w-full px-3.5 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] placeholder-[#48484a]\"
              type=\"text\" value={editTitle} onChange={(e) => setEditTitle(e.target.value)} />
          </div>
          <div>
            <label className=\"block text-xs text-[#636366] font-medium mb-1.5 tracking-tight\">Tags</label>
            <input className=\"w-full px-3.5 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] placeholder-[#48484a]\"
              type=\"text\" value={editTags.join(', ')} onChange={(e) => setEditTags(e.target.value.split(',').map((s) => s.trim()).filter(Boolean))} />
          </div>
          <div className=\"flex gap-2 justify-end\">
            <button className=\"px-3.5 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all\" onClick={() => { setShowEdit(null); setEditTitle(''); setEditTags([]); }}>{t('panel.close')}</button>
            <button className=\"px-3.5 py-1.5 rounded-xl bg-[#0a84ff] text-white text-xs font-medium hover:bg-[#0a6ed9] transition-all shadow-sm\" onClick={handleSaveEdit} disabled={!editTitle.trim() || !provider || importing}>
              {importing ? 'Saving...' : t('settings.save')}
            </button>
          </div>
          {importSteps.length > 0 && (
            <div className=\"bg-[#1c1c20] rounded-xl p-3 mt-2\">
              {importSteps.map((step, i) => <ProgressStep key={i} {...step} />)}
            </div>
          )}
        </div>
      )}"""

new_edit_dialog = """      {/* Edit dialog - modal popup */}
      {showEdit && (
        <Modal title={t('knowledgeBase.editTitle')} onClose={() => { setShowEdit(null); setEditTitle(''); setEditTags([]); setEditNewTag(''); }}>
          <div className=\"space-y-4\">
            <div>
              <label className=\"block text-xs text-[#636366] font-medium mb-1.5 tracking-tight\">{t('knowledgeBase.editTitle')}</label>
              <input className=\"w-full px-3.5 py-2 rounded-xl bg-[#2a2a2e] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] placeholder-[#48484a]\"
                type=\"text\" value={editTitle} onChange={(e) => setEditTitle(e.target.value)} />
            </div>
            <div>
              <label className=\"block text-xs text-[#636366] font-medium mb-1.5 tracking-tight\">{t('knowledgeBase.editTags')}</label>
              <div className=\"flex flex-wrap gap-1.5 mb-2\">
                {editTags.map((tag, idx) => (
                  <span key={idx} className=\"inline-flex items-center gap-1 px-2 py-0.5 rounded-lg bg-[#3a3a3e] text-xs text-[#e5e5e7]\">
                    {tag}
                    <button className=\"text-[#ff453a] hover:text-[#ff6961] text-[10px] font-medium\" onClick={() => setEditTags(editTags.filter((_, i) => i !== idx))}>&times;</button>
                  </span>
                ))}
              </div>
              <div className=\"flex gap-2\">
                <input className=\"flex-1 px-3 py-1.5 rounded-xl bg-[#2a2a2e] text-[#e5e5e7] text-xs border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] placeholder-[#48484a]\"
                  type=\"text\" placeholder={t('knowledgeBase.addTag')} value={editNewTag}
                  onChange={(e) => setEditNewTag(e.target.value)}
                  onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); if (editNewTag.trim() && !editTags.includes(editNewTag.trim())) { setEditTags([...editTags, editNewTag.trim()]); setEditNewTag(''); } } }} />
                <button className=\"px-3 py-1.5 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-xs font-medium transition-all\"
                  onClick={() => { if (editNewTag.trim() && !editTags.includes(editNewTag.trim())) { setEditTags([...editTags, editNewTag.trim()]); setEditNewTag(''); } }}>{t('knowledgeBase.addTag')}</button>
              </div>
            </div>
            <div className=\"flex gap-2 justify-end\">
              <button className=\"px-4 py-2 rounded-xl bg-[#3a3a3e] text-[#86868b] hover:text-white text-sm font-medium transition-all\" onClick={() => { setShowEdit(null); setEditTitle(''); setEditTags([]); setEditNewTag(''); }}>{t('panel.close')}</button>
              <button className=\"px-4 py-2 rounded-xl bg-[#0a84ff] text-white text-sm font-medium hover:bg-[#0a6ed9] transition-all shadow-sm\" onClick={handleSaveEdit} disabled={!editTitle.trim()}>
                {t('settings.save')}
              </button>
            </div>
          </div>
        </Modal>
      )}"""

if old_edit_dialog in c:
    c = c.replace(old_edit_dialog, new_edit_dialog)
    print('Edit dialog replaced')
else:
    print('Edit dialog NOT found')
    idx = c.find('{/* Edit dialog */}')
    if idx >= 0:
        print(c[idx:idx+100])

with open('src/ui/src/components/KnowledgeBase.jsx', 'w') as f:
    f.write(c)
print('Done')
