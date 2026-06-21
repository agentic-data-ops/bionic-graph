import { useTranslation } from 'react-i18next';

export default function Sidebar({
  conversations,
  activeConvId,
  onNewChat,
  onSwitchConv,
  onDeleteConv,
  onOpenKnowledgeBase,
  onOpenSettings,
}) {
  const { t } = useTranslation();

  return (
    <aside className="w-64 flex flex-col h-full bg-[#1c1c20] border-r border-[#2a2a2e] overflow-hidden">
      {/* Header */}
      <div className="px-4 pt-4 pb-2 flex items-center gap-2.5">
        <div className="w-7 h-7 rounded-lg bg-gradient-to-br from-[#0a84ff] to-[#5e5ce6] flex items-center justify-center text-white text-xs font-bold shadow-sm">
          BG
        </div>
        <span className="text-sm font-semibold text-[#e5e5e7] tracking-tight">Bionic-Graph</span>
      </div>

      {/* New chat button */}
      <div className="px-3 py-2">
        <button className="w-full py-2 px-3.5 rounded-xl bg-[#2a2a2e] text-[#98989d] hover:bg-[#3a3a3e] hover:text-white text-sm font-medium flex items-center justify-center gap-2 transition-all duration-200" onClick={onNewChat}>
          <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M12 4v16m8-8H4" />
          </svg>
          {t('chat.newChat')}
        </button>
      </div>

      {/* Conversation list */}
      <div className="flex-1 overflow-y-auto px-2 space-y-0.5">
        {conversations.map((conv) => (
          <div key={conv.id} className="group relative">
            <button
              className={`w-full text-left px-3 py-2.5 rounded-xl text-sm transition-all duration-200 ${
                conv.id === activeConvId
                  ? 'bg-[#3a3a3e] text-white shadow-sm'
                  : 'text-[#86868b] hover:bg-[#2a2a2e] hover:text-[#e5e5e7]'
              }`}
              onClick={() => onSwitchConv(conv.id)}
            >
              <div className="truncate font-medium text-[13px] leading-tight pr-6">
                {conv.title || t('chat.untitled')}
              </div>
              <div className="text-[11px] text-[#636366] mt-1 tracking-tight">
                {conv.messages.length} {t('chat.messages')}
              </div>
            </button>
            <button
              className="absolute right-1 top-1/2 -translate-y-1/2 w-5 h-5 rounded-md bg-[#3a3a3e] hover:bg-[#ff453a] flex items-center justify-center text-[#636366] hover:text-white opacity-0 group-hover:opacity-100 transition-all text-xs"
              onClick={(e) => { e.stopPropagation(); onDeleteConv(conv.id); }}
              title={t('settings.delete')}
            >
              <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>
        ))}
        {conversations.length === 0 && (
          <div className="text-center text-[#48484a] text-xs py-10 tracking-tight">{t('chat.noHistory')}</div>
        )}
      </div>

      {/* Bottom buttons */}
      <div className="border-t border-[#2a2a2e]">
        {/* Knowledge Base */}
        <div className="px-3 py-1.5">
          <button className="w-full py-2 px-3.5 rounded-xl text-[#86868b] hover:bg-[#2a2a2e] hover:text-white text-sm font-medium flex items-center gap-2.5 transition-all duration-200" onClick={onOpenKnowledgeBase}>
            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.8}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
            </svg>
            {t('knowledgeBase.title')}
          </button>
        </div>
        {/* Settings */}
        <div className="px-3 py-1.5">
          <button className="w-full py-2 px-3.5 rounded-xl text-[#86868b] hover:bg-[#2a2a2e] hover:text-white text-sm font-medium flex items-center gap-2.5 transition-all duration-200" onClick={onOpenSettings}>
            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.8}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
              <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
            </svg>
            {t('settings.title')}
          </button>
        </div>
      </div>
    </aside>
  );
}
