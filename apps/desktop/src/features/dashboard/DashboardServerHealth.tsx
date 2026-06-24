import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { AlertCircle, CheckCircle2, KeyRound, Loader2, Settings2 } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@mcpmux/ui';
import { ServerLogViewer } from '@/components/ServerLogViewer';
import { useNavigateTo, useSetPendingServersFilter } from '@/stores';
import type { AttentionKind, AttentionServer } from './dashboard.helpers';

interface DashboardServerHealthProps {
  attentionServers: AttentionServer[];
  isLoading: boolean;
}

/**
 * Label and icon for each server-health attention bucket.
 */
function attentionPresentation(
  kind: AttentionKind,
  t: TFunction<'dashboard'>
): {
  label: string;
  icon: ReactNode;
  badgeClass: string;
} {
  switch (kind) {
    case 'error':
      return {
        label: t('health.badges.error'),
        icon: <AlertCircle className="h-4 w-4 text-red-500" />,
        badgeClass: 'text-red-600 bg-red-500/10',
      };
    case 'auth_required':
      return {
        label: t('health.badges.authRequired'),
        icon: <KeyRound className="h-4 w-4 text-amber-500" />,
        badgeClass: 'text-amber-700 bg-amber-500/10',
      };
    case 'needs_setup':
      return {
        label: t('health.badges.needsSetup'),
        icon: <Settings2 className="h-4 w-4 text-blue-500" />,
        badgeClass: 'text-blue-700 bg-blue-500/10',
      };
  }
}

/**
 * Lists enabled servers that need operator attention, with a link to My Servers.
 */
export function DashboardServerHealth({
  attentionServers,
  isLoading,
}: DashboardServerHealthProps) {
  const { t } = useTranslation('dashboard');
  const navigateTo = useNavigateTo();
  const setPendingServersFilter = useSetPendingServersFilter();
  const hasIssues = attentionServers.length > 0;
  const [logServer, setLogServer] = useState<{ id: string; name: string } | null>(null);

  return (
    <>
      {logServer && (
        <ServerLogViewer
          serverId={logServer.id}
          serverName={logServer.name}
          onClose={() => setLogServer(null)}
        />
      )}
      <Card data-testid="dashboard-server-health">
        <CardHeader>
          <CardTitle className="text-base">{t('health.title')}</CardTitle>
          <CardDescription>{t('health.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))]">
              <Loader2 className="h-4 w-4 animate-spin" />
              {t('health.loading')}
            </div>
          ) : hasIssues ? (
            <ul className="divide-y divide-[rgb(var(--border-subtle))]">
              {attentionServers.map((server) => {
                const presentation = attentionPresentation(server.kind, t);

                return (
                  <li key={server.serverId} data-testid={`dashboard-attention-${server.serverId}`}>
                    <button
                      type="button"
                      className="flex w-full items-start gap-3 py-3 text-left first:pt-0 last:pb-0 rounded hover:bg-[rgb(var(--bg-subtle))] transition-colors px-1 -mx-1"
                      onClick={() => setLogServer({ id: server.serverId, name: server.displayName })}
                    >
                      <span className="mt-0.5">{presentation.icon}</span>
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="truncate text-sm font-medium">{server.displayName}</span>
                          <span
                            className={`rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide ${presentation.badgeClass}`}
                          >
                            {presentation.label}
                          </span>
                        </div>
                        <p className="mt-0.5 truncate text-xs text-[rgb(var(--muted))]">
                          {server.detail}
                        </p>
                      </div>
                    </button>
                  </li>
                );
              })}
            </ul>
          ) : (
            <div className="flex items-start gap-3 rounded-lg border border-green-500/20 bg-green-500/5 px-4 py-3">
              <CheckCircle2 className="mt-0.5 h-5 w-5 flex-shrink-0 text-green-500" />
              <div>
                <p className="text-sm font-medium">{t('health.allGood.title')}</p>
                <p className="mt-0.5 text-xs text-[rgb(var(--muted))]">
                  {t('health.allGood.description')}
                </p>
              </div>
            </div>
          )}

          <button
            type="button"
            onClick={() => {
              if (hasIssues) setPendingServersFilter('error');
              navigateTo('servers');
            }}
            className="mt-4 text-sm font-medium text-primary-500 hover:text-primary-400"
            data-testid="dashboard-view-all-servers"
          >
            {hasIssues ? t('health.viewAllErrors') : t('health.viewAllServers')}
          </button>
        </CardContent>
      </Card>
    </>
  );
}
