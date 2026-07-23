import { useState, useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import Sidebar from './components/Sidebar';
import ChatArea from './components/ChatArea';
import SettingsDialog from './components/SettingsDialog';
import GraphManagerDialog from './components/GraphManagerDialog';
import KnowledgeBase from './components/KnowledgeBase';
import { listGraphs, fetchLlmSettings, updateLlmSettings } from './api';

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
  activeProvider: 'DeepSeek',
  defaultGraph: '',
  timeTravel: false,
  timeTravelPoint: '',
  useGraph: false,
  chatModel: null,
  defaultModelKey: '',
};

// Keys persisted to localStorage (user preferences only — no system config)
const USER_PREFS_KEYS = ['activeProvider','defaultGraph','timeTravel','timeTravelPoint','useGraph','chatModel'];

function loadSettings() {
  try {
    const raw = localStorage.getItem('bggraph-chat-settings');
    if (raw) {
      const parsed = JSON.parse(raw);
      // Only pick user-preference keys, ignore anything else (e.g. stale providers)
      const prefs = {};
      for (const k of USER_PREFS_KEYS) {
        if (k in parsed) prefs[k] = parsed[k];
      }
      return { ...DEFAULT_SETTINGS, ...prefs };
    }
  } catch {}
  return { ...DEFAULT_SETTINGS };
}

function saveSettings(settings) {
  // Only persist user-preference keys, never system config
  const toSave = {};
  for (const k of USER_PREFS_KEYS) {
    if (k in settings) toSave[k] = settings[k];
  }
  localStorage.setItem('bggraph-chat-settings', JSON.stringify(toSave));
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
  const [graphManagerOpen, setGraphManagerOpen] = useState(false);
  const [knowledgeBaseOpen, setKnowledgeBaseOpen] = useState(false);
  const [kbInitialContent, setKbInitialContent] = useState('');
  const [kbInitialGraph, setKbInitialGraph] = useState('');
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
        const metas = d.graphs || [];
        setGraphs(metas);
        const apiDefault = d.default || '';
        // If no default set yet, or stored default not in list, use API default
        if (!settings.defaultGraph || !metas.some(g => g.name === settings.defaultGraph)) {
          setSettings((s) => ({ ...s, defaultGraph: apiDefault || (metas[0]?.name || '') }));
        }
      })
      .catch(() => {});

    // Load backend LLM providers to sync frontend defaults
    fetchLlmSettings()
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
                id: bp.name,  // use name as stable id
                name: bp.name,
                apiBase: bp.api_base_url,
                apiKey: bp.api_key || '',
                models: bp.models || [m],
                defaultModel: m,
                model: m,
              };
            });
            const chosenActiveProvider = prev.activeProvider && backendProviders.some(p => p.name === prev.activeProvider)
              ? prev.activeProvider
              : backendProviders[activeIdx]?.name || backendProviders[0]?.name || '';
            const chosenActiveProv = backendProviders.find(p => p.name === chosenActiveProvider);
            const defaultKey = llm.default_model || '';
            return {
              ...prev,
              providers: backendProviders,
              defaultModelKey: defaultKey,
              activeProvider: chosenActiveProvider,
              chatModel: prev.chatModel || (chosenActiveProv ? (chosenActiveProv.defaultModel || chosenActiveProv.model) : null),
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

  // ── Sync provider configs to backend when user saves/deletes a provider ──
  const handleUpdateProviders = useCallback((providers) => {
    setSettings((prev) => {
      const activeProv = providers.find(p => p.name === prev.activeProvider) || providers[0];
      const newDefaultKey = activeProv ? `${activeProv.name}/${activeProv.defaultModel || activeProv.model}` : (prev.defaultModelKey || '');
      return {
        ...prev,
        providers,
        defaultModelKey: newDefaultKey,
        activeProvider: activeProv ? activeProv.name : (providers.length > 0 ? providers[0].name : null),
      };
    });
    // Sync to backend on user action (not on page load)
    const backendProviders = providers.map((p) => ({
      name: p.name,
      api_base_url: p.apiBase,
      api_key: p.apiKey || '',
      models: p.models || [p.model],
    }));
    if (backendProviders.length > 0) {
      updateLlmSettings(backendProviders).catch(() => {});
    }
  }, []);

  const handleThemeToggle = useCallback(() => {
    setTheme((prev) => (prev === 'dark' ? 'light' : 'dark'));
  }, []);

  const handleLanguageToggle = useCallback((lang) => {
    i18n.changeLanguage(lang || (i18n.language === 'zh' ? 'en' : 'zh'));
  }, [i18n]);

  // Initialize first conversation if none
  useEffect(() => {
    if (conversations.length === 0) {
      handleNewChat();
    }
  }, [conversations.length, handleNewChat]);

  return (
    <div className="h-screen flex overflow-hidden select-none bg-[var(--bg-primary)] text-[var(--text-primary)]" data-version="2">
      {/* Sidebar */}
      <Sidebar
        conversations={conversations}
        activeConvId={activeConvId}
        onNewChat={handleNewChat}
        onSwitchConv={handleSwitchConv}
        onDeleteConv={handleDeleteConv}
        onOpenSettings={() => setSettingsOpen(true)}
        onOpenGraphManager={() => setGraphManagerOpen(true)}
        onOpenKnowledgeBase={() => setKnowledgeBaseOpen(true)}
      />

      {/* Main chat area */}
      <ChatArea
        activeConv={activeConv}
        onUpdateConv={handleUpdateConv}
        providers={settings.providers}
        activeProvider={settings.activeProvider}
        onProviderChange={(name) => handleUpdateSettings({ activeProvider: name })}
        useGraph={settings.useGraph}
        onGraphToggle={(v) => handleUpdateSettings({ useGraph: v })}
        timeTravel={settings.timeTravel}
        onTimeTravelToggle={(v) => handleUpdateSettings({ timeTravel: v })}
        timeTravelPoint={settings.timeTravelPoint}
        onTimeTravelPointChange={(v) => handleUpdateSettings({ timeTravelPoint: v })}
        defaultGraph={settings.defaultGraph}
        onDefaultGraphChange={(g) => handleUpdateSettings({ defaultGraph: g })}
        graphs={graphs.map(g => g.name).filter(Boolean)}
        graphMetas={graphs}
        timeTravelGraphs={{}}
        defaultModelKey={settings.defaultModelKey}
        chatModel={settings.chatModel}
        onChatModelChange={(m) => handleUpdateSettings({ chatModel: m })}
        theme={theme}
        onThemeToggle={handleThemeToggle}
        language={i18n.language}
        onLanguageToggle={handleLanguageToggle}
        onSaveToKB={(content) => {
          setKbInitialContent(content);
          setKbInitialGraph(settings.defaultGraph);
          setKnowledgeBaseOpen(true);
        }}
      />

      {/* Knowledge Base dialog */}
      <KnowledgeBase
        open={knowledgeBaseOpen}
        onClose={() => { setKnowledgeBaseOpen(false); setKbInitialContent(''); }}
        providers={settings.providers}
        activeProvider={settings.activeProvider}
        defaultGraph={settings.defaultGraph}
        theme={theme}
        initialContent={kbInitialContent}
        initialGraph={kbInitialGraph}
      />

      {/* Graph Manager dialog */}
      <GraphManagerDialog
        open={graphManagerOpen}
        onClose={() => setGraphManagerOpen(false)}
        graphName={settings.defaultGraph}
        onGraphNameChange={(g) => handleUpdateSettings({ defaultGraph: g })}
        graphMetas={graphs}
        onGraphMetasChange={setGraphs}
      />

      {/* Settings dialog */}
      <SettingsDialog
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        providers={settings.providers}
        onUpdateProviders={handleUpdateProviders}
        activeProvider={settings.activeProvider}
        onProviderChange={(name) => handleUpdateSettings({ activeProvider: name })}
        theme={theme}
        onThemeToggle={handleThemeToggle}
        language={i18n.language}
        onLanguageToggle={handleLanguageToggle}
      />
    </div>
  );
}
