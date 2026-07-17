import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import MessageList from './MessageList';
import ChatInput from './ChatInput';

import {
  chatCompletionProxy,
  parseSSEStream,
  gremlin,
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
        const searchStep = { icon: '🔍', name: 'Searching knowledge graph', status: 'running', llmOutput: '' };
        const steps = [searchStep];

        const ttMicros = timeTravel && timeTravelPoint ? localDatetimeToUTC(timeTravelPoint) : null;
        const progressMsgId = uid();
        const ttEnabled = (Array.isArray(graphMetas) ? graphMetas.find(g => g.name === defaultGraph)?.time_travel : false) || false;
        const progressMsg = { id: progressMsgId, type: 'search_progress', title: text, steps, timeTravelEnabled: ttEnabled, timeTravelAt: ttMicros };
        setSearchStream(progressMsg);
        setIsGenerating(true);

        try {
          const gremlinSteps = [{ step: 'search', text, mode: kwSearchMode, at: ttMicros }];
          if (ttMicros) gremlinSteps.push({ step: 'timeTravel', at: ttMicros });

          const res = await gremlin(gremlinSteps, defaultGraph);

          let finalData = res;

          // Show search progress message
          const doneSteps = [{ icon: '✅', name: 'Graph search completed', status: 'done', llmOutput: '' }];
          setSearchStream(null);
          const ttEnabled2 = (Array.isArray(graphMetas) ? graphMetas.find(g => g.name === defaultGraph)?.time_travel : false) || false;
          const searchMsg = { ...progressMsg, steps: doneSteps, graphData: finalData, graphName: defaultGraph, timeTravelEnabled: ttEnabled2, timeTravelAt: ttMicros };
          onUpdateConv({ ...conv, messages: [...updatedMsgs, searchMsg] });

          // ── Call LLM with graph context (informational, no restrictive prompt) ──
          const modelKey = `${activeProvider}/${chatModel || 'default'}`;

          // Build conversation history (same as non-graph mode)
          const llmMessages = updatedMsgs
            .filter((m) => m.type === 'user' || (m.type === 'assistant' && m.content))
            .map((m) => ({
              role: m.type === 'user' ? 'user' : 'assistant',
              content: m.content,
            }));

          // Inject graph search context — LLM should prioritize this data
          const graphCtx = collectGraphContext(conv.messages, finalData);
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
            setIsGenerating(true);
            let fullContent = '';
            await parseSSEStream(
              await response,
              (token) => {
                fullContent += token;
                onUpdateConv({
                  ...conv,
                  messages: [...updatedMsgs, searchMsg, { ...assistantMsg, content: fullContent }],
                });
              },
            );
          } catch (e) {
            if (e.name === 'AbortError') return;
            onUpdateConv({
              ...conv,
              messages: [...updatedMsgs, searchMsg, { ...assistantMsg, content: `**Error**: ${e.message}` }],
            });
          }

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

        // Even in non-graph mode, inject historical graph context from previous turns
        const graphCtx = collectGraphContext(conv.messages, null);
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
    [activeConv, useGraph, defaultGraph, providers, activeProvider, onUpdateConv, chatModel, kwSearchMode, timeTravel, timeTravelPoint, timeTravelGraphs, graphMetas]
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
