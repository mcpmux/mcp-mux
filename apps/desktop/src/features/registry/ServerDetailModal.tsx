/**
 * Server detail modal for viewing full server information.
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
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

/**
 * Full-screen modal showing registry server details and install actions.
 */
export function ServerDetailModal({
  server,
  onClose,
  onInstall,
  onUninstall,
  isLoading,
}: ServerDetailModalProps) {
  const { t } = useTranslation('registry');
  const [showDefinition, setShowDefinition] = useState(false);

  const hostingType =
    server.hosting_type || (server.transport.type === 'stdio' ? 'local' : 'remote');

  const hostingLabel =
    hostingType === 'local'
      ? t('modal.hostingLocal')
      : hostingType === 'remote'
        ? t('modal.hostingRemote')
        : t('modal.hostingHybrid');

  const authLabel =
    server.auth?.type === 'none'
      ? t('modal.authNone')
      : server.auth?.type === 'api_key'
        ? t('modal.authApiKey')
        : server.auth?.type === 'optional_api_key'
          ? t('modal.authOptionalApiKey')
          : t('modal.authOAuth');

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" onClick={onClose} />

      <div
        className="dropdown-menu relative w-full max-w-lg max-h-[90vh] overflow-hidden animate-in fade-in scale-in duration-150"
        data-testid="registry-server-detail-modal"
      >
        <div className="flex items-start gap-4 p-6 border-b border-[rgb(var(--border))]">
          <div className="flex-shrink-0 flex items-center justify-center">
            <ServerIcon icon={server.icon} className="w-12 h-12 object-contain rounded-lg" />
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <h2 className="text-xl font-bold">{server.name}</h2>
              {server.publisher?.verified && (
                <span className="text-[rgb(var(--info))]" title={t('modal.verifiedPublisher')}>
                  ✓
                </span>
              )}
              {server.badges && server.badges.length > 0 && (
                <div className="flex gap-1.5">
                  {server.badges.includes('official') && (
                    <span className="px-2 py-0.5 text-xs rounded-full bg-blue-500/20 text-blue-600 dark:text-blue-400">
                      {t('card.badges.official')}
                    </span>
                  )}
                  {server.badges.includes('verified') && (
                    <span className="px-2 py-0.5 text-xs rounded-full bg-green-500/20 text-green-600 dark:text-green-400">
                      {t('card.badges.verified')}
                    </span>
                  )}
                  {server.badges.includes('featured') && (
                    <span className="px-2 py-0.5 text-xs rounded-full bg-amber-500/20 text-amber-600 dark:text-amber-400">
                      {t('card.badges.featured')}
                    </span>
                  )}
                  {server.badges.includes('sponsored') && (
                    <span className="px-2 py-0.5 text-xs rounded-full bg-yellow-500/20 text-yellow-600 dark:text-yellow-400">
                      {t('card.badges.sponsored')}
                    </span>
                  )}
                  {server.badges.includes('popular') && (
                    <span className="px-2 py-0.5 text-xs rounded-full bg-red-500/20 text-red-600 dark:text-red-400">
                      {t('card.badges.popular')}
                    </span>
                  )}
                </div>
              )}
            </div>
            {server.publisher?.name && (
              <p className="text-sm text-[rgb(var(--muted))]">
                {t('modal.byPublisher', { name: server.publisher.name })}
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

        <div className="p-6 overflow-y-auto max-h-[60vh] space-y-6">
          {server.sponsored?.enabled && (
            <div className="flex items-center gap-3 p-4 rounded-lg bg-yellow-500/10 border border-yellow-500/30">
              {server.sponsored.sponsor_logo && (
                <img
                  src={server.sponsored.sponsor_logo}
                  alt={t('modal.sponsorAlt')}
                  className="w-8 h-8 rounded"
                />
              )}
              <div className="flex-1 text-sm">
                <span className="text-[rgb(var(--muted))]">{t('modal.sponsoredBy')}</span>
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

          <div>
            <h3 className="text-sm font-semibold mb-2">{t('modal.description')}</h3>
            <p className="text-sm text-[rgb(var(--muted))]">{server.description}</p>
          </div>

          <div>
            <h3 className="text-sm font-semibold mb-2">{t('modal.hosting')}</h3>
            <div className="flex items-center gap-2">
              <span
                className={`px-3 py-1 text-sm rounded-lg ${
                  hostingType === 'local'
                    ? 'bg-purple-500/20 text-purple-600 dark:text-purple-400'
                    : hostingType === 'remote'
                      ? 'bg-blue-500/20 text-blue-600 dark:text-blue-400'
                      : 'bg-indigo-500/20 text-indigo-600 dark:text-indigo-400'
                }`}
              >
                {hostingLabel}
              </span>
              <span className="text-xs text-[rgb(var(--muted))]">({server.transport.type})</span>
            </div>
          </div>

          <div>
            <h3 className="text-sm font-semibold mb-2">{t('modal.authentication')}</h3>
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
                {authLabel}
              </span>
              {server.auth && 'instructions' in server.auth && server.auth.instructions && (
                <p className="text-sm text-[rgb(var(--muted))] mt-2">{server.auth.instructions}</p>
              )}
            </div>
          </div>

          {server.categories.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold mb-2">{t('modal.categories')}</h3>
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

          {server.capabilities && (
            <div>
              <h3 className="text-sm font-semibold mb-2">{t('modal.capabilities')}</h3>
              <div className="flex flex-wrap gap-2">
                {server.capabilities.tools && (
                  <span className="px-2 py-1 text-xs rounded-lg bg-[rgb(var(--surface-hover))] text-[rgb(var(--foreground))]">
                    {t('modal.capTools')}
                  </span>
                )}
                {server.capabilities.resources && (
                  <span className="px-2 py-1 text-xs rounded-lg bg-[rgb(var(--surface-hover))] text-[rgb(var(--foreground))]">
                    {t('modal.capResources')}
                  </span>
                )}
                {server.capabilities.prompts && (
                  <span className="px-2 py-1 text-xs rounded-lg bg-[rgb(var(--surface-hover))] text-[rgb(var(--foreground))]">
                    {t('modal.capPrompts')}
                  </span>
                )}
                {server.capabilities.read_only_mode && (
                  <span className="px-2 py-1 text-xs rounded-lg bg-green-500/20 text-green-600 dark:text-green-400">
                    {t('modal.capReadOnly')}
                  </span>
                )}
              </div>
            </div>
          )}

          {server.installation && (
            <div className="bg-[rgb(var(--surface-hover))] rounded-lg p-4">
              <h3 className="text-sm font-semibold mb-3">{t('modal.installationInfo')}</h3>
              <div className="space-y-2 text-sm">
                {server.installation.difficulty && (
                  <div className="flex items-center gap-2">
                    <span className="text-[rgb(var(--muted))]">{t('modal.difficulty')}</span>
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
                    <span className="text-[rgb(var(--muted))]">{t('modal.time')}</span>
                    <span>{server.installation.estimated_time}</span>
                  </div>
                )}
                {server.installation.prerequisites &&
                  server.installation.prerequisites.length > 0 && (
                    <div>
                      <span className="text-[rgb(var(--muted))]">{t('modal.prerequisites')}</span>
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

          {server.license && (
            <div>
              <h3 className="text-sm font-semibold mb-2">{t('modal.license')}</h3>
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
                    {t('modal.viewLicense')}
                  </a>
                )}
              </div>
            </div>
          )}

          {server.media?.screenshots && server.media.screenshots.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold mb-2">{t('modal.screenshots')}</h3>
              <div className="grid grid-cols-2 gap-2">
                {server.media.screenshots.map((url, i) => (
                  <img
                    key={i}
                    src={url}
                    alt={t('modal.screenshotAlt', { index: i + 1 })}
                    className="w-full h-32 object-cover rounded-lg border border-[rgb(var(--border))]"
                    loading="lazy"
                  />
                ))}
              </div>
            </div>
          )}

          {(server.media?.demo_video || server.changelog_url) && (
            <div className="flex flex-col gap-2">
              {server.media?.demo_video && (
                <a
                  href={server.media.demo_video}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center gap-2 text-sm text-[rgb(var(--primary))] hover:underline"
                >
                  {t('modal.watchDemo')}
                </a>
              )}
              {server.changelog_url && (
                <a
                  href={server.changelog_url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center gap-2 text-sm text-[rgb(var(--primary))] hover:underline"
                >
                  {t('modal.viewChangelog')}
                </a>
              )}
            </div>
          )}

          {server.source.type === 'Registry' && (
            <div>
              <h3 className="text-sm font-semibold mb-2">{t('modal.source')}</h3>
              <p className="text-sm text-[rgb(var(--muted))]">{server.source.name}</p>
            </div>
          )}
        </div>

        <div className="flex justify-end gap-3 p-6 border-t border-[rgb(var(--border))]">
          <button
            onClick={() => setShowDefinition(true)}
            className="flex items-center gap-1.5 px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] hover:bg-[rgb(var(--surface-hover))] transition-colors mr-auto"
            data-testid="registry-view-json-btn"
          >
            <Code className="h-4 w-4 text-[rgb(var(--muted))]" />
            {t('modal.viewJson')}
          </button>
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
          >
            {t('modal.close')}
          </button>
          {server.is_installed ? (
            <button
              onClick={() => onUninstall(server.id)}
              disabled={isLoading}
              className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--error))]/30 text-[rgb(var(--error))] hover:bg-[rgb(var(--error))]/10 transition-colors disabled:opacity-50"
            >
              {t('modal.uninstall')}
            </button>
          ) : (
            <button
              onClick={() => onInstall(server.id)}
              disabled={isLoading}
              className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] hover:bg-[rgb(var(--primary-hover))] transition-colors disabled:opacity-50"
            >
              {t('modal.install')}
            </button>
          )}
        </div>
      </div>

      {showDefinition && (
        <ServerDefinitionModal server={server} onClose={() => setShowDefinition(false)} />
      )}
    </div>
  );
}
