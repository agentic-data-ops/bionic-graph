import { useState, useRef, useCallback, useEffect, forwardRef, useImperativeHandle } from 'react';
import { useTranslation } from 'react-i18next';
import { fetchModels } from '../api';

// ── Model selector component (extracted from IIFE to comply with Rules of Hooks) ──
function ChatModelSelector({
  providers,
  defaultModelKey,
  activeProvider,
  chatModel,
  onProviderChange,
  onChatModelChange,
}) {
  const { t } = useTranslation();
  const [modelList, setModelList] = useState(null);
  const [defaultModel, setDefaultModel] = useState('');
  const [fetching, setFetching] = useState(false);
  const [modelOpen, setModelOpen] = useState(false);
  const initialised = useRef(false);

  // Fetch model list + default model from backend.
  // Re-fetches when providers or defaultModelKey change (settings modified).
  useEffect(() => {
    if (!fetching) {
      setFetching(true);
      fetchModels().then(({ models, defaultModel: dm }) => {
        const list = models?.data || [];
        setModelList(list);
        setDefaultModel(dm || '');

        // On first load only, determine the initial model key:
        // 1. Try localStorage saved model
        // 2. If missing or not in the list, use backend defaultModel
        if (!initialised.current) {
          initialised.current = true;
          const saved = localStorage.getItem('bgraph-last-model');
          const validSaved = saved && list.some(e => e.id === saved);
          const targetKey = validSaved ? saved : (dm || list[0]?.id || '');
          if (targetKey) {
            const parts = targetKey.split('/');
            if (parts.length >= 2) {
              onProviderChange(parts[0]);
              onChatModelChange(parts.slice(1).join('/'));
            }
          }
        }
      }).catch(() => {}).finally(() => setFetching(false));
    }
  }, [providers, defaultModelKey]);

  if (!modelList) {
    return <span className="text-xs text-[var(--text-tertiary)]">加载中...</span>;
  }

  const currentModel = chatModel || '';
  const currentProvider = activeProvider || '';
  const currentKey = currentProvider && currentModel ? `${currentProvider}/${currentModel}` : '';

  const options = modelList.map(entry => ({
    key: entry.id,
    providerName: entry.owned_by,
    model: entry.id.includes('/') ? entry.id.split('/').slice(1).join('/') : entry.id,
    isDefault: entry.id === defaultModel,
  }));

  return (
    <div className="relative">
      <button
        className="px-2 py-1 text-xs rounded-lg bg-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] transition-all font-medium flex items-center gap-1 flex-shrink-0"
        onClick={(e) => { e.stopPropagation(); setModelOpen(!modelOpen); }}
        style={{ maxWidth: '220px' }}
      >
        <span className="truncate max-w-[160px]">{currentKey || t('chat.selectModel')}</span>
        <svg className={`w-2.5 h-2.5 flex-shrink-0 transition-transform ${modelOpen ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}><path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" /></svg>
      </button>
      {modelOpen && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setModelOpen(false)} />
          <div className="absolute left-0 bottom-full mb-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden min-w-full w-max max-h-[300px] overflow-y-auto max-w-[260px]">
            {options.map((opt) => (
              <button
                key={opt.key}
                className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap truncate transition-all ${opt.key === currentKey ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                onClick={() => {
                  localStorage.setItem('bgraph-last-model', opt.key);
                  onProviderChange(opt.providerName);
                  onChatModelChange(opt.model);
                  setModelOpen(false);
                }}
              >
                <span>{opt.key}</span>
                {opt.isDefault && <span className="ml-1.5 text-[var(--text-muted)]">({t('chat.default')})</span>}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

const ChatInput = forwardRef(function ChatInput({
  providers,
  activeProvider,
  onProviderChange,
  useGraph,
  onGraphToggle,
  useWebSearch,
  onWebSearchToggle,
  extractKeywords,
  onExtractKeywordsToggle,
  kwSearchMode,
  onkwSearchModeChange,
  enableSemanticFilter,
  onSemanticFilterChange,
  timeTravel,
  onTimeTravelToggle,
  timeTravelPoint,
  onTimeTravelPointChange,
  graphName,
  onGraphNameChange,
  graphs,
  timeTravelGraphs,
  graphMetas,
  defaultModelKey,
  chatModel,
  onChatModelChange,
  onSend,
  disabled,
  isGenerating,
  onStop,
}, ref) {
  const { t } = useTranslation();
  const [text, setText] = useState('');
  const [kwModeOpen, setKwModeOpen] = useState(false);
  const [graphOpen, setGraphOpen] = useState(false);
  const textareaRef = useRef(null);
  useImperativeHandle(ref, () => ({
    focus: () => textareaRef.current?.focus(),
    setText: (t) => setText(t),
  }), []);

  useEffect(() => {
    const ta = textareaRef.current;
    if (ta) {
      ta.style.height = 'auto';
      ta.style.height = Math.min(ta.scrollHeight, 160) + 'px';
    }
  }, [text]);

  const handleSend = useCallback(() => {
    if (!text.trim() || disabled) return;
    onSend(text.trim());
    setText('');
    if (textareaRef.current) textareaRef.current.style.height = 'auto';
  }, [text, disabled, onSend]);

  const handleKeyDown = useCallback((e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }, [handleSend]);

  return (
    <div className="bg-[var(--bg-secondary)] border-t border-[var(--border)] px-4 py-3">
      {/* Mode bar */}
      <div className="flex items-center gap-3 mb-2.5 flex-wrap">
        {/* Model selector — leftmost */}
        <ChatModelSelector
          providers={providers}
          defaultModelKey={defaultModelKey}
          activeProvider={activeProvider}
          chatModel={chatModel}
          onProviderChange={onProviderChange}
          onChatModelChange={onChatModelChange}
        />

        {/* Web search toggle */}
        <label className="flex items-center gap-1.5 cursor-pointer select-none flex-shrink-0" onClick={(e) => { e.preventDefault(); onWebSearchToggle(!useWebSearch); }}>
          <div className={`relative w-8 h-4.5 rounded-full transition-all duration-200 ${useWebSearch ? 'bg-[var(--accent)]' : 'bg-[var(--bg-hover)]'}`} style={{ height: '18px', width: '32px' }}>
            <div className={`absolute top-0.5 w-3.5 h-3.5 rounded-full bg-white shadow-sm transition-all duration-200 ${useWebSearch ? 'left-[14px]' : 'left-[1px]'}`} style={{ width: '14px', height: '14px', top: '2px' }} />
          </div>
          <span className="text-xs text-[var(--text-secondary)] font-medium">{t('chat.webSearch')}</span>
        </label>

        {/* Graph toggle */}
        <label className="flex items-center gap-1.5 cursor-pointer select-none flex-shrink-0" onClick={(e) => { e.preventDefault(); onGraphToggle(!useGraph); }}>
          <div className={`relative w-8 h-4.5 rounded-full transition-all duration-200 ${useGraph ? 'bg-[var(--accent)]' : 'bg-[var(--bg-hover)]'}`} style={{ height: '18px', width: '32px' }}>
            <div className={`absolute top-0.5 w-3.5 h-3.5 rounded-full bg-white shadow-sm transition-all duration-200 ${useGraph ? 'left-[14px]' : 'left-[1px]'}`} style={{ width: '14px', height: '14px', top: '2px' }} />
          </div>
          <span className="text-xs text-[var(--text-secondary)] font-medium">{t('chat.useGraph')}</span>
        </label>

        {useGraph && (
          <>
            {/* Graph selector */}
            <div className="relative">
              <button
                className="px-2 py-1 text-xs rounded-lg bg-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] transition-all font-medium flex items-center gap-1"
                onClick={(e) => { e.stopPropagation(); setGraphOpen(!graphOpen); }}
              >
                {graphName}
                <svg className={`w-2.5 h-2.5 transition-transform ${graphOpen ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}><path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" /></svg>
              </button>
              {graphOpen && (
                <>
                  <div className="fixed inset-0 z-40" onClick={() => setGraphOpen(false)} />
                  <div className="absolute right-0 bottom-full mb-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden min-w-full w-max max-w-[200px]">
                    {graphs.map((g) => (
                      <button
                        key={g}
                        className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap truncate transition-all ${g === graphName ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                        onClick={() => { onGraphNameChange(g); setGraphOpen(false); }}
                      >{g}</button>
                    ))}
                  </div>
                </>
              )}
            </div>

            {/* Match mode dropdown */}
            <div className="relative">
              <button
                className="px-2 py-1 text-xs rounded-lg bg-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)] border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] transition-all font-medium flex items-center gap-1"
                onClick={(e) => { e.stopPropagation(); setKwModeOpen(!kwModeOpen); }}
              >
                {kwSearchMode === 'exact' ? t('chat.exactSearch') : t('chat.greedySearch')}
                <svg className={`w-2.5 h-2.5 transition-transform ${kwModeOpen ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}><path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" /></svg>
              </button>
              {kwModeOpen && (
                <>
                  <div className="fixed inset-0 z-40" onClick={() => setKwModeOpen(false)} />
                  <div className="absolute left-0 bottom-full mb-1 z-50 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl shadow-lg overflow-hidden w-full">
                    <button className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap transition-all ${kwSearchMode === 'greedy' ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                      onClick={() => { onkwSearchModeChange('greedy'); setKwModeOpen(false); }}>{t('chat.greedySearch')}</button>
                    <button className={`w-full text-left px-2.5 py-2 text-xs font-medium whitespace-nowrap transition-all ${kwSearchMode === 'exact' ? 'text-[var(--accent)] bg-[var(--accent-bg)]' : 'text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'}`}
                      onClick={() => { onkwSearchModeChange('exact'); setKwModeOpen(false); }}>{t('chat.exactSearch')}</button>
                  </div>
                </>
              )}
            </div>

            {/* Time travel — only if graph supports it */}
            {(Array.isArray(graphMetas) ? graphMetas.find(g => g.name === graphName)?.time_travel : timeTravelGraphs[graphName]) && (<>
            <label className="flex items-center gap-1.5 cursor-pointer select-none text-xs text-[var(--text-secondary)] font-medium whitespace-nowrap">
              <input type="checkbox" checked={timeTravel} onChange={(e) => onTimeTravelToggle(e.target.checked)}
                className="w-3.5 h-3.5 rounded border-[var(--border)] bg-[var(--bg-tertiary)] checked:bg-[var(--accent)] checked:border-[var(--accent)] focus:ring-0 cursor-pointer" />
              {t('chat.timeTravel')}
            </label>
            {timeTravel && (
              <input type="datetime-local"
                className="bg-transparent text-[var(--text-primary)] rounded-lg px-2 py-1 text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)]"
                value={timeTravelPoint || ''}
                onChange={(e) => onTimeTravelPointChange?.(e.target.value)}
              />
            )}
            </>)}
          </>
        )}

        {/* Extract keywords toggle — shown when any search is active */}
        {(useWebSearch || useGraph) && (
          <label className="flex items-center gap-1.5 cursor-pointer select-none flex-shrink-0" onClick={(e) => { e.preventDefault(); onExtractKeywordsToggle(!extractKeywords); }}>
            <div className={`relative w-8 h-4.5 rounded-full transition-all duration-200 ${extractKeywords ? 'bg-[var(--accent)]' : 'bg-[var(--bg-hover)]'}`} style={{ height: '18px', width: '32px' }}>
              <div className={`absolute top-0.5 w-3.5 h-3.5 rounded-full bg-white shadow-sm transition-all duration-200 ${extractKeywords ? 'left-[14px]' : 'left-[1px]'}`} style={{ width: '14px', height: '14px', top: '2px' }} />
            </div>
            <span className="text-xs text-[var(--text-secondary)] font-medium">提取关键字</span>
          </label>
        )}

      </div>

      {/* Input area */}
      <div className="flex items-end gap-2">
        <textarea
          ref={textareaRef}
          className="flex-1 resize-none bg-transparent text-[var(--text-primary)] rounded-2xl px-4 py-2.5 text-sm border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] placeholder-[var(--text-tertiary)] max-h-[160px] leading-relaxed transition-all duration-200"
          placeholder={t('chat.inputPlaceholder')}
          rows={1}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={disabled}
        />

        {isGenerating ? (
          <button
            className="flex-shrink-0 p-2.5 rounded-xl transition-all duration-200 bg-red-500 text-white hover:bg-red-600"
            onClick={() => { onStop?.(); textareaRef.current?.focus(); }}
          >
            <svg className="w-4.5 h-4.5" style={{ width: '18px', height: '18px' }} fill="currentColor" viewBox="0 0 24 24">
              <rect x="6" y="6" width="12" height="12" rx="2" />
            </svg>
          </button>
        ) : (
          <button
            className="flex-shrink-0 p-2.5 rounded-xl transition-all duration-200 disabled:opacity-30 disabled:cursor-not-allowed"
            style={{
              backgroundColor: text.trim() && !disabled ? 'var(--accent)' : 'var(--bg-hover)',
              color: text.trim() && !disabled ? 'white' : 'var(--text-tertiary)',
            }}
            onClick={handleSend}
            disabled={!text.trim() || disabled}
          >
            <svg className="w-4.5 h-4.5" style={{ width: '18px', height: '18px' }} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 19V5m0 0l-7 7m7-7l7 7" />
            </svg>
          </button>
        )}
      </div>

      {!useGraph && providers.length === 0 && (
        <div className="text-[11px] text-[#ff9f0a] mt-2 text-center tracking-tight">{t('chat.noProvider')}</div>
      )}
    </div>
  );
});

export default ChatInput;