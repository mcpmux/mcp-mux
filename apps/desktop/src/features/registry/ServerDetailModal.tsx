/**
 * Server detail modal for viewing full server information.
 */

import type { ServerViewModel } from '../../types/registry';

interface ServerDetailModalProps {
  server: ServerViewModel;
  onClose: () => void;
  onInstall: (id: string) => void;
  onUninstall: (id: string) => void;
  isLoading?: boolean;
}

export function ServerDetailModal({
  server,
  onClose,
  onInstall,
  onUninstall,
  isLoading,
}: ServerDetailModalProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={onClose}
      />

      {/* Modal */}
      <div className="dropdown-menu relative w-full max-w-lg max-h-[90vh] overflow-hidden animate-in fade-in scale-in duration-150">
        {/* Header */}
        <div className="flex items-start gap-4 p-6 border-b border-[rgb(var(--border))]">
          <div className="text-5xl">{server.icon || '📦'}</div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2">
              <h2 className="text-xl font-bold">
                {server.name}
              </h2>
              {server.publisher?.verified && (
                <span className="text-[rgb(var(--info))]" title="Verified Publisher">
                  ✓
                </span>
              )}
            </div>
            {server.publisher?.name && (
              <p className="text-sm text-[rgb(var(--muted))]">
                by {server.publisher.name}
              </p>
            )}
          </div>
          <button
            onClick={onClose}
            className="p-2 hover:bg-[rgb(var(--surface-hover))] rounded-lg transition-colors"
          >
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Content */}
        <div className="p-6 overflow-y-auto max-h-[60vh] space-y-6">
          {/* Description */}
          <div>
            <h3 className="text-sm font-semibold mb-2">
              Description
            </h3>
            <p className="text-sm text-[rgb(var(--muted))]">
              {server.description}
            </p>
          </div>

          {/* Transport */}
          <div>
            <h3 className="text-sm font-semibold mb-2">
              Transport
            </h3>
            <div className="flex items-center gap-2">
              <span
                className={`px-3 py-1 text-sm rounded-lg ${
                  server.transport.type === 'stdio'
                    ? 'bg-purple-500/20 text-purple-600 dark:text-purple-400'
                    : 'bg-[rgb(var(--primary))]/20 text-[rgb(var(--primary))]'
                }`}
              >
                {server.transport.type === 'stdio' 
                  ? '🖥️ Local Process (stdio)' 
                  : '🌐 Remote Server (HTTP)'}
              </span>
            </div>
          </div>

          {/* Authentication */}
          <div>
            <h3 className="text-sm font-semibold mb-2">
              Authentication
            </h3>
            <div className="space-y-2">
              <span
                className={`px-3 py-1.5 text-sm font-medium rounded-lg inline-block ${
                  server.auth?.type === 'none'
                    ? 'bg-[rgb(var(--success))] text-white'
                    : server.auth?.type === 'api_key'
                    ? 'bg-[rgb(var(--warning))] text-white'
                    : server.auth?.type === 'optional_api_key'
                    ? 'bg-[rgb(var(--warning))]/80 text-white'
                    : 'bg-[rgb(var(--info))] text-white'
                }`}
              >
                {server.auth?.type === 'none'
                  ? '✅ No authentication required'
                  : server.auth?.type === 'api_key'
                  ? '🔑 API Key Required'
                  : server.auth?.type === 'optional_api_key'
                  ? '🔑 API Key (Optional)'
                  : '🔐 OAuth Authentication'}
              </span>
              {server.auth && 'instructions' in server.auth && server.auth.instructions && (
                <p className="text-sm text-[rgb(var(--muted))] mt-2">
                  {server.auth.instructions}
                </p>
              )}
            </div>
          </div>

          {/* Categories */}
          {server.categories.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold mb-2">
                Categories
              </h3>
              <div className="flex flex-wrap gap-2">
                {server.categories.map((cat) => (
                  <span
                    key={cat}
                    className="px-3 py-1 text-sm rounded-lg bg-[rgb(var(--primary))]/20 text-[rgb(var(--primary))]"
                  >
                    {cat}
                  </span>
                ))}
              </div>
            </div>
          )}

          {/* Source */}
          {server.source.type === 'Registry' && (
            <div>
              <h3 className="text-sm font-semibold mb-2">
                Source
              </h3>
              <p className="text-sm text-[rgb(var(--muted))]">
                {server.source.name}
              </p>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-3 p-6 border-t border-[rgb(var(--border))]">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
          >
            Close
          </button>
          {server.is_installed ? (
            <button
              onClick={() => onUninstall(server.id)}
              disabled={isLoading}
              className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--error))]/30 text-[rgb(var(--error))] hover:bg-[rgb(var(--error))]/10 transition-colors disabled:opacity-50"
            >
              Uninstall
            </button>
          ) : (
            <button
              onClick={() => onInstall(server.id)}
              disabled={isLoading}
              className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] hover:bg-[rgb(var(--primary-hover))] transition-colors disabled:opacity-50"
            >
              Install
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
