/**
 * Server detail modal for viewing full server information.
 */

import { useState } from 'react';
import { Code } from 'lucide-react';
import type { ServerViewModel } from '../../types/registry';
import { ServerIcon } from '../../components/ServerIcon';
import { ServerDefinitionModal } from '../../components/ServerDefinitionModal';

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
  const [showDefinition, setShowDefinition] = useState(false);

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
          <div className="flex-shrink-0 flex items-center justify-center">
            <ServerIcon icon={server.icon} className="w-12 h-12 object-contain rounded-lg" />
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <h2 className="text-xl font-bold">
                {server.name}
              </h2>
              {server.publisher?.verified && (
                <span className="text-[rgb(var(--info))]" title="Verified Publisher">
                  ✓
                </span>
              )}
              {/* Badges */}
              {server.badges && server.badges.length > 0 && (
                <div className="flex gap-1.5">
                  {server.badges.includes('official') && (
                    <span className="px-2 py-0.5 text-xs rounded-full bg-blue-500/20 text-blue-600 dark:text-blue-400">
                      Official
                    </span>
                  )}
                  {server.badges.includes('verified') && (
                    <span className="px-2 py-0.5 text-xs rounded-full bg-green-500/20 text-green-600 dark:text-green-400">
                      ✓ Verified
                    </span>
                  )}
                  {server.badges.includes('featured') && (
                    <span className="px-2 py-0.5 text-xs rounded-full bg-amber-500/20 text-amber-600 dark:text-amber-400">
                      ⭐ Featured
                    </span>
                  )}
                  {server.badges.includes('sponsored') && (
                    <span className="px-2 py-0.5 text-xs rounded-full bg-yellow-500/20 text-yellow-600 dark:text-yellow-400">
                      Sponsored
                    </span>
                  )}
                  {server.badges.includes('popular') && (
                    <span className="px-2 py-0.5 text-xs rounded-full bg-red-500/20 text-red-600 dark:text-red-400">
                      🔥 Popular
                    </span>
                  )}
                </div>
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
          {/* Sponsored Banner */}
          {server.sponsored?.enabled && (
            <div className="flex items-center gap-3 p-4 rounded-lg bg-yellow-500/10 border border-yellow-500/30">
              {server.sponsored.sponsor_logo && (
                <img src={server.sponsored.sponsor_logo} alt="Sponsor" className="w-8 h-8 rounded" />
              )}
              <div className="flex-1 text-sm">
                <span className="text-[rgb(var(--muted))]">Sponsored by </span>
                {server.sponsored.sponsor_url ? (
                  <a
                    href={server.sponsored.sponsor_url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="font-medium hover:underline text-[rgb(var(--foreground))]"
                  >
                    {server.sponsored.sponsor_name}
                  </a>
                ) : (
                  <span className="font-medium">{server.sponsored.sponsor_name}</span>
                )}
              </div>
            </div>
          )}

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
              Hosting
            </h3>
            <div className="flex items-center gap-2">
              <span
                className={`px-3 py-1 text-sm rounded-lg ${
                  (server.hosting_type || (server.transport.type === 'stdio' ? 'local' : 'remote')) === 'local'
                    ? 'bg-purple-500/20 text-purple-600 dark:text-purple-400'
                    : (server.hosting_type || 'remote') === 'remote'
                    ? 'bg-blue-500/20 text-blue-600 dark:text-blue-400'
                    : 'bg-indigo-500/20 text-indigo-600 dark:text-indigo-400'
                }`}
              >
                {(server.hosting_type || (server.transport.type === 'stdio' ? 'local' : 'remote')) === 'local'
                  ? '💻 Local Process'
                  : (server.hosting_type || 'remote') === 'remote'
                  ? '☁️ Remote Server'
                  : '🔄 Hybrid'}
              </span>
              <span className="text-xs text-[rgb(var(--muted))]">
                ({server.transport.type})
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

          {/* Capabilities */}
          {server.capabilities && (
            <div>
              <h3 className="text-sm font-semibold mb-2">
                Capabilities
              </h3>
              <div className="flex flex-wrap gap-2">
                {server.capabilities.tools && (
                  <span className="px-2 py-1 text-xs rounded-lg bg-[rgb(var(--surface-hover))] text-[rgb(var(--foreground))]">
                    🛠️ Tools
                  </span>
                )}
                {server.capabilities.resources && (
                  <span className="px-2 py-1 text-xs rounded-lg bg-[rgb(var(--surface-hover))] text-[rgb(var(--foreground))]">
                    📁 Resources
                  </span>
                )}
                {server.capabilities.prompts && (
                  <span className="px-2 py-1 text-xs rounded-lg bg-[rgb(var(--surface-hover))] text-[rgb(var(--foreground))]">
                    💬 Prompts
                  </span>
                )}
                {server.capabilities.read_only_mode && (
                  <span className="px-2 py-1 text-xs rounded-lg bg-green-500/20 text-green-600 dark:text-green-400">
                    🛡️ Read-Only (Safe)
                  </span>
                )}
              </div>
            </div>
          )}

          {/* Installation Info */}
          {server.installation && (
            <div className="bg-[rgb(var(--surface-hover))] rounded-lg p-4">
              <h3 className="text-sm font-semibold mb-3">Installation Info</h3>
              <div className="space-y-2 text-sm">
                {server.installation.difficulty && (
                  <div className="flex items-center gap-2">
                    <span className="text-[rgb(var(--muted))]">Difficulty:</span>
                    <span
                      className={`px-2 py-0.5 text-xs rounded-full ${
                        server.installation.difficulty === 'easy'
                          ? 'bg-green-500/20 text-green-600 dark:text-green-400'
                          : server.installation.difficulty === 'moderate'
                          ? 'bg-yellow-500/20 text-yellow-600 dark:text-yellow-400'
                          : 'bg-red-500/20 text-red-600 dark:text-red-400'
                      }`}
                    >
                      {server.installation.difficulty}
                    </span>
                  </div>
                )}
                {server.installation.estimated_time && (
                  <div className="flex items-center gap-2">
                    <span className="text-[rgb(var(--muted))]">Time:</span>
                    <span>{server.installation.estimated_time}</span>
                  </div>
                )}
                {server.installation.prerequisites && server.installation.prerequisites.length > 0 && (
                  <div>
                    <span className="text-[rgb(var(--muted))]">Prerequisites:</span>
                    <ul className="mt-1 ml-4 list-disc list-inside text-[rgb(var(--muted))]">
                      {server.installation.prerequisites.map((prereq, i) => (
                        <li key={i}>{prereq}</li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>
            </div>
          )}

          {/* License */}
          {server.license && (
            <div>
              <h3 className="text-sm font-semibold mb-2">License</h3>
              <div className="flex items-center gap-2">
                <span className="px-3 py-1 text-sm rounded-lg bg-[rgb(var(--surface-hover))] text-[rgb(var(--foreground))]">
                  {server.license}
                </span>
                {server.license_url && (
                  <a
                    href={server.license_url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-sm text-[rgb(var(--primary))] hover:underline"
                  >
                    View License →
                  </a>
                )}
              </div>
            </div>
          )}

          {/* Screenshots */}
          {server.media?.screenshots && server.media.screenshots.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold mb-2">Screenshots</h3>
              <div className="grid grid-cols-2 gap-2">
                {server.media.screenshots.map((url, i) => (
                  <img
                    key={i}
                    src={url}
                    alt={`Screenshot ${i + 1}`}
                    className="w-full h-32 object-cover rounded-lg border border-[rgb(var(--border))]"
                    loading="lazy"
                  />
                ))}
              </div>
            </div>
          )}

          {/* Links */}
          {(server.media?.demo_video || server.changelog_url) && (
            <div className="flex flex-col gap-2">
              {server.media?.demo_video && (
                <a
                  href={server.media.demo_video}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center gap-2 text-sm text-[rgb(var(--primary))] hover:underline"
                >
                  🎥 Watch Demo Video →
                </a>
              )}
              {server.changelog_url && (
                <a
                  href={server.changelog_url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center gap-2 text-sm text-[rgb(var(--primary))] hover:underline"
                >
                  📝 View Changelog →
                </a>
              )}
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
            onClick={() => setShowDefinition(true)}
            className="flex items-center gap-1.5 px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] hover:bg-[rgb(var(--surface-hover))] transition-colors mr-auto"
          >
            <Code className="h-4 w-4 text-[rgb(var(--muted))]" />
            View JSON
          </button>
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

      {showDefinition && (
        <ServerDefinitionModal
          server={server}
          onClose={() => setShowDefinition(false)}
        />
      )}
    </div>
  );
}
