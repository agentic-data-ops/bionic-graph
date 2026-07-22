import { useEffect, useRef, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import GraphViewer from './GraphViewer';

function SimpleMarkdown({ text }) {
  if (!text) return null;
  const escaped = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  const html = escaped
    .replace(/\*\*(.+?)\*\*/g, '<strong class="font-semibold">$1</strong>')
    .replace(/`(.+?)`/g, '<code class="bg-[var(--bg-tertiary)] px-1.5 py-0.5 rounded-md text-xs font-mono text-[#ff9f0a]">$1</code>')
    .replace(/\n/g, '<br/>');
  return <span dangerouslySetInnerHTML={{ __html: html }} />;
}

function SearchStep({ step }) {
  const icon = step.status === 'done' ? (step.icon || '✅')
    : step.status === 'running' ? '⏳'
    : step.icon || '⏸';
  const textColor = step.status === 'done' ? 'text-[var(--success)]'
    : step.status === 'running' ? 'text-[var(--accent)]'
    : step.status === 'failed' ? 'text-[var(--danger)]'
    : 'text-[var(--text-tertiary)]';
  return (
    <div className="py-1.5">
      <div className="flex items-center gap-2">
        <span className="text-xs">{icon}</span>
        <span className={`text-xs ${textColor} font-medium tracking-tight`}>{step.name}</span>
        {step.status === 'running' && (
          <span className="inline-flex gap-0.5 ml-1">
            <span className="w-1 h-1 rounded-full bg-[var(--accent)] pulse-dot" />
            <span className="w-1 h-1 rounded-full bg-[var(--accent)] pulse-dot" style={{ animationDelay: '0.2s' }} />
            <span className="w-1 h-1 rounded-full bg-[var(--accent)] pulse-dot" style={{ animationDelay: '0.4s' }} />
          </span>
        )}
      </div>
      {step.llmOutput && step.status === 'done' && (
        <div className="mt-1.5 ml-5 text-[11px] text-[var(--text-tertiary)] leading-relaxed font-mono whitespace-pre-wrap border-l border-[var(--border)] pl-3">{step.llmOutput}</div>
      )}
      {step.llmOutput && step.status === 'running' && (
        <div className="mt-1.5 ml-5 text-[11px] text-[var(--text-muted)] leading-relaxed font-mono whitespace-pre-wrap border-l border-[var(--border)] pl-3 max-h-20 overflow-y-auto">{step.llmOutput}</div>
      )}
    </div>
  );
}

function CopyButton({ text }) {
  const [copied, setCopied] = useState(false);
  const timerRef = useRef(null);

  const handleCopy = async () => {
    try { await navigator.clipboard.writeText(text); } catch {}
    setCopied(true);
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => setCopied(false), 2000);
  };

  return (
    <button className="w-7 h-7 rounded-lg hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-all"
      onClick={handleCopy} title="复制">
      {copied ? (
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
          <path d="M13.3 4.3L6 11.6L2.7 8.3" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" fill="none"/>
        </svg>
      ) : (
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
          <path d="M6.14929 4.02032C7.11197 4.02032 7.87983 4.02016 8.49597 4.07598C9.12128 4.13269 9.65792 4.25188 10.1415 4.53106C10.7202 4.8653 11.2008 5.3459 11.535 5.92462C11.8142 6.40818 11.9334 6.94481 11.9901 7.57012C12.0459 8.18625 12.0458 8.95419 12.0458 9.9168C12.0458 10.8795 12.0459 11.6473 11.9901 12.2635C11.9334 12.8888 11.8142 13.4254 11.535 13.909C11.2008 14.4877 10.7202 14.9683 10.1415 15.3025C9.65792 15.5817 9.12128 15.7009 8.49597 15.7576C7.87984 15.8134 7.11196 15.8133 6.14929 15.8133C5.18667 15.8133 4.41874 15.8134 3.80261 15.7576C3.1773 15.7009 2.64067 15.5817 2.1571 15.3025C1.5784 14.9683 1.09778 14.4877 0.76355 13.909C0.484366 13.4254 0.365184 12.8888 0.308472 12.2635C0.252649 11.6473 0.252808 10.8795 0.252808 9.9168C0.252808 8.95418 0.252664 8.18625 0.308472 7.57012C0.365184 6.94481 0.484366 6.40818 0.76355 5.92462C1.09777 5.34589 1.57839 4.86529 2.1571 4.53106C2.64067 4.25188 3.1773 4.13269 3.80261 4.07598C4.41874 4.02017 5.18666 4.02032 6.14929 4.02032ZM6.14929 5.37774C5.16181 5.37774 4.46634 5.37761 3.92566 5.42657C3.39434 5.47472 3.07859 5.56574 2.83582 5.70587C2.4632 5.92106 2.15354 6.2307 1.93835 6.60333C1.79823 6.8461 1.70721 7.16185 1.65906 7.69317C1.6101 8.23385 1.61023 8.92933 1.61023 9.9168C1.61023 10.9043 1.61009 11.5998 1.65906 12.1404C1.70721 12.6717 1.79823 12.9875 1.93835 13.2303C2.15356 13.6029 2.46321 13.9126 2.83582 14.1277C3.07859 14.2679 3.39434 14.3589 3.92566 14.407C4.46634 14.456 5.16182 14.4559 6.14929 14.4559C7.13682 14.4559 7.83224 14.456 8.37292 14.407C8.90425 14.3589 9.21999 14.2679 9.46277 14.1277C9.83535 13.9126 10.145 13.6029 10.3602 13.2303C10.5004 12.9875 10.5914 12.6717 10.6395 12.1404C10.6885 11.5998 10.6884 10.9043 10.6884 9.9168C10.6884 8.92934 10.6885 8.23384 10.6395 7.69317C10.5914 7.16185 10.5004 6.8461 10.3602 6.60333C10.1451 6.23071 9.83536 5.92107 9.46277 5.70587C9.21999 5.56574 8.90424 5.47472 8.37292 5.42657C7.83224 5.3776 7.13682 5.37774 6.14929 5.37774ZM9.80164 0.367975C10.7638 0.367975 11.5314 0.36788 12.1473 0.423639C12.7726 0.480307 13.3093 0.598759 13.7928 0.877741C14.3717 1.21192 14.8521 1.69355 15.1864 2.27227C15.4655 2.75574 15.5857 3.29164 15.6425 3.9168C15.6983 4.53301 15.6971 5.3016 15.6971 6.26446V7.82989C15.6971 8.29264 15.6989 8.58993 15.6649 8.84844C15.4668 10.3525 14.401 11.5738 12.9833 11.9988V10.5467C13.6973 10.1903 14.2105 9.49662 14.3192 8.67169C14.3387 8.52347 14.3407 8.3358 14.3407 7.82989V6.26446C14.3407 5.27706 14.3398 4.58149 14.2909 4.04083C14.2428 3.50968 14.1526 3.19372 14.0126 2.95098C13.7974 2.57849 13.4876 2.26869 13.1151 2.05352C12.8724 1.91347 12.5564 1.82237 12.0253 1.77423C11.4847 1.72528 10.7888 1.7254 9.80164 1.7254H7.71472C6.7562 1.72558 5.92665 2.27697 5.52332 3.07891H4.07019C4.54221 1.51132 5.9932 0.368186 7.71472 0.367975H9.80164Z" fill="currentColor"/>
        </svg>
      )}
    </button>
  );
}

function ChatMessage({ message, graphRef, onMaximizeRef, theme, onEdit, onSaveToKB, onDataChange }) {
  const { t } = useTranslation();

  if (message.type === 'user') {
    return (
      <div className="flex justify-end mb-3 message-enter">
        <div className="max-w-[72%]">
          <div className="bg-[var(--accent)] text-white rounded-2xl rounded-br-md px-4 py-2.5 text-sm leading-relaxed shadow-sm select-text">{message.content}</div>
          <div className="flex justify-end gap-0.5 mt-0.5 pr-1">
            <CopyButton text={message.content} />
            <button className="w-7 h-7 rounded-lg hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-all"
              onClick={() => onEdit?.(message.content)} title="修改">
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
                <path d="M11.5 1.5L14.5 4.5L5.5 13.5L1.5 14.5L2.5 10.5L11.5 1.5Z" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" fill="none"/>
                <path d="M9.5 3.5L12.5 6.5" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" fill="none"/>
              </svg>
            </button>
          </div>
        </div>
      </div>
    );
  }
  if (message.type === 'assistant') {
    const hasContent = message.content?.length > 0;
    return (
      <div className="flex justify-start mb-3 message-enter">
        <div className="max-w-[90%]">
          <div className="bg-[var(--bg-tertiary)] text-[var(--text-primary)] rounded-2xl rounded-bl-md px-4 py-2.5 text-sm leading-relaxed shadow-sm select-text">
            {hasContent ? <SimpleMarkdown text={message.content} /> : (
              <span className="text-[var(--accent)] text-sm italic thinking-text">Thinking...</span>
            )}
          </div>
          {hasContent && (
            <div className="flex gap-0.5 mt-0.5 pl-1">
              <CopyButton text={message.content} />
              <button className="w-7 h-7 rounded-lg hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-all"
                onClick={() => onSaveToKB?.(message.content)} title="保存到知识库">
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
                  <path d="M8 1V10M8 10L4 6M8 10L12 6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                  <path d="M1 11V13.5C1 14.3284 1.67157 15 2.5 15H13.5C14.3284 15 15 14.3284 15 13.5V11" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                </svg>
              </button>
            </div>
          )}
        </div>
      </div>
    );
  }
  if (message.type === 'search_progress') {
    return (
      <div className="flex justify-start mb-3 message-enter">
        <div className="w-full max-w-[90%] bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl overflow-hidden shadow-sm">
          <div className="px-4 py-3.5">
            <div className="text-xs text-[var(--accent)] font-semibold mb-2 tracking-tight">🔎 Graph Search · <span className="text-[var(--text-tertiary)] font-normal">{message.title}</span></div>
            <div className="space-y-0">{(message.steps || []).map((step, i) => <SearchStep key={i} step={step} />)}</div>
          </div>
          {message.graphData && (
            <div className="border-t border-[var(--border)]">
              <div className="px-4 py-2 bg-[var(--bg-secondary)] border-b border-[var(--border)] flex items-center gap-2">
                <span className="text-xs font-semibold text-[var(--text-primary)] tracking-tight">{t('chat.searchResult')}</span>
                <span className="text-xs text-[var(--text-tertiary)] ml-auto font-medium">{message.graphData?.data?.length || 0} <span className="text-[var(--text-muted)]">items</span></span>
                <button className="w-6 h-6 rounded-md bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-all flex-shrink-0 ml-2" onClick={() => onMaximizeRef?.(message.graphName)} title={t('chat.maximize')}>
                  <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
                  </svg>
                </button>
              </div>
               <div className="h-[420px] relative"><GraphViewer ref={graphRef} data={message.graphData} graph={message.graphName} theme={theme} timeTravelEnabled={message.timeTravelEnabled || false} timeTravelAt={message.timeTravelAt} onDataChange={onDataChange} /></div>
            </div>
          )}
        </div>
      </div>
    );
  }
  if (message.type === 'graph_result') {
    return (
      <div className="flex justify-start mb-3 message-enter">
        <div className="w-full max-w-[90%] bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl overflow-hidden shadow-sm">
          <div className="px-4 py-2.5 bg-[var(--bg-secondary)] border-b border-[var(--border)] flex items-center gap-2">
            <span className="text-xs font-semibold text-[var(--text-primary)] tracking-tight">{t('chat.searchResult')}</span>
            <span className="text-xs text-[var(--text-tertiary)] ml-auto font-medium">{message.data?.data?.length || 0} <span className="text-[var(--text-muted)]">items</span></span>
            <button className="w-6 h-6 rounded-md bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-all flex-shrink-0 ml-2" onClick={() => onMaximizeRef?.(message.graphName)} title={t('chat.maximize')}>
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
              </svg>
            </button>
          </div>
          <div className="h-[420px] relative"><GraphViewer ref={graphRef} data={message.data} graph={message.graphName} theme={theme} timeTravelEnabled={message.timeTravelEnabled || false} timeTravelAt={message.timeTravelAt} onDataChange={onDataChange} /></div>
        </div>
      </div>
    );
  }
  if (message.type === 'web_search_progress') {
    return (
      <div className="flex justify-start mb-3 message-enter">
        <div className="w-full max-w-[90%] bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl overflow-hidden shadow-sm">
          <div className="px-4 py-3.5">
            <div className="text-xs text-[var(--accent)] font-semibold mb-2 tracking-tight">🌐 Web Search · <span className="text-[var(--text-tertiary)] font-normal">{message.title}</span></div>
            <div className="space-y-0">{(message.steps || []).map((step, i) => <SearchStep key={i} step={step} />)}</div>
            {message.webDetail && (
              <div className="mt-2 text-xs text-[var(--text-tertiary)] leading-relaxed border-t border-[var(--border)] pt-2">
                <div className="font-medium text-[var(--text-primary)] mb-1">Selected results:</div>
                <div className="space-y-0.5">{message.webDetail.split('\n').map((line, i) => (
                  <div key={i} className="truncate">{line}</div>
                ))}</div>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }
  return null;
}

export default function MessageList({ messages, searchStream, theme, onEdit, onSaveToKB, onDataChange }) {
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
    id: '__search_progress__',
    type: searchStream.type || 'search_progress',
    title: searchStream.title || searchStream.query || 'Graph Search',
    steps: searchStream.steps || [], graphData: searchStream.graphData, graphName: searchStream.graphName,
    timeTravelEnabled: searchStream.timeTravelEnabled || false, timeTravelAt: searchStream.timeTravelAt,
    webResults: searchStream.webResults, webDetail: searchStream.webDetail,
  }] : messages;

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="flex-1 overflow-y-auto px-4 py-4 min-h-0 relative scroll-smooth">
        {allMessages.length === 0 && (
          <div className="h-full flex flex-col items-center justify-center text-[var(--text-muted)]">
            <div className="w-14 h-14 rounded-2xl bg-[var(--bg-tertiary)] flex items-center justify-center mb-5 shadow-sm">
              <svg className="w-7 h-7 text-[var(--text-tertiary)]" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M9.75 3.104v5.714a2.25 2.25 0 01-.659 1.591L5 14.5M9.75 3.104c-.251.023-.501.05-.75.082m.75-.082a24.301 24.301 0 014.5 0m0 0v5.714c0 .597.237 1.17.659 1.591L19.8 15.3M14.25 3.104c.251.023.501.05.75.082M19.8 15.3l-1.57.393A9.065 9.065 0 0112 15a9.065 9.065 0 00-6.23.693L5 14.5m14.8.8l1.402 1.402c1.232 1.232.65 3.318-1.067 3.611A48.309 48.309 0 0112 21c-2.773 0-5.491-.235-8.135-.687-1.718-.293-2.3-2.379-1.067-3.61L5 14.5" />
              </svg>
            </div>
            <p className="text-base font-medium text-[var(--text-tertiary)] tracking-tight">{t('chat.welcome')}</p>
            <p className="text-xs text-[var(--text-muted)] mt-1.5 max-w-xs text-center leading-relaxed">{t('chat.welcomeHint')}</p>
          </div>
        )}
        {allMessages.map((msg) => (
          <ChatMessage key={msg.id} message={msg} theme={theme}
            graphRef={(el) => { if (el) inlineRefs.current[msg.id] = el; }}
            onMaximizeRef={(graphName) => handleMaximize(msg.id, graphName)}
            onEdit={onEdit} onSaveToKB={onSaveToKB} onDataChange={onDataChange}
          />
        ))}
        <div ref={bottomRef} />
      </div>

      {/* Maximized overlay */}
      {maximized && (
        <div className="fixed inset-0 z-[100] bg-[var(--bg-primary)] flex flex-col">
          <div className="flex items-center justify-between px-5 py-3 border-b border-[var(--border)] bg-[var(--bg-secondary)] flex-shrink-0">
            <span className="text-sm font-semibold text-[var(--text-primary)] tracking-tight">{t('chat.searchResult')}</span>
            <button className="w-7 h-7 rounded-lg bg-[var(--bg-tertiary)] hover:bg-[var(--bg-hover)] flex items-center justify-center text-[var(--text-tertiary)] hover:text-[var(--text-primary)] transition-all" onClick={handleRestore} title={t('panel.close')}>
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>
          <div className="flex-1 relative">
            <GraphViewer ref={fullscreenRef} theme={theme}
              data={{ success: true, data: [
                ...(maximized.nodes || []).map((n) => n._original || { type: 'vertex', id: n.id, labels: [], properties: { name: n.label } }),
                ...(maximized.edges || []).map((e) => e._original || { type: 'edge', id: e.id, source: e.from, target: e.to, label: e.label || '', properties: {} }),
              ]}}
              graph={maximized.graphName}
              timeTravelEnabled={maximized.timeTravelEnabled || false}
              timeTravelAt={maximized.timeTravelAt}
              onDataChange={onDataChange}
            />
          </div>
        </div>
      )}
    </div>
  );
}
