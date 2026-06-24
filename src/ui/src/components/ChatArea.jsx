import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import MessageList from './MessageList';
import ChatInput from './ChatInput';

import {
  chatCompletionProxy,
  parseSSEStream,
  graphSearch,
} from '../api';

let _idCounter = 0;
function uid() {
  return `m${Date.now()}-${++_idCounter}`;
}

export default function ChatArea({
  activeConv,
  onUpdateConv,
  providers,
  activeProvider,
  onProviderChange,
  useGraph,
  onGraphToggle,
  searchMode,
  onSearchModeChange,
  timeTravel,
  onTimeTravelToggle,
  timeTravelPoint,
  defaultGraph,
  onDefaultGraphChange,
  graphs,
  timeTravelGraphs,
  defaultModelKey,
  chatModel,
  onChatModelChange,
  theme,
  onThemeToggle,
  language,
  onLanguageToggle,
  onSaveToKB,
}) {
  const { t } = useTranslation();
  const [langOpen, setLangOpen] = useState(false);

  const chatInputRef = useRef(null);
  const [kwSearchMode, setKwSearchMode] = useState("greedy");
  const [searchStream, setSearchStream] = useState(null);
  const abortRef = useRef(null);
  const [isGenerating, setIsGenerating] = useState(false);

  // Reset search stream when active conversation changes
  useEffect(() => {
    setSearchStream(null);
  }, [activeConv?.id]);

  // ── Handle sending a message ──
  const handleSend = useCallback(
    async (text) => {
      const conv = activeConv;
      if (!conv) return;

      const userMsg = { id: uid(), type: 'user', content: text };
      const updatedMsgs = [...(conv.messages || []), userMsg];
      onUpdateConv({ ...conv, messages: updatedMsgs });

      if (useGraph) {
        // ── Graph mode: keyword or semantic search ──
        const isSemantic = searchMode === 'semantic';
        const modelKey = `${activeProvider}/${chatModel || 'default'}`;

        const steps = isSemantic
          ? [
              { icon: '🔑', name: 'Extracting search keywords', status: 'pending', llmOutput: '' },
              { icon: '🔍', name: 'Searching knowledge graph', status: 'pending', llmOutput: '' },
              { icon: '🎯', name: 'Filtering semantically relevant results', status: 'pending', llmOutput: '' },
            ]
          : [
              { icon: '🔍', name: 'Searching knowledge graph', status: 'running', llmOutput: '' },
            ];

        const progressMsgId = uid();
        const progressMsg = { id: progressMsgId, type: 'search_progress', title: text, steps };
        setSearchStream(progressMsg); // only in stream, not saved to conversation
        setIsGenerating(true);

        try {
          let keywordsArr;
          let step1done;
          if (isSemantic) {
            // Step 1: Extract keywords via LLM (streaming)
            const step1 = { icon: '🔑', name: 'Extracting search keywords', status: 'running', llmOutput: '' };
            setSearchStream({ ...progressMsg, steps: [step1, progressMsg.steps[1], progressMsg.steps[2]] });

            const systemPrompt = 'Select 3-5 key search keywords from the user\'s query below. ONLY pick words/phrases that actually appear in the query — do NOT generate, infer, or translate any new words. Return ONLY a JSON array of strings, no other text.';
            const { response, abort } = chatCompletionProxy(
              [{ role: 'system', content: systemPrompt }, { role: 'user', content: `Query: ${text}` }],
              modelKey,
            );
            abortRef.current = abort;
            let llmBuf = '';
            await parseSSEStream(await response, (t) => {
              llmBuf += t;
              setSearchStream({
                ...progressMsg,
                steps: [
                  { icon: '🔑', name: 'Extracting search keywords', status: 'running', llmOutput: llmBuf },
                  progressMsg.steps[1],
                  progressMsg.steps[2],
                ],
              });
            });
            abortRef.current = null;
            try { keywordsArr = JSON.parse(llmBuf.trim()); }
            catch { keywordsArr = text.split(/\s+/).filter(Boolean); }

            step1done = { icon: '✅', name: `Extracted keywords: ${keywordsArr.join(', ')}`, status: 'done', llmOutput: llmBuf };
            const step2run = { icon: '🔍', name: 'Searching knowledge graph', status: 'running', llmOutput: '' };
            setSearchStream({ ...progressMsg, steps: [step1done, step2run, progressMsg.steps[2]] });
          } else {
            keywordsArr = text.split(/\s+/).filter(Boolean);
          }

          // Step 2 (or only step for keyword): Search graph
          // When semantic mode, always use greedy for the API call
          const effectiveKwMode = isSemantic ? 'greedy' : kwSearchMode;
          const res = await graphSearch(keywordsArr, defaultGraph, effectiveKwMode);

          if (!isSemantic) {
            const doneSteps = [{ icon: '✅', name: 'Graph search completed', status: 'done', llmOutput: '' }];
            setSearchStream(null);
            requestAnimationFrame(() => chatInputRef.current?.focus());
            abortRef.current = null;
            setIsGenerating(false);
            onUpdateConv({ ...conv, messages: [...updatedMsgs, { ...progressMsg, steps: doneSteps, graphData: res, graphName: defaultGraph }] });
            return;
          }

          const step2done = { icon: '✅', name: 'Graph search completed', status: 'done', llmOutput: '' };
          const step3run = { icon: '🎯', name: 'Filtering semantically relevant results', status: 'running', llmOutput: '' };
          setSearchStream({ ...progressMsg, steps: [step1done, step2done, step3run] });

          // Step 3: Filter results via LLM (streaming)
          const items = (res?.data || []).slice(0, 30);
          const filterPrompt = `You are a semantic relevance filter. Given a user query and a list of search results, identify which results are semantically relevant to the query.

The search results are graph data with two types of items:
- vertex: represents an entity, with fields: name, type, labels, properties
- edge: represents a relationship, with fields: label, source (vertex id), target (vertex id)

Selection rules:
1. Select vertices that match the entities mentioned in the query
2. Select edges whose label matches the relationship described in the query
3. If you select an edge, ALSO select its source and target vertices (even if they weren't explicitly mentioned)

Return ONLY a comma-separated list of 1-based array indices of the selected items. If none are relevant, return "NONE". No other text.`;
          const { response: filterResponse, abort: filterAbort } = chatCompletionProxy(
            [{ role: 'system', content: filterPrompt }, { role: 'user', content: `Query: ${text}\n\nSearch Results:\n${JSON.stringify(items, null, 2)}` }],
            modelKey,
          );
          abortRef.current = filterAbort;
          let filterBuf = '';
          await parseSSEStream(await filterResponse, (t) => {
            filterBuf += t;
            setSearchStream({
              ...progressMsg,
              steps: [step1done, step2done, { icon: '🎯', name: 'Filtering semantically relevant results', status: 'running', llmOutput: filterBuf }],
            });
          });
          abortRef.current = null;

          const text2 = filterBuf.trim();
          let filteredData;
          if (text2 === 'NONE') {
            filteredData = { ...res, data: [] };
          } else {
            const indices = text2.split(',').map((s) => parseInt(s.trim(), 10) - 1).filter((i) => !isNaN(i) && i >= 0 && i < items.length);
            const selected = indices.length > 0 ? indices.map((i) => items[i]) : items;
            // Collect vertex IDs from filtered results, then include edges that connect them
            const keptVertexIds = new Set(selected.filter((i) => i.type === 'vertex').map((i) => i.id));
            const allData = (res?.data || []);
            const extraEdges = allData.filter((i) => i.type === 'edge' && keptVertexIds.has(i.source) && keptVertexIds.has(i.target));
            // Merge: selected items (minus edges duplicated by extra) + extra edges
            const selectedIds = new Set(selected.map((i) => i.type === 'edge' ? `e:${i.id}` : `v:${i.id}`));
            const merged = [...selected.filter((i) => i.type !== 'edge' || !extraEdges.some((e) => e.id === i.id)), ...extraEdges];
            filteredData = { ...res, data: merged };
          }

          setSearchStream(null);
          const finalSteps = [step1done, step2done, { icon: '✅', name: 'Filtering completed', status: 'done', llmOutput: filterBuf }];
          onUpdateConv({ ...conv, messages: [...updatedMsgs, { ...progressMsg, steps: finalSteps, graphData: filteredData, graphName: defaultGraph }] });
          requestAnimationFrame(() => chatInputRef.current?.focus());
        } catch (e) {
          const failedSteps = (steps || []).map((s) => ({ ...s, status: 'failed' }));
          setSearchStream(null);
          onUpdateConv({ ...conv, messages: [...updatedMsgs, { ...progressMsg, steps: failedSteps }] });
          requestAnimationFrame(() => chatInputRef.current?.focus());
        } finally {
          abortRef.current = null;
          setIsGenerating(false);
        }
      } else {
        // ── LLM mode: streaming chat ──
        const modelKey = `${activeProvider}/${chatModel || 'default'}`;

        // Build message list for LLM — skip assistant placeholders with empty content
        // Only send user/assistant messages with content to the LLM
        const llmMessages = updatedMsgs
          .filter((m) => m.type === 'user' || (m.type === 'assistant' && m.content))
          .map((m) => ({
            role: m.type === 'user' ? 'user' : 'assistant',
            content: m.content,
          }));

        const assistantMsg = { id: uid(), type: 'assistant', content: '' };

        try {
          const { response, abort } = chatCompletionProxy(llmMessages, modelKey);
          abortRef.current = abort;
          setIsGenerating(true);
          let fullContent = '';
          await parseSSEStream(
            await response,
            (token) => {
              fullContent += token;
              onUpdateConv({
                ...conv,
                messages: [...updatedMsgs, { ...assistantMsg, content: fullContent }],
              });
            },
          );
          requestAnimationFrame(() => chatInputRef.current?.focus());
        } catch (e) {
          if (e.name === 'AbortError') return;
          onUpdateConv({
            ...conv,
            messages: [...updatedMsgs, { ...assistantMsg, content: `**Error**: ${e.message}` }],
          });
        } finally {
          abortRef.current = null;
          setIsGenerating(false);
        }
      }
    },
    [activeConv, useGraph, searchMode, defaultGraph, providers, activeProvider, onUpdateConv, chatModel, kwSearchMode]
  );

  const messages = activeConv?.messages || [];

  return (
    <div className="flex-1 flex flex-col min-w-0 bg-[var(--bg-primary)]">
      <div className="px-5 py-3 border-b border-[var(--border)] bg-[var(--bg-secondary)] flex items-center justify-between">
        <h2 className="text-sm font-semibold text-[var(--text-primary)] truncate tracking-tight">
          {activeConv?.title || t('chat.newChat')}
        </h2>
        <div className="flex items-center gap-2 flex-shrink-0 ml-4">
          <button className="w-7 h-7 rounded-lg bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-xs transition-all" onClick={onThemeToggle} title={theme === 'dark' ? 'Light mode' : 'Dark mode'}>
            {theme === 'dark' ? '☀️' : '🌙'}
          </button>
          <div className="relative">
            <button className="px-2 py-1 rounded-lg bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] text-xs font-medium transition-all" onClick={() => setLangOpen(!langOpen)}>
              LANG
            </button>
            {langOpen && (
              <>
                <div className="fixed inset-0 z-40" onClick={() => setLangOpen(false)} />
                <div className="absolute right-0 top-full mt-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden min-w-[120px]">
                  <button
                    className={`w-full text-left px-3 py-2 text-xs font-medium transition-all ${language === 'zh' ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                    onClick={() => { onLanguageToggle('zh'); setLangOpen(false); }}
                  >中文</button>
                  <button
                    className={`w-full text-left px-3 py-2 text-xs font-medium transition-all ${language === 'en' ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                    onClick={() => { onLanguageToggle('en'); setLangOpen(false); }}
                  >English</button>
                </div>
              </>
            )}
          </div>
        </div>
      </div>

      <div className="flex-1 flex flex-col min-h-0">
        <MessageList messages={messages} searchStream={searchStream} theme={theme}
          onEdit={(text) => { chatInputRef.current?.setText(text); requestAnimationFrame(() => chatInputRef.current?.focus()); }}
          onSaveToKB={onSaveToKB}
        />
      </div>

      <ChatInput
        ref={chatInputRef}
        isGenerating={isGenerating}
        onStop={() => { abortRef.current?.(); abortRef.current = null; setIsGenerating(false); }}
        kwSearchMode={kwSearchMode}
        onkwSearchModeChange={setKwSearchMode}
        providers={providers}
        activeProvider={activeProvider}
        onProviderChange={onProviderChange}
        useGraph={useGraph}
        onGraphToggle={onGraphToggle}
        searchMode={searchMode}
        onSearchModeChange={onSearchModeChange}
        timeTravel={timeTravel}
        onTimeTravelToggle={onTimeTravelToggle}
        timeTravelPoint={timeTravelPoint}
        onTimeTravelPointChange={(v) => {}}
        timeTravelGraphs={timeTravelGraphs}
        defaultModelKey={defaultModelKey}
        graphName={defaultGraph}
        onGraphNameChange={onDefaultGraphChange}
        graphs={graphs}
        chatModel={chatModel}
        onChatModelChange={onChatModelChange}
        onSend={handleSend}
        disabled={!!searchStream}
      />
    </div>
  );
}
