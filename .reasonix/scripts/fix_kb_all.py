with open('src/ui/src/components/KnowledgeBase.jsx', 'r') as f:
    content = f.read()

# ===== Fix 1: Import fetchSettings =====
content = content.replace(
    'listDocuments, addDocument, deleteDocument, getDocumentContent,\n  addVertex, addEdge, deleteVertex, graphSearch, listGraphs,',
    'listDocuments, addDocument, deleteDocument, getDocumentContent,\n  addVertex, addEdge, deleteVertex, graphSearch, listGraphs,\n  startDocumentExtraction, getExtractionTask, fetchSettings,'
)

# ===== Fix 2: ProgressStep: support both 'done' and 'completed' =====
content = content.replace(
    "const icon = status === 'done' ? '✅' : status === 'running' ? '⏳' : status === 'failed' ? '❌' : '⏸';",
    "const icon = (status === 'done' || status === 'completed') ? '✅' : status === 'running' ? '⏳' : status === 'failed' ? '❌' : '⏸';"
)
content = content.replace(
    "const color = status === 'done' ? 'text-[#30d158]' : status === 'running' ? 'text-[#0a84ff]' : status === 'failed' ? 'text-[#ff453a]' : 'text-[#636366]';",
    "const color = (status === 'done' || status === 'completed') ? 'text-[#30d158]' : status === 'running' ? 'text-[#0a84ff]' : status === 'failed' ? 'text-[#ff453a]' : 'text-[#636366]';"
)
content = content.replace(
    "{detail && status === 'done' &&",
    "{detail && (status === 'done' || status === 'completed') &&"
)

# ===== Fix 3: Replace runExtraction with backend task flow =====
old_run_extraction = """  const runExtraction = useCallback(async (content, providerCfg, graphName) => {
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
  }, []);"""

new_run_extraction = """  /** Poll a backend extraction task until completion/failure */
  const pollTask = useCallback(async (taskId, onUpdate) => {
    while (true) {
      const task = await getExtractionTask(taskId);
      onUpdate(task);
      if (task.status === 'completed' || task.status === 'failed') return task;
      await new Promise((r) => setTimeout(r, 1500));
    }
  }, []);

  /** Core extraction: frontend generates title/tags, then backend extracts knowledge graph */
  const runExtraction = useCallback(async (content, providerCfg, graphName) => {
    const steps = [];
    const addStep = (s) => { steps.push(s); setImportSteps([...steps]); };

    addStep({ label: 'Generating title...', status: 'running', detail: '' });
    const title = await generateTitle(providerCfg, content);
    addStep({ label: `Title: ${title}`, status: 'done', detail: title });

    addStep({ label: 'Generating tags...', status: 'running', detail: '' });
    const tags = await generateTags(providerCfg, content);
    addStep({ label: `Tags: ${tags.join(', ')}`, status: 'done', detail: tags.join(', ')});

    addStep({ label: 'Saving document...', status: 'running', detail: '' });
    const doc = await addDocument(title, content, tags);
    addStep({ label: 'Document saved', status: 'done', detail: doc.id });

    // Submit backend extraction task
    addStep({ label: 'Submitting extraction task...', status: 'running', detail: '' });
    const { task_id } = await startDocumentExtraction(doc.id);
    addStep({ label: 'Extraction started', status: 'done', detail: `Task: ${task_id.slice(0, 8)}...` });

    // Poll for progress
    await pollTask(task_id, (task) => {
      if (task.steps) {
        setImportSteps((prev) => {
          const frontendDone = prev.filter((s) =>
            s.status === 'done' || s.status === 'failed'
          );
          const backendSteps = task.steps.map((s) => ({
            label: s.label,
            status: s.status,
            progressPct: s.progress_pct || 0,
            detail: s.detail || undefined,
          }));
          return [...frontendDone, ...backendSteps];
        });
      }
      setImporting(task.status === 'running');
    });

    // Check final result
    const finalTask = await getExtractionTask(task_id);
    if (finalTask.error && !finalTask.stats) {
      addStep({ label: finalTask.error.startsWith('DOCUMENT_TOO_LARGE')
        ? '⚠️ Document too large, please split into smaller sections'
        : `❌ Error: ${finalTask.error}`, status: 'failed', detail: '' });
    } else {
      addStep({ label: '✅ Import complete', status: 'done', detail: '' });
    }

    const docs = await listDocuments();
    setDocuments(docs.documents || []);
  }, [pollTask]);"""

if old_run_extraction in content:
    content = content.replace(old_run_extraction, new_run_extraction)
    print("runExtraction replaced")
else:
    print("runExtraction NOT found")
    idx = content.find('const runExtraction')
    if idx >= 0:
        print(content[idx:idx+100])

# ===== Fix 4: Add fetchSettings on open =====
old_effect = """  useEffect(() => {
    if (open) {
      setLoading(true);
      Promise.all([
        listDocuments().then((d) => setDocuments(d.documents || [])).catch(() => {}),
        listGraphs().then((d) => setGraphs(d.graphs || [])).catch(() => {}),
      ]).then(() => setLoading(false));
    }"""

new_effect = """  useEffect(() => {
    if (open) {
      setLoading(true);
      Promise.all([
        listDocuments().then((d) => setDocuments(d.documents || [])).catch(() => {}),
        listGraphs().then((d) => setGraphs(d.graphs || [])).catch(() => {}),
        fetchSettings().then((s) => {
          if (s?.llm?.providers?.length > 0) {
            const p = s.llm.providers;
            const defaultModel = s.llm.default_model || '';
            const provName = defaultModel.split('/')[0];
            const idx = p.findIndex((x) => x.name === provName);
            const providerId = idx >= 0 ? 'provider-' + idx : 'provider-0';
            setImportProvider(providerId);
          }
        }).catch(() => {}),
      ]).then(() => setLoading(false));
    }"""

if old_effect in content:
    content = content.replace(old_effect, new_effect)
    print("useEffect replaced")
else:
    print("useEffect NOT found")
    idx = content.find('setLoading(true)')
    if idx >= 0:
        print(content[idx-60:idx+60])

# ===== Fix 5: Wrap selectors in labeled divs + model Provider/Model format =====
old_selectors = """          {/* Graph selector */}
          <div className="flex gap-3 items-center">
            <select className="flex-1 px-3 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] appearance-none cursor-pointer" value={importGraph} onChange={(e) => setImportGraph(e.target.value)} style={{ backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'%3E%3Cpath fill='%23636366' d='M0 0l5 6 5-6z'/%3E%3C/svg%3E\")", backgroundRepeat: 'no-repeat', backgroundPosition: 'right 12px center', paddingRight: '32px' }}>
              {graphs.map((g) => <option key={g} value={g}>{g}</option>)}
            </select>
            <select className="flex-1 px-3 py-2 rounded-xl bg-[#1c1c20] text-[#e5e5e7] text-sm border-0 outline-none ring-1 ring-[#3a3a3e] focus:ring-[#0a84ff] appearance-none cursor-pointer" value={importProvider} onChange={(e) => setImportProvider(e.target.value)} style={{ backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'%3E%3Cpath fill='%23636366' d='M0 0l5 6 5-6z'/%3E%3C/svg%3E\")", backgroundRepeat: 'no-repeat', backgroundPosition: 'right 12px center', paddingRight: '32px' }}>
              {providers.map((p) => <option key={p.id} value={p.id}>{p.name} ({p.model})</option>)}
            </select>
          </div>"""

new_selectors = """          <div>
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
          </div>"""

if old_selectors in content:
    content = content.replace(old_selectors, new_selectors)
    print("Selectors replaced")
else:
    print("Selectors NOT found")
    idx = content.find('{/* Graph selector */}')
    if idx >= 0:
        print(content[idx:idx+120])

# ===== Fix 6: Text area label =====
old_ta = '          {/* Text area */}\n          <textarea'
new_ta = '          <div>\n            <label className="block text-xs text-[#636366] font-medium mb-1.5 tracking-tight">{t(\'knowledgeBase.content\')}</label>\n            <textarea'
content = content.replace(old_ta, new_ta, 1)

# Close wrapping div after textarea
old_ta_end = '/>\n\n          {/* Action buttons */}'
new_ta_end = ' />\n          </div>\n\n          {/* Action buttons */}'
content = content.replace(old_ta_end, new_ta_end, 1)

# ===== Fix 7: Upload button =====
content = content.replace('\U0001f4c4 Upload .md', "{'\U0001f4c4'} {t('knowledgeBase.upload')}")

# ===== Fix 8: Cancel button =====
content = content.replace('>Cancel<', ">{t('panel.close')}<")

# ===== Fix 9: Import button =====
content = content.replace(
    "{importing ? 'Importing...' : 'Import'}",
    "{importing ? t('knowledgeBase.import') + '...' : t('knowledgeBase.import')}"
)

# ===== Fix 10: Textarea placeholder =====
content = content.replace(
    'placeholder="Paste markdown content or drag a .md file..."',
    "placeholder={t('knowledgeBase.import') + '...'}"
)

with open('src/ui/src/components/KnowledgeBase.jsx', 'w') as f:
    f.write(content)

print("All done!")
