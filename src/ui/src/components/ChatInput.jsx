import { useState, useRef, useCallback, useEffect, forwardRef, useImperativeHandle } from 'react';
import { useTranslation } from 'react-i18next';

const ChatInput = forwardRef(function ChatInput({
  providers,
  activeProvider,
  onProviderChange,
  useGraph,
  onGraphToggle,
  searchMode,
  onSearchModeChange,
  kwSearchMode,
  onkwSearchModeChange,
  timeTravel,
  onTimeTravelToggle,
  graphName,
  onGraphNameChange,
  graphs,
  chatModel,
  onChatModelChange,
  onSend,
  disabled,
}, ref) {
  const { t } = useTranslation();
  const [text, setText] = useState('');
  const textareaRef = useRef(null);
  useImperativeHandle(ref, () => ({ focus: () => textareaRef.current?.focus() }), []);

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
      <div className="flex items-center gap-2 mb-2.5 flex-wrap">
        {/* Model selector — leftmost */}
        {providers.length > 0 && activeProvider && (() => {
          const activeProv = providers.find(p => p.id === activeProvider);
          if (!activeProv) return null;
          const effectiveModel = chatModel || activeProv.defaultModel || activeProv.model;
          const defaultModel = activeProv.defaultModel || activeProv.model;
          const currentKey = `${activeProv.name}/${effectiveModel}`;
          const options = [];
          providers.forEach(p => {
            const models = p.models || [p.model];
            models.forEach(m => {
              options.push({ key: `${p.name}/${m}`, providerId: p.id, model: m, isDefault: p.id === activeProvider && m === defaultModel });
            });
          });
          return (
            <select
              className="bg-[var(--bg-tertiary)] text-[var(--text-primary)] rounded-lg px-2.5 py-1 text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] cursor-pointer appearance-none flex-shrink-0"
              style={{ maxWidth: '220px', backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='8' height='5' viewBox='0 0 8 5'%3E%3Cpath fill='%2386868b' d='M0 0l4 5 4-5z'/%3E%3C/svg%3E\")", backgroundRepeat: 'no-repeat', backgroundPosition: 'right 6px center', paddingRight: '22px' }}
              value={currentKey}
              onChange={(e) => {
                const selected = options.find(o => o.key === e.target.value);
                if (!selected) return;
                if (selected.providerId !== activeProvider) {
                  onProviderChange(selected.providerId);
                }
                onChatModelChange(selected.isDefault ? null : selected.model);
              }}
            >
              {options.map((opt) => (
                <option key={opt.key} value={opt.key}>
                  {opt.key}{opt.isDefault ? ' (default)' : ''}
                </option>
              ))}
            </select>
          );
        })()}

        {/* Graph toggle */}
        <label className="flex items-center gap-1.5 cursor-pointer select-none flex-shrink-0" onClick={(e) => { e.preventDefault(); onGraphToggle(!useGraph); }}>
          <div className={`relative w-8 h-4.5 rounded-full transition-all duration-200 ${useGraph ? 'bg-[var(--accent)]' : 'bg-[var(--bg-hover)]'}`} style={{ height: '18px', width: '32px' }}>
            <div className={`absolute top-0.5 w-3.5 h-3.5 rounded-full bg-white shadow-sm transition-all duration-200 ${useGraph ? 'left-[14px]' : 'left-[1px]'}`} style={{ width: '14px', height: '14px', top: '2px' }} />
          </div>
          <span className="text-xs text-[var(--text-secondary)] font-medium">{t('chat.useGraph')}</span>
        </label>

        {useGraph && (
          <>
            <select
              className="bg-[var(--bg-tertiary)] text-[var(--text-primary)] rounded-lg px-2.5 py-1 text-xs border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] cursor-pointer appearance-none"
              style={{ backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='8' height='5' viewBox='0 0 8 5'%3E%3Cpath fill='%2386868b' d='M0 0l4 5 4-5z'/%3E%3C/svg%3E\")", backgroundRepeat: 'no-repeat', backgroundPosition: 'right 6px center', paddingRight: '22px' }}
              value={graphName}
              onChange={(e) => onGraphNameChange(e.target.value)}
            >
              {graphs.map((g) => <option key={g} value={g}>{g}</option>)}
            </select>
            <label className="flex items-center gap-1.5 cursor-pointer select-none text-xs text-[var(--text-secondary)] font-medium whitespace-nowrap">
              <input type="checkbox" checked={timeTravel} onChange={(e) => onTimeTravelToggle(e.target.checked)}
                className="w-3.5 h-3.5 rounded border-[#3a3a3e] bg-[var(--bg-tertiary)] checked:bg-[var(--accent)] checked:border-[#0a84ff] focus:ring-0 cursor-pointer" />
              {t('chat.timeTravel')}
            </label>
            <div className="flex rounded-lg overflow-hidden ring-1 ring-[var(--bg-hover)]">
              <button
                className={`px-2.5 py-1 text-[11px] font-medium transition-all ${searchMode === 'keyword' ? 'bg-[var(--accent)] text-white' : 'bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]'}`}
                onClick={() => onSearchModeChange('keyword')}
              >{t('chat.keyword')}</button>
              <button
                className={`px-2.5 py-1 text-[11px] font-medium transition-all ${searchMode === 'semantic' ? 'bg-[var(--accent)] text-white' : 'bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]'}`}
                onClick={() => onSearchModeChange('semantic')}
              >{t('chat.semantic')}</button>
            </div>
            {searchMode === 'keyword' && (
              <button
                className={`px-2 py-1 text-[10px] font-mono rounded-lg transition-all ${kwSearchMode === 'exact' ? 'bg-[var(--accent)] text-white' : 'bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]'}`}
                onClick={() => onkwSearchModeChange(kwSearchMode === 'exact' ? 'greedy' : 'exact')}
                title={t('chat.kwModeHint')}
              >{kwSearchMode === 'exact' ? 'ALL' : 'ANY'}</button>
            )}
          </>
        )}

      </div>
      {/* Input area */}
      <div className="flex items-end gap-2">
        <textarea
          ref={textareaRef}
          className="flex-1 resize-none bg-[var(--bg-tertiary)] text-[var(--text-primary)] rounded-2xl px-4 py-2.5 text-sm border-0 outline-none ring-1 ring-[var(--bg-hover)] focus:ring-[var(--accent)] placeholder-[var(--text-tertiary)] max-h-[160px] leading-relaxed transition-all duration-200"
          placeholder={t('chat.inputPlaceholder')}
          rows={1}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={disabled}
        />

        <button
          className="flex-shrink-0 p-2.5 rounded-xl transition-all duration-200 disabled:opacity-30 disabled:cursor-not-allowed"
          style={{
            backgroundColor: text.trim() && !disabled ? '#0a84ff' : '#2a2a2e',
            color: text.trim() && !disabled ? 'white' : '#636366',
          }}
          onClick={handleSend}
          disabled={!text.trim() || disabled}
        >
          <svg className="w-4.5 h-4.5" style={{ width: '18px', height: '18px' }} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M12 19V5m0 0l-7 7m7-7l7 7" />
          </svg>
        </button>
      </div>

      {!useGraph && providers.length === 0 && (
        <div className="text-[11px] text-[#ff9f0a] mt-2 text-center tracking-tight">{t('chat.noProvider')}</div>
      )}
    </div>
  );
});

export default ChatInput;
