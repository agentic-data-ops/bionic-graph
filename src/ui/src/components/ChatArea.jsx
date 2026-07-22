import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import MessageList from './MessageList';
import ChatInput from './ChatInput';

import {
  chatCompletionProxy,
  parseSSEStream,
  gremlin,
  searchWeb,
  fetchWebSearchConfig,
} from '../api';

/** Convert HTML datetime-local value (local time, "YYYY-MM-DDTHH:MM") to UTC microseconds. */
function localDatetimeToUTC(dt) {
  const d = new Date(dt + ':00');
  return d.getTime() * 1000;
}

let _idCounter = 0;
function uid() {
  return `m${Date.now()}-${++_idCounter}`;
}

/** Extract and format graph search context from `search_progress` messages.
 *  Output uses [Entity] / [Relation] labels in English. */
function formatGraphContext(items) {
  if (!items?.length) return '';
  return items
    .slice(0, 80)
    .map((item) => {
      if (item.type === 'vertex') {
        let s = `[Entity] ${item.name}${item.labels?.length ? ' (' + item.labels.join(', ') + ')' : ''}`;
        if (item.keywords?.length) {
          s += ` — keywords: ${item.keywords.join(', ')}`;
        }
        return s;
      } else if (item.type === 'edge') {
        let s = `[Relation] ${item.name}: ${item.source} → ${item.target}`;
        if (item.strength !== undefined && item.strength !== 1.0) {
          s += ` (strength: ${item.strength})`;
        }
        if (item.keywords?.length) {
          s += ` — ${item.keywords.join(', ')}`;
        }
        return s;
      }
      return '';
    })
    .filter(Boolean)
    .join('\n');
}

/** Collect graph search data from conversation history + current search result. */
function collectGraphContext(convMessages, currentSearchData) {
  const ctx = [];
  // Historical: extract from search_progress messages
  if (convMessages) {
    for (const msg of convMessages) {
      if (msg.type === 'search_progress' && msg.graphData?.data?.length) {
        ctx.push(...msg.graphData.data);
      }
    }
  }
  // Current (if any)
  if (currentSearchData?.data?.length) {
    ctx.push(...currentSearchData.data);
  }
  return formatGraphContext(ctx);
}

export default function ChatArea({
  activeConv,
  onUpdateConv,
  providers,
  activeProvider,
  onProviderChange,
  useGraph,
  onGraphToggle,
  timeTravel,
  onTimeTravelToggle,
  timeTravelPoint,
  onTimeTravelPointChange,
  defaultGraph,
  onDefaultGraphChange,
  graphs,
  timeTravelGraphs,
  graphMetas,
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
  const [kwSearchMode, setKwSearchMode] = useState(() => localStorage.getItem('bgraph-kw-search-mode') || "greedy");
  const [useWebSearch, setUseWebSearch] = useState(() => localStorage.getItem('bgraph-web-search') === 'true');
  const [extractKeywords, setExtractKeywords] = useState(() => localStorage.getItem('bgraph-extract-keywords') !== 'false');
  const [searchStream, setSearchStream] = useState(null);
  const abortRef = useRef(null);
  const [isGenerating, setIsGenerating] = useState(false);

  // Reset search stream when active conversation changes
  useEffect(() => {
    setSearchStream(null);
  }, [activeConv?.id]);

  // Sync extractKeywords to localStorage
  useEffect(() => {
    localStorage.setItem('bgraph-extract-keywords', extractKeywords);
  }, [extractKeywords]);

  // Sync useWebSearch to localStorage
  useEffect(() => {
    localStorage.setItem('bgraph-web-search', useWebSearch);
  }, [useWebSearch]);

  // Sync kwSearchMode to localStorage
  useEffect(() => {
    localStorage.setItem('bgraph-kw-search-mode', kwSearchMode);
  }, [kwSearchMode]);

  // ── Handle sending a message ──
  const handleSend = useCallback(
    async (text) => {
      const conv = activeConv;
      if (!conv) return;

      const userMsg = { id: uid(), type: 'user', content: text };
      const updatedMsgs = [...(conv.messages || []), userMsg];
      onUpdateConv({ ...conv, messages: updatedMsgs });

      let allSearchMsgs = []; // all progress messages to append before assistant
      let graphData = null; // final graph search data
      let webSearchContext = ''; // accumulated web search text for LLM
      let webSearchDetail = ''; // details about what was found
      const modelKey = `${activeProvider}/${chatModel || 'default'}`;

      // ── 1. Web search ──
      if (useWebSearch) {
        try {
          const wsConfig = await fetchWebSearchConfig();
          const provider = (wsConfig?.providers || []).find(p => p.id === wsConfig.default_provider) || wsConfig?.providers?.[0];
          if (provider) {
            const wsSteps = [];
            const wsProgressId = uid();
            const wsProgressMsg = { id: wsProgressId, type: 'web_search_progress', title: text, steps: wsSteps, graphName: defaultGraph };
            setSearchStream(wsProgressMsg);
            setIsGenerating(true);

            let searchQuery = text;

            // Step 0: Extract search keywords (if enabled)
            if (extractKeywords) {
              wsSteps.push({ icon: '🔑', name: 'Extracting search keywords...', status: 'running', llmOutput: '' });
              setSearchStream({ ...wsProgressMsg, steps: [...wsSteps] });

              try {
                const recentMsgs = (conv.messages || []).slice(-6).filter(m => m.type === 'user' || m.type === 'assistant');
                const kwContextMsgs = recentMsgs.map(m => ({
                  role: m.type === 'user' ? 'user' : 'assistant',
                  content: m.content || '',
                }));
                const kwMessages = [
                  { role: 'system', content: 'You are a search query optimizer. Based on the conversation history and the latest question, extract 2-5 concise search keywords. Return ONLY the keywords separated by spaces, no punctuation or extra text. Focus on core entities and omit generic words like "介绍", "什么是", "怎么样", "how", "what", "explain".' },
                  ...kwContextMsgs,
                ];
                const kwRes = await chatCompletionProxy(kwMessages, modelKey, false);
                const kwData = await kwRes.response;
                const kwBody = await kwData.json();
                const kwContent = (kwBody?.choices?.[0]?.message?.content || '').trim();
                if (kwContent && kwContent.split(' ').length <= 8) searchQuery = kwContent;
              } catch (e) { /* use original text as fallback */ }

              wsSteps[wsSteps.length - 1] = { ...wsSteps[wsSteps.length - 1], status: 'done', llmOutput: searchQuery };
              setSearchStream({ ...wsProgressMsg, steps: [...wsSteps] });
            }

            // Step 1: Search web
            wsSteps.push({ icon: '🌐', name: 'Searching web...', status: 'running', llmOutput: '' });
            setSearchStream({ ...wsProgressMsg, steps: [...wsSteps] });

            const rawHtml = await searchWeb(provider, searchQuery);
            wsSteps[wsSteps.length - 1] = { ...wsSteps[wsSteps.length - 1], status: 'done', llmOutput: `Received ${rawHtml.length} chars from ${provider.name}` };

            // Step 2: Pass raw search results to LLM as reference
            webSearchContext = rawHtml.slice(0, 32000);
            webSearchDetail = '';
            let webResults = [];

            const finalWsMsg = { ...wsProgressMsg, steps: [...wsSteps], webResults, webDetail: webSearchDetail };
            allSearchMsgs.push(finalWsMsg);
          }
        } catch (e) {
          console.error('Web search error:', e);
          setSearchStream(null);
          allSearchMsgs.push({ id: uid(), type: 'web_search_progress', title: text, steps: [{ icon: '❌', name: `Web search failed: ${e.message}`, status: 'failed' }], graphName: defaultGraph });
        }
      }

      // ── 2. Graph search ──
      if (useGraph) {
        const searchStep = { icon: '🔍', name: 'Searching knowledge graph', status: 'running', llmOutput: '' };
        const steps = [searchStep];

        const ttMicros = timeTravel && timeTravelPoint ? localDatetimeToUTC(timeTravelPoint) : null;
        const progressMsgId = uid();
        const ttEnabled = (Array.isArray(graphMetas) ? graphMetas.find(g => g.name === defaultGraph)?.time_travel : false) || false;
        const progressMsg = { id: progressMsgId, type: 'search_progress', title: text, steps, timeTravelEnabled: ttEnabled, timeTravelAt: ttMicros };
        if (allSearchMsgs.length === 0) {
          setSearchStream(progressMsg);
        }
        setIsGenerating(true);

        let graphQuery = text;

        try {
          // Check keyword extraction setting
          if (extractKeywords) {
            steps.push({ icon: '🔑', name: 'Extracting search keywords...', status: 'running', llmOutput: '' });
            setSearchStream({ ...progressMsg, steps: [...steps] });

            try {
              const recentMsgs = (conv.messages || []).slice(-6).filter(m => m.type === 'user' || m.type === 'assistant');
              const kwContextMsgs = recentMsgs.map(m => ({
                role: m.type === 'user' ? 'user' : 'assistant',
                content: m.content || '',
              }));
              const kwMessages = [
                { role: 'system', content: 'You are a search query optimizer. Based on the conversation history and the latest question, extract 2-5 concise search keywords. Return ONLY the keywords separated by spaces, no punctuation or extra text. Focus on core entities and omit generic words.' },
                ...kwContextMsgs,
              ];
              const kwRes = await chatCompletionProxy(kwMessages, modelKey, false);
              const kwData = await kwRes.response;
              const kwBody = await kwData.json();
              const kwContent = (kwBody?.choices?.[0]?.message?.content || '').trim();
              if (kwContent && kwContent.split(' ').length <= 8) graphQuery = kwContent;
            } catch (e) { /* use original text as fallback */ }

            steps[steps.length - 1] = { ...steps[steps.length - 1], status: 'done', llmOutput: graphQuery };
            setSearchStream({ ...progressMsg, steps: [...steps] });
          }

          const gremlinSteps = [{ step: 'search', text: graphQuery, mode: kwSearchMode }];
          const res = await gremlin(gremlinSteps, defaultGraph, ttMicros);
          graphData = res;

          const doneSteps = [{ icon: '✅', name: 'Graph search completed', status: 'done', llmOutput: '' }];
          const ttEnabled2 = (Array.isArray(graphMetas) ? graphMetas.find(g => g.name === defaultGraph)?.time_travel : false) || false;
          const searchMsg = { ...progressMsg, steps: doneSteps, graphData: res, graphName: defaultGraph, timeTravelEnabled: ttEnabled2, timeTravelAt: ttMicros };
          setSearchStream(null);
          allSearchMsgs.push(searchMsg);
        } catch (e) {
          const failedSteps = (steps || []).map((s) => ({ ...s, status: 'failed' }));
          setSearchStream(null);
          allSearchMsgs.push({ ...progressMsg, steps: failedSteps });
          requestAnimationFrame(() => chatInputRef.current?.focus());
          setIsGenerating(false);
          abortRef.current = null;
          return;
        }
      }

      // ── 3. Build combined context & call LLM ──
      setIsGenerating(true);

      // Commit search messages to conversation BEFORE clearing search stream
      let allUpdatedMsgs;
      if (allSearchMsgs.length > 0) {
        allUpdatedMsgs = [...updatedMsgs, ...allSearchMsgs];
        onUpdateConv({ ...conv, messages: allUpdatedMsgs });
      } else {
        allUpdatedMsgs = updatedMsgs;
      }
      setSearchStream(null);

      // Build LLM conversation history
      const llmMessages = updatedMsgs
        .filter((m) => m.type === 'user' || (m.type === 'assistant' && m.content))
        .map((m) => ({
          role: m.type === 'user' ? 'user' : 'assistant',
          content: m.content,
        }));

      // Inject web search context
      if (webSearchContext) {
        llmMessages.unshift({
          role: 'system',
          content: `The following information was retrieved from the web search. Prioritize it when answering the user's question.
If the web data is sufficient, directly reference it. If not, supplement with your own knowledge.

${webSearchContext}`,
        });
      }

      // Inject graph search context
      const graphCtx = collectGraphContext(conv.messages, graphData);
      if (graphCtx) {
        llmMessages.unshift({
          role: 'system',
          content: `The following information was retrieved from the knowledge graph. Prioritize it when answering the user's question.
If the graph data is sufficient, directly reference its entities and relationships. If not, supplement with your own knowledge.
Do not mention entity or relationship ID numbers — use their names directly.
Provide the answer first, then the reasoning process.

${graphCtx}`,
        });
      }

      const assistantMsg = { id: uid(), type: 'assistant', content: '' };

      try {
        const { response, abort } = chatCompletionProxy(llmMessages, modelKey);
        abortRef.current = abort;
        let fullContent = '';
        await parseSSEStream(
          await response,
          (token) => {
            fullContent += token;
            onUpdateConv({
              ...conv,
              messages: [...allUpdatedMsgs, { ...assistantMsg, content: fullContent }],
            });
          },
        );
        requestAnimationFrame(() => chatInputRef.current?.focus());
      } catch (e) {
        if (e.name === 'AbortError') return;
        onUpdateConv({
          ...conv,
          messages: [...allUpdatedMsgs, { ...assistantMsg, content: `**Error**: ${e.message}` }],
        });
      } finally {
        abortRef.current = null;
        setIsGenerating(false);
      }
    },
    [activeConv, useGraph, useWebSearch, extractKeywords, defaultGraph, providers, activeProvider, onUpdateConv, chatModel, kwSearchMode, timeTravel, timeTravelPoint, timeTravelGraphs, graphMetas]
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
            <button className="w-7 h-7 rounded-lg bg-[var(--bg-tertiary)] flex items-center justify-center text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] transition-all" onClick={() => setLangOpen(!langOpen)} title="Language">
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M12 2a10 10 0 110 20 10 10 0 010-20zM2 12h20M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z" />
              </svg>
            </button>
            {langOpen && (
              <>
                <div className="fixed inset-0 z-40" onClick={() => setLangOpen(false)} />
                <div className="absolute right-0 bottom-full mb-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden w-max">
                  <button
                    className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap transition-all ${language === 'zh' ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                    onClick={() => { onLanguageToggle('zh'); setLangOpen(false); }}
                  >中文</button>
                  <button
                    className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap transition-all ${language === 'en' ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
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
          onDataChange={(items) => {
            const conv = activeConv;
            if (!conv) return;
            const itemIds = new Set(items.map(i => i.id));
            const updatedMsgs = conv.messages.map((m) => {
              const graphSrc = m.graphData || m.data;
              if (graphSrc?.data?.length) {
                const match = graphSrc.data.some(d => itemIds.has(d.id));
                if (match) {
                  if (m.graphData) return { ...m, graphData: { ...m.graphData, data: items } };
                  return { ...m, data: { ...m.data, data: items } };
                }
              }
              return m;
            });
            onUpdateConv({ ...conv, messages: updatedMsgs });
          }}
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
        useWebSearch={useWebSearch}
        onWebSearchToggle={setUseWebSearch}
        extractKeywords={extractKeywords}
        onExtractKeywordsToggle={setExtractKeywords}
        timeTravel={timeTravel}
        onTimeTravelToggle={onTimeTravelToggle}
        timeTravelPoint={timeTravelPoint}
        onTimeTravelPointChange={onTimeTravelPointChange}
        graphMetas={graphMetas}
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
