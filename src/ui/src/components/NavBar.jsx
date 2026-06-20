import { useState, useEffect, useRef, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { listGraphs, createGraph, compact, extractDocAsync, getTaskStatus } from '../api';

function Modal({ title, children, onClose }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={onClose}>
      <div className="bg-gray-800 rounded-lg p-6 min-w-96 max-w-lg max-h-[80vh] overflow-y-auto" onClick={(e) => e.stopPropagation()}>
        <div className="text-lg font-semibold text-white mb-4">{title}</div>
        {children}
      </div>
    </div>
  );
}

/** A progress bar component with label. */
function ProgressBar({ percent, label, status }) {
  const clamped = Math.max(0, Math.min(100, percent));
  const barColor = status === 'failed' ? 'bg-red-500' :
                   status === 'completed' ? 'bg-green-500' : 'bg-blue-500';
  return (
    <div className="w-full mb-3">
      <div className="flex justify-between text-xs text-gray-400 mb-1">
        <span>{label}</span>
        <span>{Math.round(clamped)}%</span>
      </div>
      <div className="w-full h-3 bg-gray-700 rounded-full overflow-hidden">
        <div className={`h-full rounded-full transition-all duration-500 ease-out ${barColor}`}
             style={{ width: `${clamped}%` }} />
      </div>
    </div>
  );
}

export default function NavBar({ graph, setGraph, onExtractDone }) {
  const { t, i18n } = useTranslation();
  const [graphs, setGraphs] = useState([]);
  const [theme, setTheme] = useState(() => localStorage.getItem('theme') || 'dark');
  const [showAdd, setShowAdd] = useState(false);
  const [showCompact, setShowCompact] = useState(false);
  const [showExtract, setShowExtract] = useState(false);
  const [extractContent, setExtractContent] = useState('');
  const [extractResult, setExtractResult] = useState(null); // { task_id, status } or task status
  const [extractDoneClean, setExtractDoneClean] = useState(false); // true after completion, reset on input
  const [compactDays, setCompactDays] = useState('7');
  const [newName, setNewName] = useState('');
  const [newTT, setNewTT] = useState(false);
  const pollRef = useRef(null);
  const isSubmitting = useRef(false);

  useEffect(() => { listGraphs().then(d => setGraphs(d.graphs || [])).catch(() => {}); }, []);

  // Cleanup polling on unmount
  useEffect(() => {
    return () => { if (pollRef.current) clearInterval(pollRef.current); };
  }, []);

  const toggleTheme = () => {
    const next = theme === 'dark' ? 'light' : 'dark';
    setTheme(next);
    localStorage.setItem('theme', next);
    document.documentElement.classList.toggle('dark', next === 'dark');
  };

  const switchLang = () => i18n.changeLanguage(i18n.language === 'zh' ? 'en' : 'zh');

  const handleAdd = async () => {
    if (!newName) return;
    await createGraph(newName, newTT);
    setGraphs(await listGraphs().then(d => d.graphs || []));
    setShowAdd(false); setNewName('');
  };

  const handleCompact = async () => {
    const days = parseInt(compactDays) || 7;
    const before = (Date.now() - days * 86400 * 1000) * 1000;
    await compact(before, graph);
    setShowCompact(false);
  };

  // ─── Async Extraction with Polling ─────────────────────────────

  const stopPolling = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  const pollTask = useCallback((taskId) => {
    stopPolling();
    pollRef.current = setInterval(async () => {
      try {
        const task = await getTaskStatus(taskId);
        setExtractResult(task);

        if (task.status === 'completed') {
          stopPolling();
          isSubmitting.current = false;
          setExtractDoneClean(true);
          setExtractContent('');
          onExtractDone?.(task.stats);
        } else if (task.status === 'failed') {
          stopPolling();
          isSubmitting.current = false;
        }
      } catch (e) {
        console.error('Poll error:', e);
        // Don't stop on transient errors, keep polling
      }
    }, 1500);
  }, [stopPolling, onExtractDone]);

  const handleExtract = async () => {
    if (!extractContent.trim() || isSubmitting.current) return;
    isSubmitting.current = true;

    // Show pending state immediately
    setExtractResult({ status: 'submitting', progress: null, stats: null });

    try {
      const res = await extractDocAsync(extractContent, graph);
      // res = { task_id, status: "pending" }
      setExtractResult({ status: 'pending', task_id: res.task_id, progress: null, stats: null });
      // Start polling
      pollTask(res.task_id);
    } catch (e) {
      setExtractResult({ status: 'failed', error: e.message });
      isSubmitting.current = false;
    }
  };

  const handleCloseExtract = () => {
    stopPolling();
    isSubmitting.current = false;
    setShowExtract(false);
    setExtractContent('');
    setExtractResult(null);
    setExtractDoneClean(false);
  };

  // Derive progress bar display
  const taskStatus = extractResult?.status;
  const taskProgress = extractResult?.progress;
  const taskStats = extractResult?.stats;
  const taskError = extractResult?.error;

  const progressPercent = taskProgress
    ? (taskProgress.processed_sections / Math.max(1, taskProgress.total_sections)) * 100
    : 0;

  const isRunning = taskStatus === 'pending' || taskStatus === 'running' || taskStatus === 'submitting';

  return (
    <div className="bg-gray-850 border-b border-gray-700 px-4 py-2 flex items-center gap-3 flex-wrap">
      <span className="text-blue-400 font-bold text-lg mr-2">BG</span>

      {/* Graph selector */}
      <div className="flex items-center gap-1">
        <select className="bg-gray-700 text-gray-200 rounded px-2 py-1 text-sm border border-gray-600" value={graph} onChange={e => setGraph(e.target.value)}>
          {graphs.map(g => <option key={g} value={g}>{g}</option>)}
        </select>
        <button className="text-green-400 hover:text-green-300 text-sm px-1" title={t('nav.addGraph')} onClick={() => setShowAdd(true)}>+</button>
      </div>

      <span className="text-gray-600">|</span>

      {/* Compact / Extract */}
      <button className="text-sm text-gray-400 hover:text-white px-2" onClick={() => setShowCompact(true)}>{t('nav.compact')}</button>
      <button className="text-sm text-gray-400 hover:text-white px-2" onClick={() => setShowExtract(true)}>{t('nav.extract')}</button>

      <div className="flex-1" />

      {/* Theme & Lang */}
      <button className="text-lg" onClick={toggleTheme}>{theme === 'dark' ? '☀️' : '🌙'}</button>
      <button className="text-sm text-gray-400 hover:text-white px-2" onClick={switchLang}>{i18n.language === 'zh' ? 'EN' : '中文'}</button>

      {/* ── Add Graph Modal ── */}
      {showAdd && <Modal title={t('modal.addGraphTitle')} onClose={() => setShowAdd(false)}>
        <input className="w-full px-3 py-2 rounded bg-gray-700 border border-gray-600 text-gray-100 mb-3" placeholder={t('modal.addGraphName')} value={newName} onChange={e => setNewName(e.target.value)} />
        <label className="flex items-center gap-2 text-gray-300 mb-4"><input type="checkbox" checked={newTT} onChange={e => setNewTT(e.target.checked)} /> {t('modal.addGraphTimeTravel')}</label>
        <div className="flex justify-end gap-2"><button className="px-4 py-2 rounded bg-gray-600 text-gray-200" onClick={() => setShowAdd(false)}>{t('modal.addGraphCancel')}</button><button className="px-4 py-2 rounded bg-blue-600 text-white" onClick={handleAdd}>{t('modal.addGraphConfirm')}</button></div>
      </Modal>}

      {/* ── Compact Modal ── */}
      {showCompact && <Modal title={t('modal.compactTitle')} onClose={() => setShowCompact(false)}>
        <p className="text-gray-400 text-sm mb-3">{t('modal.compactBefore')}</p>
        <div className="flex gap-2 mb-4">
          {['1', '7', '30'].map(d => <button key={d} className={`px-3 py-1 rounded text-sm ${compactDays === d ? 'bg-blue-600 text-white' : 'bg-gray-700 text-gray-300'}`} onClick={() => setCompactDays(d)}>{d}d</button>)}
          <input className="w-20 px-2 py-1 rounded bg-gray-700 border border-gray-600 text-gray-100 text-sm" placeholder={t('modal.compactCustom')} value={compactDays} onChange={e => setCompactDays(e.target.value)} />
        </div>
        <div className="flex justify-end"><button className="px-4 py-2 rounded bg-blue-600 text-white" onClick={handleCompact}>{t('modal.compactRun')}</button></div>
      </Modal>}

      {/* ── Extract Modal (Redesigned with Task Progress) ── */}
      {showExtract && <Modal title={t('modal.extractTitle')} onClose={handleCloseExtract}>
        {/* Input area — only show when not running */}
        {!isRunning && (
          <>
            <div className="flex gap-2 mb-3">
              <label className="flex-1 px-3 py-2 rounded bg-gray-700 border border-gray-600 text-gray-400 text-sm cursor-pointer hover:bg-gray-600 text-center">
                📄 {t('modal.extractUpload')}
                <input type="file" accept=".md,.markdown,.txt" className="hidden" onChange={e => {
                  const file = e.target.files?.[0];
                  if (file) file.text().then(t => { setExtractContent(t); setExtractDoneClean(false); });
                }} />
              </label>
            </div>
            <textarea className="w-full h-28 px-3 py-2 rounded bg-gray-700 border border-gray-600 text-gray-100 text-sm mb-3" placeholder={t('modal.extractDrop')} value={extractContent} onChange={e => { setExtractContent(e.target.value); setExtractDoneClean(false); }} />
          </>
        )}

        {/* ── Task Status Display ── */}
        {taskStatus === 'submitting' && (
          <div className="flex items-center gap-2 text-yellow-400 text-sm mb-3">
            <span className="animate-spin">⏳</span>
            <span>{t('modal.taskSubmitting')}</span>
          </div>
        )}

        {taskStatus === 'pending' && (
          <div className="flex items-center gap-2 text-yellow-400 text-sm mb-3">
            <span className="animate-spin">⏳</span>
            <span>{t('modal.taskPending')}</span>
          </div>
        )}

        {(taskStatus === 'running' || (taskProgress && isRunning)) && (
          <div className="mb-3">
            <ProgressBar
              percent={progressPercent}
              label={taskProgress?.current_heading || t('modal.extractProgress')}
              status="running"
            />
            <p className="text-gray-400 text-xs text-center">
              {taskProgress?.processed_sections ?? 0} / {taskProgress?.total_sections ?? '?'} {t('modal.taskSections')}
            </p>
          </div>
        )}

        {taskStatus === 'completed' && taskStats && (
          <div className="mb-3">
            <ProgressBar percent={100} label={t('modal.taskCompleted')} status="completed" />
            <div className="bg-green-900/30 border border-green-700 rounded px-3 py-2 text-green-300 text-sm">
              <p className="font-semibold mb-1">{t('modal.extractDone', { v: taskStats.new_vertices, e: taskStats.new_edges })}</p>
              <div className="text-xs text-green-400/80 space-y-0.5">
                <p>{t('modal.taskSections')}: {taskStats.total_sections} ({taskStats.processed_sections} {t('modal.taskProcessed')})</p>
                <p>{t('modal.taskVertices')}: {taskStats.new_vertices}</p>
                <p>{t('modal.taskEdges')}: {taskStats.new_edges}</p>
                {taskStats.total_prompt_tokens > 0 && <p>{t('modal.taskTokens')}: {taskStats.total_prompt_tokens} ↑ / {taskStats.total_completion_tokens} ↓</p>}
              </div>
            </div>
          </div>
        )}

        {taskStatus === 'failed' && (
          <div className="mb-3">
            <ProgressBar percent={100} label={t('modal.taskFailed')} status="failed" />
            <p className="text-red-400 text-sm bg-red-900/30 border border-red-700 rounded px-3 py-2">{t('modal.extractError')}: {taskError}</p>
          </div>
        )}

        {/* ── Action buttons ── */}
        <div className="flex justify-end gap-2">
          <button className="px-4 py-2 rounded bg-gray-600 text-gray-200" onClick={handleCloseExtract}>
            {isRunning ? t('modal.taskClose') : t('panel.close')}
          </button>
          {!isRunning && (
            <button className="px-4 py-2 rounded bg-blue-600 text-white hover:bg-blue-500 disabled:opacity-50" onClick={handleExtract} disabled={!extractContent.trim() || extractDoneClean}>
              {t('modal.extractRun')}
            </button>
          )}
        </div>
      </Modal>}
    </div>
  );
}
