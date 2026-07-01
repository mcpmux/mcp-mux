/**
 * Server card component for displaying a registry server.
 */

import { useTranslation } from 'react-i18next';
import type { ServerViewModel } from '../../types/registry';
import { ServerIcon } from '../../components/ServerIcon';

interface ServerCardProps {
  server: ServerViewModel;
  onInstall: (id: string) => void;
  onUninstall: (id: string) => void;
  onViewDetails: (server: ServerViewModel) => void;
  isLoading?: boolean;
}

/**
 * Renders a registry server summary card with install/uninstall actions.
 */
export function ServerCard({
  server,
  onInstall,
  onUninstall,
  onViewDetails,
  isLoading,
}: ServerCardProps) {
  const { t } = useTranslation('registry');

  const getAuthBadge = () => {
    const authType = server.auth?.type || 'none';
    switch (authType) {
      case 'none':
        return (
          <span className="px-2 py-0.5 text-xs rounded-full bg-[rgb(var(--success))]/20 text-[rgb(var(--success))]">
            {t('card.auth.none')}
          </span>
        );
      case 'api_key':
        return (
          <span className="px-2 py-0.5 text-xs rounded-full bg-[rgb(var(--warning))]/20 text-[rgb(var(--warning))]">
            {t('card.auth.apiKey')}
          </span>
        );
      case 'optional_api_key':
        return (
          <span className="px-2 py-0.5 text-xs rounded-full bg-[rgb(var(--warning))]/30 text-[rgb(var(--warning))]">
            {t('card.auth.optionalApiKey')}
          </span>
        );
      case 'oauth':
        return (
          <span className="px-2 py-0.5 text-xs rounded-full bg-[rgb(var(--info))]/20 text-[rgb(var(--info))]">
            {t('card.auth.oauth')}
          </span>
        );
    }
  };

  const getTransportBadge = () => {
    const hostingType =
      server.hosting_type || (server.transport.type === 'stdio' ? 'local' : 'remote');

    const config = {
      local: {
        icon: '💻',
        label: t('card.hosting.local'),
        bg: 'bg-purple-500/20',
        text: 'text-purple-600 dark:text-purple-400',
      },
      remote: {
        icon: '☁️',
        label: t('card.hosting.remote'),
        bg: 'bg-blue-500/20',
        text: 'text-blue-600 dark:text-blue-400',
      },
      hybrid: {
        icon: '🔄',
        label: t('card.hosting.hybrid'),
        bg: 'bg-indigo-500/20',
        text: 'text-indigo-600 dark:text-indigo-400',
      },
    }[hostingType];

    return (
      <span className={`px-2 py-0.5 text-xs rounded-full ${config.bg} ${config.text}`}>
        {config.icon} {config.label}
      </span>
    );
  };

  const getBadges = () => {
    if (!server.badges || server.badges.length === 0) return null;

    const badgeConfig: Record<string, { label: string; bg: string; text: string }> = {
      official: {
        label: t('card.badges.official'),
        bg: 'bg-blue-500/20',
        text: 'text-blue-600 dark:text-blue-400',
      },
      verified: {
        label: t('card.badges.verified'),
        bg: 'bg-green-500/20',
        text: 'text-green-600 dark:text-green-400',
      },
      featured: {
        label: t('card.badges.featured'),
        bg: 'bg-amber-500/20',
        text: 'text-amber-600 dark:text-amber-400',
      },
      sponsored: {
        label: t('card.badges.sponsored'),
        bg: 'bg-yellow-500/20',
        text: 'text-yellow-600 dark:text-yellow-400',
      },
      popular: {
        label: t('card.badges.popular'),
        bg: 'bg-red-500/20',
        text: 'text-red-600 dark:text-red-400',
      },
    };

    return (
      <>
        {server.badges.slice(0, 2).map((badge) => {
          const config = badgeConfig[badge];
          if (!config) return null;
          return (
            <span
              key={badge}
              className={`px-2 py-0.5 text-xs rounded-full ${config.bg} ${config.text}`}
            >
              {config.label}
            </span>
          );
        })}
      </>
    );
  };

  return (
    <div
      className="group relative bg-[rgb(var(--card))] border border-[rgb(var(--border-subtle))] rounded-xl p-5 
                 hover:border-[rgb(var(--primary))]/50 hover:shadow-lg
                 transition-all duration-200 cursor-pointer shadow-sm"
      onClick={() => onViewDetails(server)}
      data-testid={`server-card-${server.id}`}
    >
      <div className="flex items-start gap-3 mb-3">
        <div className="text-3xl flex-shrink-0 flex items-center justify-center">
          <ServerIcon icon={server.icon} />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5">
            <h3 className="font-semibold truncate" title={server.name} data-testid={`server-name-${server.id}`}>
              {server.name}
            </h3>
            {server.publisher?.verified && (
              <span className="text-[rgb(var(--info))] flex-shrink-0" title={t('card.verifiedPublisher')}>
                ✓
              </span>
            )}
          </div>
          {server.publisher?.name && (
            <p className="text-xs text-[rgb(var(--muted))] truncate">
              {t('card.byPublisher', { name: server.publisher.name })}
            </p>
          )}
        </div>
      </div>

      <p className="text-sm text-[rgb(var(--muted))] line-clamp-2 mb-4">{server.description}</p>

      <div className="flex flex-wrap gap-2 mb-4">
        {getBadges()}
        {getTransportBadge()}
        {getAuthBadge()}
        {server.capabilities?.read_only_mode && (
          <span className="px-2 py-0.5 text-xs rounded-full bg-green-500/20 text-green-600 dark:text-green-400">
            {t('card.readOnly')}
          </span>
        )}
      </div>

      {server.categories.length > 0 && (
        <div className="flex flex-wrap gap-1 mb-4">
          {server.categories.slice(0, 3).map((cat) => (
            <span
              key={cat}
              className="px-2 py-0.5 text-xs rounded bg-[rgb(var(--surface-hover))] text-[rgb(var(--muted))]"
            >
              {cat}
            </span>
          ))}
          {server.categories.length > 3 && (
            <span className="px-2 py-0.5 text-xs text-[rgb(var(--muted))]">
              +{server.categories.length - 3}
            </span>
          )}
        </div>
      )}

      <div className="flex justify-end">
        {server.is_installed ? (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onUninstall(server.id);
            }}
            disabled={isLoading}
            className="px-4 py-1.5 text-sm rounded-lg border border-[rgb(var(--error))]/30 text-[rgb(var(--error))]
                       hover:bg-[rgb(var(--error))]/10 transition-colors disabled:opacity-50"
            data-testid={`uninstall-btn-${server.id}`}
          >
            {t('card.uninstall')}
          </button>
        ) : (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onInstall(server.id);
            }}
            disabled={isLoading}
            className="px-4 py-1.5 text-sm rounded-lg bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))]
                       hover:bg-[rgb(var(--primary-hover))] transition-colors disabled:opacity-50"
            data-testid={`install-btn-${server.id}`}
          >
            {t('card.install')}
          </button>
        )}
      </div>
    </div>
  );
}
