import { useState, useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import Sidebar from './components/Sidebar';
import ChatArea from './components/ChatArea';
import SettingsDialog from './components/SettingsDialog';
import KnowledgeBase from './components/KnowledgeBase';
import { listGraphs, fetchSettings, updateSettings } from './api';

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
      apiKey: '',
      models: ['deepseek-v4-flash', 'deepseek-v4-pro'],
      defaultModel: 'deepseek-v4-flash',
      model: 'deepseek-v4-flash',
    },
  ],
  activeProvider: 'default-deepseek',
  defaultGraph: 'default',
  timeTravel: false,
  useGraph: false,
  searchMode: 'semantic',
  chatModel: null,
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
    document.documentElement.classList.toggle('light', theme === 'light');
    localStorage.setItem('theme', theme);
  }, [theme]);

  // ── Load graph list & backend settings ──
  useEffect(() => {
    listGraphs()
      .then((d) => {
        const gs = d.graphs || [];
        setGraphs(gs);
        if (!gs.includes(settings.defaultGraph)) {
          setSettings((s) => ({ ...s, defaultGraph: gs[0] || 'default' }));
        }
      })
      .catch(() => {});

    // Load backend LLM providers to sync frontend defaults
    fetchSettings()
      .then((backend) => {
        const llm = backend?.llm;
        if (llm?.providers?.length > 0) {
          setSettings((prev) => {
            // Parse default_model "Provider/Model" format
            let activeIdx = 0;
            let defaultModel = llm.providers[0]?.models?.[0] || 'deepseek-v4-flash';
            if (llm.default_model && llm.default_model.includes('/')) {
              const [provName, modelName] = llm.default_model.split('/');
              const idx = llm.providers.findIndex((p) => p.name === provName);
              if (idx >= 0) { activeIdx = idx; defaultModel = modelName; }
            }
            const backendProviders = llm.providers.map((bp, i) => {
              const m = i === activeIdx ? defaultModel : (bp.models?.[0] || 'deepseek-v4-flash');
              return {
                id: `provider-${i}`,
                name: bp.name,
                apiBase: bp.api_base_url,
                apiKey: bp.api_key || '',
                models: bp.models || [m],
                defaultModel: m,
                model: m,
              };
            });
            return {
              ...prev,
              providers: backendProviders,
              activeProvider: `provider-${activeIdx}`,
            };
          });
        }
      })
      .catch(() => {});
  }, []);

  // ── Persist settings on change ──
  useEffect(() => {
    saveSettings(settings);
  }, [settings]);

  // ── Sync providers to backend when they change ──
  const mounted = useRef(false);
  useEffect(() => {
    if (!mounted.current) { mounted.current = true; return; }
    const providers = settings.providers.map((p) => ({
      name: p.name,
      api_base_url: p.apiBase,
      api_key: p.apiKey || '',
      models: p.models || [p.model],
    }));
    const activeProv = settings.providers.find((p) => p.id === settings.activeProvider);
    const defaultModel = activeProv
      ? `${activeProv.name}/${activeProv.defaultModel || activeProv.model}`
      : 'DeepSeek/deepseek-v4-flash';
    if (providers.length > 0) {
      updateSettings(providers, defaultModel)
        .catch(() => {});
    }
  }, [settings.providers, settings.activeProvider]);

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

  const handleDeleteConv = useCallback((id) => {
    setConversations((prev) => prev.filter((c) => c.id !== id));
    setActiveConvId((prev) => prev === id ? null : prev);
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
    <div className="h-screen flex overflow-hidden select-none bg-[var(--bg-primary)] text-[var(--text-primary)]">
      {/* Sidebar */}
      <Sidebar
        conversations={conversations}
        activeConvId={activeConvId}
        onNewChat={handleNewChat}
        onSwitchConv={handleSwitchConv}
        onDeleteConv={handleDeleteConv}
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
        chatModel={settings.chatModel}
        onChatModelChange={(m) => handleUpdateSettings({ chatModel: m })}
        theme={theme}
        onThemeToggle={handleThemeToggle}
        language={i18n.language}
        onLanguageToggle={handleLanguageToggle}
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
        activeProvider={settings.activeProvider}
        onProviderChange={(id) => handleUpdateSettings({ activeProvider: id })}
        theme={theme}
        onThemeToggle={handleThemeToggle}
        language={i18n.language}
        onLanguageToggle={handleLanguageToggle}
      />
    </div>
  );
}
