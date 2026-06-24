import { useCallback, useEffect, useState, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { X, Download, Trash2, RefreshCw, Copy } from 'lucide-react';
import { useToast, ToastContainer, useConfirm } from '@mcpmux/ui';
import { getServerLogs, clearServerLogs, getServerLogFile, type ServerLogEntry } from '@/lib/api/logs';

interface ServerLogViewerProps {
  serverId: string;
  serverName: string;
  onClose: () => void;
}

const LOG_LEVELS = ['trace', 'debug', 'info', 'warn', 'error'] as const;
type LogLevel = typeof LOG_LEVELS[number];

const LEVEL_COLORS: Record<LogLevel, string> = {
  trace: 'text-gray-400',
  debug: 'text-blue-400',
  info: 'text-green-400',
  warn: 'text-yellow-400',
  error: 'text-red-400',
};

const SOURCE_COLORS: Record<string, string> = {
  app: 'text-purple-400',
  stdout: 'text-primary-400',
  stderr: 'text-orange-400',
  'http-request': 'text-blue-300',
  'http-response': 'text-blue-400',
  'sse-event': 'text-indigo-400',
  connection: 'text-green-300',
  oauth: 'text-pink-400',
  server: 'text-cyan-400',
};

/**
 * Formats an ISO timestamp for log display and export.
 */
function formatTimestamp(ts: string): string {
  const date = new Date(ts);
  const hours = date.getHours().toString().padStart(2, '0');
  const minutes = date.getMinutes().toString().padStart(2, '0');
  const seconds = date.getSeconds().toString().padStart(2, '0');
  const ms = date.getMilliseconds().toString().padStart(3, '0');
  return `${hours}:${minutes}:${seconds}.${ms}`;
}

/**
 * Formats a log entry as a single plain-text line for display export.
 */
function formatLogLine(log: ServerLogEntry): string {
  const level = log.level.toUpperCase().padEnd(5);
  const base = `${formatTimestamp(log.timestamp)}  ${level}  ${log.source}  ${log.message}`;
  if (!log.metadata) {
    return base;
  }
  return `${base}  ${JSON.stringify(log.metadata)}`;
}

export function ServerLogViewer({ serverId, serverName, onClose }: ServerLogViewerProps) {
  const { t } = useTranslation('settings');
  const { t: tCommon } = useTranslation('common');
  const [logs, setLogs] = useState<ServerLogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [levelFilter, setLevelFilter] = useState<LogLevel | 'all'>('all');
  const [autoRefresh, setAutoRefresh] = useState(false);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const shouldScrollRef = useRef(true);
  const { toasts, success, error: showError, dismiss } = useToast();
  const { confirm, ConfirmDialogElement } = useConfirm();

  const loadLogs = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const fetchedLogs = await getServerLogs(
        serverId,
        500, // Load last 500 logs
        levelFilter === 'all' ? undefined : levelFilter
      );
      setLogs(fetchedLogs);
      
      // Auto-scroll to bottom if user was at bottom
      if (shouldScrollRef.current && scrollContainerRef.current) {
        setTimeout(() => {
          scrollContainerRef.current?.scrollTo({
            top: scrollContainerRef.current.scrollHeight,
            behavior: 'smooth',
          });
        }, 100);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [levelFilter, serverId]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  useEffect(() => {
    void loadLogs();
  }, [loadLogs]);

  // Auto-refresh every 2 seconds if enabled
  useEffect(() => {
    if (!autoRefresh) return;

    const interval = setInterval(() => {
      void loadLogs();
    }, 2000);

    return () => clearInterval(interval);
  }, [autoRefresh, loadLogs]);

  // Track scroll position
  const handleScroll = () => {
    if (!scrollContainerRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollContainerRef.current;
    // If within 100px of bottom, auto-scroll
    shouldScrollRef.current = scrollHeight - scrollTop - clientHeight < 100;
  };

  const handleClearLogs = async () => {
    if (!await confirm({
      title: t('logs.viewer.confirm.clearTitle'),
      message: t('logs.viewer.confirm.clearMessage', { serverName }),
      confirmLabel: t('logs.viewer.confirm.clearConfirm'),
      cancelLabel: tCommon('actions.cancel'),
      variant: 'danger',
    })) {
      return;
    }
    
    try {
      await clearServerLogs(serverId);
      setLogs([]);
      success(
        t('logs.viewer.toast.cleared'),
        t('logs.viewer.toast.clearedBody', { serverName }),
      );
    } catch (e) {
      showError(
        t('logs.viewer.toast.clearFailed'),
        e instanceof Error ? e.message : String(e),
      );
    }
  };

  const handleOpenInEditor = async () => {
    try {
      const filePath = await getServerLogFile(serverId);
      await navigator.clipboard.writeText(filePath);
      success(t('logs.viewer.toast.pathCopied'), t('logs.viewer.toast.pathCopiedBody'));
    } catch (e) {
      showError(
        t('logs.viewer.toast.pathFailed'),
        e instanceof Error ? e.message : String(e),
      );
    }
  };

  const filteredLogs = logs.filter(log => {
    if (levelFilter === 'all') return true;
    const logLevelIndex = LOG_LEVELS.indexOf(log.level as LogLevel);
    const filterLevelIndex = LOG_LEVELS.indexOf(levelFilter);
    return logLevelIndex >= filterLevelIndex;
  });

  /** Copies all currently visible (filtered) log lines to the clipboard. */
  const handleCopyAll = async () => {
    if (filteredLogs.length === 0) {
      showError(
        t('logs.viewer.toast.nothingToCopy'),
        t('logs.viewer.toast.nothingToCopyBody'),
      );
      return;
    }

    try {
      const text = filteredLogs.map((log) => formatLogLine(log)).join('\n');
      await navigator.clipboard.writeText(text);
      success(
        t('logs.viewer.toast.logsCopiedTitle'),
        t('logs.viewer.toast.logsCopied', { count: filteredLogs.length }),
      );
    } catch (e) {
      showError(
        t('logs.viewer.toast.copyFailed'),
        e instanceof Error ? e.message : String(e),
      );
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <ToastContainer toasts={toasts} onClose={dismiss} />
      {ConfirmDialogElement}
      <div className="bg-[rgb(var(--card))] border border-[rgb(var(--border-subtle))] rounded-xl shadow-xl w-[90vw] h-[85vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-[rgb(var(--border-subtle))]">
          <div className="flex items-center gap-3">
            <h2 className="text-lg font-semibold">{t('logs.viewer.title')}</h2>
            <span className="text-sm text-[rgb(var(--muted))]">{serverName}</span>
          </div>
          <div className="flex items-center gap-2">
            {/* Level Filter */}
            <select
              value={levelFilter}
              onChange={(e) => setLevelFilter(e.target.value as LogLevel | 'all')}
              className="px-3 py-1.5 text-sm bg-[rgb(var(--surface-elevated))] border border-[rgb(var(--border-subtle))] rounded-lg"
            >
              <option value="all">{t('logs.viewer.allLevels')}</option>
              {LOG_LEVELS.map(level => (
                <option key={level} value={level}>
                  {level.toUpperCase()}
                </option>
              ))}
            </select>
            
            {/* Refresh with auto-refresh toggle */}
            <div className="flex items-center gap-1">
              <button
                onClick={loadLogs}
                disabled={loading}
                className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors"
                title={t('logs.viewer.refreshTitle')}
              >
                <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
              </button>
              <button
                onClick={() => setAutoRefresh(!autoRefresh)}
                className={`px-2 py-1.5 text-xs rounded-lg border transition-colors ${
                  autoRefresh
                    ? 'bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] border-[rgb(var(--primary))]'
                    : 'bg-[rgb(var(--surface-elevated))] border-[rgb(var(--border-subtle))] text-[rgb(var(--muted))]'
                }`}
                title={t('logs.viewer.autoRefreshTitle')}
              >
                {t('logs.viewer.autoRefresh')}
              </button>
            </div>
            
            {/* Open in Editor */}
            <button
              onClick={handleOpenInEditor}
              className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors"
              title={t('logs.viewer.openInEditorTitle')}
            >
              <Download className="h-4 w-4" />
            </button>

            {/* Copy All */}
            <button
              onClick={handleCopyAll}
              disabled={filteredLogs.length === 0}
              className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors disabled:opacity-40 disabled:pointer-events-none"
              title={t('logs.viewer.copyAllTitle')}
            >
              <Copy className="h-4 w-4" />
            </button>
            
            {/* Clear Logs */}
            <button
              onClick={handleClearLogs}
              className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors text-red-400"
              title={t('logs.viewer.clearTitle')}
            >
              <Trash2 className="h-4 w-4" />
            </button>
            
            {/* Close */}
            <button
              onClick={onClose}
              className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        {/* Log Content */}
        <div
          ref={scrollContainerRef}
          onScroll={handleScroll}
          className="flex-1 overflow-y-auto p-4 font-mono text-sm"
        >
          {loading && logs.length === 0 ? (
            <div className="flex items-center justify-center h-full">
              <div className="animate-spin rounded-full h-8 w-8 border-2 border-[rgb(var(--primary))] border-t-transparent" />
            </div>
          ) : error ? (
            <div className="text-red-400">{error}</div>
          ) : filteredLogs.length === 0 ? (
            <div className="text-center text-[rgb(var(--muted))] py-12">
              {t('logs.viewer.noLogs')}
            </div>
          ) : (
            <div className="space-y-1">
              {filteredLogs.map((log, idx) => {
                const levelColor = LEVEL_COLORS[log.level as LogLevel] || 'text-gray-400';
                const sourceColor = SOURCE_COLORS[log.source] || 'text-gray-500';
                
                return (
                  <div
                    key={idx}
                    className="flex gap-3 hover:bg-[rgb(var(--surface-hover))] px-2 py-1 rounded"
                  >
                    <span className="text-[rgb(var(--muted))] shrink-0">
                      {formatTimestamp(log.timestamp)}
                    </span>
                    <span className={`shrink-0 w-16 ${levelColor}`}>
                      {log.level.toUpperCase().padEnd(5)}
                    </span>
                    <span className={`shrink-0 w-24 ${sourceColor}`}>
                      {log.source}
                    </span>
                    <span className="flex-1 break-words">{log.message}</span>
                    {log.metadata && (
                      <details className="shrink-0">
                        <summary className="cursor-pointer text-[rgb(var(--muted))] text-xs">
                          ...
                        </summary>
                        <pre className="mt-1 text-xs bg-[rgb(var(--surface-elevated))] p-2 rounded overflow-x-auto">
                          {JSON.stringify(log.metadata, null, 2)}
                        </pre>
                      </details>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="p-3 border-t border-[rgb(var(--border-subtle))] text-xs text-[rgb(var(--muted))] flex items-center justify-between">
          <span>
            {t('logs.viewer.logCount', { count: filteredLogs.length })}
            {levelFilter !== 'all' &&
              t('logs.viewer.filteredFrom', { total: logs.length })}
          </span>
          {autoRefresh && (
            <span className="flex items-center gap-2">
              <span className="h-2 w-2 bg-[rgb(var(--primary))] rounded-full animate-pulse" />
              {t('logs.viewer.autoRefreshing')}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}

