import { useEffect, useRef, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import GraphViewer from './GraphViewer';

function SimpleMarkdown({ text }) {
  if (!text) return null;
  const escaped = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  const html = escaped
    .replace(/\*\*(.+?)\*\*/g, '<strong class="font-semibold">$1</strong>')
    .replace(/`(.+?)`/g, '<code class="bg-[#2a2a2e] px-1.5 py-0.5 rounded-md text-xs font-mono text-[#ff9f0a]">$1</code>')
    .replace(/\n/g, '<br/>');
  return <span dangerouslySetInnerHTML={{ __html: html }} />;
}

function SearchStep({ step }) {
  const icon = step.status === 'done' ? (step.icon || '✅')
    : step.status === 'running' ? '⏳'
    : step.icon || '⏸';
  const textColor = step.status === 'done' ? 'text-[#30d158]'
    : step.status === 'running' ? 'text-[#0a84ff]'
    : step.status === 'failed' ? 'text-[#ff453a]'
    : 'text-[#636366]';
  return (
    <div className="py-1.5">
      <div className="flex items-center gap-2">
        <span className="text-xs">{icon}</span>
        <span className={`text-xs ${textColor} font-medium tracking-tight`}>{step.name}</span>
        {step.status === 'running' && (
          <span className="inline-flex gap-0.5 ml-1">
            <span className="w-1 h-1 rounded-full bg-[#0a84ff] pulse-dot" />
            <span className="w-1 h-1 rounded-full bg-[#0a84ff] pulse-dot" style={{ animationDelay: '0.2s' }} />
            <span className="w-1 h-1 rounded-full bg-[#0a84ff] pulse-dot" style={{ animationDelay: '0.4s' }} />
          </span>
        )}
      </div>
      {step.llmOutput && step.status === 'done' && (
        <div className="mt-1.5 ml-5 text-[11px] text-[#636366] leading-relaxed font-mono whitespace-pre-wrap border-l border-[#2a2a2e] pl-3">{step.llmOutput}</div>
      )}
      {step.llmOutput && step.status === 'running' && (
        <div className="mt-1.5 ml-5 text-[11px] text-[#48484a] leading-relaxed font-mono whitespace-pre-wrap border-l border-[#2a2a2e] pl-3 max-h-20 overflow-y-auto">{step.llmOutput}</div>
      )}
    </div>
  );
}

function ChatMessage({ message, graphRef, onMaximizeRef }) {
  const { t } = useTranslation();
  if (message.type === 'user') {
    return <div className="flex justify-end mb-3 message-enter"><div className="max-w-[72%] bg-[#0a84ff] text-white rounded-2xl rounded-br-md px-4 py-2.5 text-sm leading-relaxed shadow-sm select-text">{message.content}</div></div>;
  }
  if (message.type === 'assistant') {
    const hasContent = message.content?.length > 0;
    return (
      <div className="flex justify-start mb-3 message-enter">
        <div className="max-w-[72%] bg-[#2a2a2e] text-[#e5e5e7] rounded-2xl rounded-bl-md px-4 py-2.5 text-sm leading-relaxed shadow-sm select-text">
          {hasContent ? <SimpleMarkdown text={message.content} /> : (
            <span className="inline-flex gap-1">
              <span className="w-1.5 h-1.5 rounded-full bg-[#0a84ff] pulse-dot" />
              <span className="w-1.5 h-1.5 rounded-full bg-[#0a84ff] pulse-dot" style={{ animationDelay: '0.2s' }} />
              <span className="w-1.5 h-1.5 rounded-full bg-[#0a84ff] pulse-dot" style={{ animationDelay: '0.4s' }} />
            </span>
          )}
        </div>
      </div>
    );
  }
  if (message.type === 'search_progress') {
    return (
      <div className="flex justify-start mb-3 message-enter">
        <div className="w-full max-w-[90%] bg-[#1c1c20] border border-[#2a2a2e] rounded-2xl overflow-hidden shadow-sm">
          <div className="px-4 py-3.5">
            <div className="text-xs text-[#0a84ff] font-semibold mb-2 tracking-tight">🔎 Graph Search · <span className="text-[#636366] font-normal">{message.title}</span></div>
            <div className="space-y-0">{(message.steps || []).map((step, i) => <SearchStep key={i} step={step} />)}</div>
          </div>
          {message.graphData && (
            <div className="border-t border-[#2a2a2e]">
              <div className="px-4 py-2 bg-[#1c1c20] border-b border-[#2a2a2e] flex items-center gap-2">
                <span className="text-xs font-semibold text-[#e5e5e7] tracking-tight">{t('chat.searchResult')}</span>
                <span className="text-xs text-[#636366] ml-auto font-medium">{message.graphData?.data?.length || 0} <span className="text-[#48484a]">items</span></span>
                <button className="w-6 h-6 rounded-md bg-[#2a2a2e] hover:bg-[#3a3a3e] flex items-center justify-center text-[#636366] hover:text-white transition-all flex-shrink-0 ml-2" onClick={() => onMaximizeRef?.(message.graphName)} title={t('chat.maximize')}>
                  <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
                  </svg>
                </button>
              </div>
              <div className="h-[420px] relative"><GraphViewer ref={graphRef} data={message.graphData} graph={message.graphName} /></div>
            </div>
          )}
        </div>
      </div>
    );
  }
  if (message.type === 'graph_result') {
    return (
      <div className="flex justify-start mb-3 message-enter">
        <div className="w-full max-w-[90%] bg-[#1c1c20] border border-[#2a2a2e] rounded-2xl overflow-hidden shadow-sm">
          <div className="px-4 py-2.5 bg-[#1c1c20] border-b border-[#2a2a2e] flex items-center gap-2">
            <span className="text-xs font-semibold text-[#e5e5e7] tracking-tight">{t('chat.searchResult')}</span>
            <span className="text-xs text-[#636366] ml-auto font-medium">{message.data?.data?.length || 0} <span className="text-[#48484a]">items</span></span>
            <button className="w-6 h-6 rounded-md bg-[#2a2a2e] hover:bg-[#3a3a3e] flex items-center justify-center text-[#636366] hover:text-white transition-all flex-shrink-0 ml-2" onClick={() => onMaximizeRef?.(message.graphName)} title={t('chat.maximize')}>
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
              </svg>
            </button>
          </div>
          <div className="h-[420px] relative"><GraphViewer ref={graphRef} data={message.data} graph={message.graphName} /></div>
        </div>
      </div>
    );
  }
  return null;
}

export default function MessageList({ messages, searchStream }) {
  const { t } = useTranslation();
  const bottomRef = useRef(null);
  const inlineRefs = useRef({});
  const fullscreenRef = useRef(null);
  const [maximized, setMaximized] = useState(null); // { msgId, graphName }

  const handleMaximize = useCallback((msgId, graphName) => {
    const ref = inlineRefs.current[msgId];
    if (ref?.getSnapshot) {
      const snap = ref.getSnapshot();
      if (snap) setMaximized({ msgId, graphName, ...snap });
    }
  }, []);

  const handleRestore = useCallback(() => {
    // Get snapshot from fullscreen view
    const fsSnap = fullscreenRef.current?.getSnapshot();
    // Apply to inline view
    if (maximized?.msgId && fsSnap) {
      const inlineRef = inlineRefs.current[maximized.msgId];
      inlineRef?.applySnapshot?.(fsSnap);
    }
    setMaximized(null);
  }, [maximized]);

  useEffect(() => { bottomRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' }); }, [messages, searchStream]);

  const allMessages = searchStream ? [...messages, {
    id: '__search_progress__', type: 'search_progress',
    title: searchStream.title || searchStream.query || 'Graph Search',
    steps: searchStream.steps || [], graphData: searchStream.graphData, graphName: searchStream.graphName,
  }] : messages;

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="flex-1 overflow-y-auto px-4 py-4 min-h-0 relative scroll-smooth">
        {allMessages.length === 0 && (
          <div className="h-full flex flex-col items-center justify-center text-[#48484a]">
            <div className="w-14 h-14 rounded-2xl bg-[#2a2a2e] flex items-center justify-center mb-5 shadow-sm">
              <svg className="w-7 h-7 text-[#636366]" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M9.75 3.104v5.714a2.25 2.25 0 01-.659 1.591L5 14.5M9.75 3.104c-.251.023-.501.05-.75.082m.75-.082a24.301 24.301 0 014.5 0m0 0v5.714c0 .597.237 1.17.659 1.591L19.8 15.3M14.25 3.104c.251.023.501.05.75.082M19.8 15.3l-1.57.393A9.065 9.065 0 0112 15a9.065 9.065 0 00-6.23.693L5 14.5m14.8.8l1.402 1.402c1.232 1.232.65 3.318-1.067 3.611A48.309 48.309 0 0112 21c-2.773 0-5.491-.235-8.135-.687-1.718-.293-2.3-2.379-1.067-3.61L5 14.5" />
              </svg>
            </div>
            <p className="text-base font-medium text-[#636366] tracking-tight">{t('chat.welcome')}</p>
            <p className="text-xs text-[#48484a] mt-1.5 max-w-xs text-center leading-relaxed">{t('chat.welcomeHint')}</p>
          </div>
        )}
        {allMessages.map((msg) => (
          <ChatMessage key={msg.id} message={msg}
            graphRef={(el) => { if (el) inlineRefs.current[msg.id] = el; }}
            onMaximizeRef={(graphName) => handleMaximize(msg.id, graphName)}
          />
        ))}
        <div ref={bottomRef} />
      </div>

      {/* Maximized overlay */}
      {maximized && (
        <div className="fixed inset-0 z-[100] bg-[#1a1a1e] flex flex-col">
          <div className="flex items-center justify-between px-5 py-3 border-b border-[#2a2a2e] bg-[#1c1c20] flex-shrink-0">
            <span className="text-sm font-semibold text-[#e5e5e7] tracking-tight">{t('chat.searchResult')}</span>
            <button className="w-7 h-7 rounded-lg bg-[#2a2a2e] hover:bg-[#3a3a3e] flex items-center justify-center text-[#636366] hover:text-white transition-all" onClick={handleRestore} title={t('panel.close')}>
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>
          <div className="flex-1 relative">
            <GraphViewer ref={fullscreenRef}
              data={{ success: true, data: [
                ...(maximized.nodes || []).map((n) => n._original || { type: 'vertex', id: n.id, labels: [], properties: { name: n.label } }),
                ...(maximized.edges || []).map((e) => e._original || { type: 'edge', id: e.id, source: e.from, target: e.to, label: e.label || '', properties: {} }),
              ]}}
              graph={maximized.graphName}
            />
          </div>
        </div>
      )}
    </div>
  );
}
