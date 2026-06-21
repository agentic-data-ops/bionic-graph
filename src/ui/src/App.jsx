import { useState, useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import Sidebar from './components/Sidebar';
import ChatArea from './components/ChatArea';
import SettingsDialog from './components/SettingsDialog';
import KnowledgeBase from './components/KnowledgeBase';
import { listGraphs } from './api';

// ── Helpers ──────────────────────────────────────────────────────

let _convCounter = 0;
function newConvId() {
  return `conv-${Date.now()}-${++_convCounter}`;
}

const DEFAULT_SETTINGS = {
  providers: [
    {
      id: 'default-deepseek',
      name: 'DeepSeek',
      apiBase: 'https://api.deepseek.com/v1',
      model: 'deepseek-v4-flash',
      apiKey: '',
    },
  ],
  activeProvider: 'default-deepseek',
  defaultGraph: 'default',
  timeTravel: false,
  useGraph: false,
  searchMode: 'semantic',
};

function loadSettings() {
  try {
    const raw = localStorage.getItem('bgraph-settings');
    if (raw) {
      const parsed = JSON.parse(raw);
      return { ...DEFAULT_SETTINGS, ...parsed };
    }
  } catch {}
  return { ...DEFAULT_SETTINGS };
}

function saveSettings(settings) {
  localStorage.setItem('bgraph-settings', JSON.stringify(settings));
}

function loadConversations() {
  try {
    const raw = localStorage.getItem('bgraph-convs');
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed) && parsed.length > 0) return parsed;
    }
  } catch {}
  return [];
}

function saveConversations(convs) {
  localStorage.setItem('bgraph-convs', JSON.stringify(convs));
}

export default function App() {
  const { t, i18n } = useTranslation();

  // ── Settings (persisted) ──
  const [settings, setSettings] = useState(loadSettings);

  // ── Conversations (persisted) ──
  const [conversations, setConversations] = useState(() => {
    const convs = loadConversations();
    return convs;
  });
  const [activeConvId, setActiveConvId] = useState(
    () => conversations[0]?.id || null
  );

  // ── UI state ──
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [knowledgeBaseOpen, setKnowledgeBaseOpen] = useState(false);
  const [graphs, setGraphs] = useState([]);

  // ── Theme ──
  const [theme, setTheme] = useState(() => localStorage.getItem('theme') || 'dark');

  useEffect(() => {
    document.documentElement.classList.toggle('dark', theme === 'dark');
    localStorage.setItem('theme', theme);
  }, [theme]);

  // ── Load graph list ──
  useEffect(() => {
    listGraphs()
      .then((d) => {
        const gs = d.graphs || [];
        setGraphs(gs);
        // Ensure default graph exists
        if (!gs.includes(settings.defaultGraph)) {
          setSettings((s) => ({ ...s, defaultGraph: gs[0] || 'default' }));
        }
      })
      .catch(() => {});
  }, []);

  // ── Persist settings on change ──
  useEffect(() => {
    saveSettings(settings);
  }, [settings]);

  // ── Persist conversations on change ──
  useEffect(() => {
    saveConversations(conversations);
  }, [conversations]);

  // ── Derived: active conversation ──
  const activeConv = conversations.find((c) => c.id === activeConvId) || null;

  // ── Conversation actions ──
  const handleNewChat = useCallback(() => {
    const newConv = {
      id: newConvId(),
      title: t('chat.newChat'),
      messages: [],
      createdAt: Date.now(),
    };
    setConversations((prev) => [newConv, ...prev]);
    setActiveConvId(newConv.id);
  }, [t]);

  const handleSwitchConv = useCallback((id) => {
    setActiveConvId(id);
  }, []);

  const handleUpdateConv = useCallback((updated) => {
    setConversations((prev) =>
      prev.map((c) => (c.id === updated.id ? updated : c))
    );
    // Auto-title: use first user message
    if (
      !updated.title ||
      updated.title === t('chat.newChat') ||
      updated.title === t('chat.untitled')
    ) {
      const firstUserMsg = updated.messages.find((m) => m.type === 'user');
      if (firstUserMsg) {
        const title = firstUserMsg.content.slice(0, 40) + (firstUserMsg.content.length > 40 ? '…' : '');
        setConversations((prev) =>
          prev.map((c) =>
            c.id === updated.id ? { ...c, title } : c
          )
        );
      }
    }
  }, [t]);

  // ── Settings actions ──
  const handleUpdateSettings = useCallback((partial) => {
    setSettings((prev) => ({ ...prev, ...partial }));
  }, []);

  const handleUpdateProviders = useCallback((providers) => {
    setSettings((prev) => ({
      ...prev,
      providers,
      activeProvider: providers.length > 0 ? prev.activeProvider : null,
    }));
  }, []);

  const handleThemeToggle = useCallback(() => {
    setTheme((prev) => (prev === 'dark' ? 'light' : 'dark'));
  }, []);

  const handleLanguageToggle = useCallback(() => {
    const next = i18n.language === 'zh' ? 'en' : 'zh';
    i18n.changeLanguage(next);
  }, [i18n]);

  // Initialize first conversation if none
  useEffect(() => {
    if (conversations.length === 0) {
      handleNewChat();
    }
  }, [conversations.length, handleNewChat]);

  return (
    <div className={`h-screen flex overflow-hidden select-none ${
      theme === 'dark'
        ? 'bg-[#1a1a1e] text-[#e5e5e7]'
        : 'bg-[#f5f5f7] text-[#1d1d1f]'
    }`}>
      {/* Sidebar */}
      <Sidebar
        conversations={conversations}
        activeConvId={activeConvId}
        onNewChat={handleNewChat}
        onSwitchConv={handleSwitchConv}
        onOpenSettings={() => setSettingsOpen(true)}
        onOpenKnowledgeBase={() => setKnowledgeBaseOpen(true)}
      />

      {/* Main chat area */}
      <ChatArea
        activeConv={activeConv}
        onUpdateConv={handleUpdateConv}
        providers={settings.providers}
        activeProvider={settings.activeProvider}
        onProviderChange={(id) => handleUpdateSettings({ activeProvider: id })}
        useGraph={settings.useGraph}
        onGraphToggle={(v) => handleUpdateSettings({ useGraph: v })}
        searchMode={settings.searchMode}
        onSearchModeChange={(v) => handleUpdateSettings({ searchMode: v })}
        timeTravel={settings.timeTravel}
        onTimeTravelToggle={(v) => handleUpdateSettings({ timeTravel: v })}
        defaultGraph={settings.defaultGraph}
        onDefaultGraphChange={(g) => handleUpdateSettings({ defaultGraph: g })}
        graphs={graphs}
      />

      {/* Knowledge Base dialog */}
      <KnowledgeBase
        open={knowledgeBaseOpen}
        onClose={() => setKnowledgeBaseOpen(false)}
        providers={settings.providers}
        activeProvider={settings.activeProvider}
        defaultGraph={settings.defaultGraph}
        theme={theme}
      />

      {/* Settings dialog */}
      <SettingsDialog
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        providers={settings.providers}
        onUpdateProviders={handleUpdateProviders}
        graphName={settings.defaultGraph}
        onGraphNameChange={(g) => handleUpdateSettings({ defaultGraph: g })}
        graphs={graphs}
        onGraphsChange={setGraphs}
        theme={theme}
        onThemeToggle={handleThemeToggle}
        language={i18n.language}
        onLanguageToggle={handleLanguageToggle}
      />
    </div>
  );
}
